//! Typed materialization of the sealed native ownership arena.
//!
//! Fixed CVS executes ordinary nodes as `pre -> children -> post`, while
//! `term_tbl()` and `term_eqn()` own their complete recursive invocation
//! graphs. The arena preserves that distinction. This module consumes it
//! without flattening definition HEAD/BODY segments into blocks first and
//! without rediscovering structure from rendered geometry.

use std::collections::{BTreeMap, BTreeSet};

use libmandoc_rs::ExecutionNodeKey;
use mant_ir::{Block, ContentBlockStep, DefinitionLayout, Inline, SourceSpan};

use super::{
    super::ownership::{
        ChildRole, DefinitionSegmentKind, FinalConsumer, ProjectionArena, ProjectionDestination,
        ProjectionNode, ProjectionNodeKey, ProjectionNodeKind, ProjectionPiece, RawRecordRef,
        SectionIndex, SectionKey, SectionLocation, SpecializedRecipe, StreamEvent,
    },
    InlineProjector, Location, SectionPlan,
    bindings::{
        SemanticBindingError, SemanticBindingRegistry, SemanticItemAddress, StableSemanticItem,
        StableSemanticOwner,
    },
};

use crate::mandoc::native_execution::{NativeProjection, native_node_wrapper_index};

mod frame;
mod native;
pub(super) use native::NativeRecipeEmitter;

#[derive(Clone, Debug)]
pub(super) struct MaterializedTerm {
    pub(super) owner: StableSemanticOwner,
    pub(super) content: Vec<Inline>,
    pub(super) evidence: crate::definitions::ExactNativeDefinitionEvidence,
}

#[derive(Debug)]
pub(super) struct MaterializedMdocItem {
    pub(super) item: StableSemanticItem,
    pub(super) flow_epoch: usize,
    pub(super) leading: Vec<Inline>,
    pub(super) term: MaterializedTerm,
    pub(super) bodies: Vec<MaterializedChunk>,
    pub(super) layout: DefinitionLayout,
    pub(super) source: Option<SourceSpan>,
}

#[derive(Debug)]
pub(super) struct MaterializedManItem {
    pub(super) item: StableSemanticItem,
    pub(super) presentation: super::man::Presentation,
    pub(super) origin_columns: i32,
    pub(super) terms: Vec<MaterializedTerm>,
    pub(super) description: MaterializedChunk,
    pub(super) layout: DefinitionLayout,
    pub(super) source: Option<SourceSpan>,
}

#[derive(Debug)]
pub(super) struct MaterializedManContinuation {
    pub(super) owner: ExecutionNodeKey,
    pub(super) head: InlineProduct,
    pub(super) body: MaterializedChunk,
}

#[derive(Debug)]
struct PendingSemanticItem {
    item: StableSemanticItem,
    owner_path: Vec<ContentBlockStep>,
    terms: Vec<MaterializedTerm>,
}

/// Blocks plus semantic bindings relative to the first returned block.
#[derive(Debug, Default)]
pub(super) struct MaterializedChunk {
    pub(super) blocks: Vec<Block>,
    semantic_items: Vec<PendingSemanticItem>,
    /// Native logical line commits waiting for the next emitted product.
    /// `true` means that the committed formatter row itself was empty.
    pending_line_commits: Vec<bool>,
    /// A typed structural boundary forbids paragraph coalescing across this
    /// chunk edge even when the native formatter has no blank device row.
    barrier_before: bool,
    barrier_after: bool,
}

impl MaterializedChunk {
    pub(super) fn from_blocks(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            semantic_items: Vec::new(),
            pending_line_commits: Vec::new(),
            barrier_before: false,
            barrier_after: false,
        }
    }

    fn is_empty(&self) -> bool {
        self.blocks.is_empty()
            && self.semantic_items.is_empty()
            && self.pending_line_commits.is_empty()
    }

    pub(super) fn append(&mut self, mut other: Self) -> Result<(), MaterializationError> {
        let self_was_empty = self.is_empty();
        let other_was_empty = other.is_empty();
        let old_len = self.blocks.len();
        let mut inserted_before = 0usize;
        let mut merged_first = false;
        if !self.barrier_after
            && !other.barrier_before
            && self.pending_line_commits.is_empty()
            && let (Some(last), Some(first)) = (self.blocks.last_mut(), other.blocks.first_mut())
            && merge_inline_blocks(last, first, 0)
        {
            // Stream pieces are ownership units, not paragraph boundaries.
            // Fixed CVS keeps adjacent words in one formatter flow until a
            // native line/boundary event says otherwise.
            other.blocks.remove(0);
            merged_first = true;
        } else if !self.pending_line_commits.is_empty() && !other.blocks.is_empty() {
            if let (Some(last), Some(first)) = (self.blocks.last_mut(), other.blocks.first_mut())
                && merge_inline_blocks(last, first, self.pending_line_commits.len())
            {
                other.blocks.remove(0);
                merged_first = true;
            } else if self.blocks.is_empty()
                && prepend_line_breaks(&mut other.blocks[0], self.pending_line_commits.len())
            {
                // The exact leading native line commits remain inline in the
                // first content block.
            } else {
                inserted_before = self
                    .pending_line_commits
                    .iter()
                    .filter(|empty| **empty || self.blocks.is_empty())
                    .count();
                self.blocks
                    .extend((0..inserted_before).map(|_| Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    }));
            }
            self.pending_line_commits.clear();
        }
        let base = if merged_first {
            old_len.saturating_sub(1)
        } else {
            old_len.saturating_add(inserted_before)
        };
        for pending in &mut other.semantic_items {
            remap_first_block(&mut pending.owner_path, base, merged_first)?;
        }
        self.blocks.append(&mut other.blocks);
        self.semantic_items.append(&mut other.semantic_items);
        self.pending_line_commits
            .append(&mut other.pending_line_commits);
        if self_was_empty && !other_was_empty {
            self.barrier_before = other.barrier_before;
        }
        if !other_was_empty {
            self.barrier_after = other.barrier_after;
        }
        Ok(())
    }

    pub(super) fn set_inline_barriers(&mut self, before: bool, after: bool) {
        if !self.is_empty() {
            self.barrier_before = before;
            self.barrier_after = after;
        }
    }

    pub(super) fn line_commit(&mut self, empty: bool) {
        self.pending_line_commits.push(empty);
    }

    pub(super) fn blank_device_line(&mut self, source: Option<SourceSpan>) {
        self.settle_pending_before_blank(source);
        self.blocks.push(Block::VerticalSpace { lines: 1, source });
    }

    pub(super) fn settle_before_structural(&mut self, source: Option<SourceSpan>) {
        self.settle_pending_before_blank(source);
    }

    fn settle_pending_before_blank(&mut self, source: Option<SourceSpan>) {
        let extra = self
            .pending_line_commits
            .drain(..)
            .filter(|empty| *empty || self.blocks.is_empty())
            .count();
        self.blocks
            .extend((0..extra).map(|_| Block::VerticalSpace { lines: 1, source }));
    }

    fn finish_pending(&mut self) {
        self.settle_pending_before_blank(None);
    }

    pub(super) fn register_definition_item(
        &mut self,
        item: StableSemanticItem,
        owner_path: Vec<ContentBlockStep>,
        terms: Vec<MaterializedTerm>,
    ) -> Result<(), MaterializationError> {
        if terms.is_empty() {
            return Err(MaterializationError::DefinitionWithoutTerms(item));
        }
        if !matches!(
            owner_path.last(),
            Some(ContentBlockStep::DefinitionItem { .. })
        ) {
            return Err(MaterializationError::InvalidSemanticPath(item));
        }
        self.semantic_items.push(PendingSemanticItem {
            item,
            owner_path,
            terms,
        });
        Ok(())
    }

    pub(super) fn nest(mut self, prefix: &[ContentBlockStep]) -> Self {
        for pending in &mut self.semantic_items {
            let mut path = Vec::with_capacity(prefix.len() + pending.owner_path.len());
            path.extend_from_slice(prefix);
            path.append(&mut pending.owner_path);
            pending.owner_path = path;
        }
        self
    }
}

fn inline_children_mut(block: &mut Block) -> Option<&mut Vec<Inline>> {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => Some(children),
        _ => None,
    }
}

fn same_inline_block_kind(left: &Block, right: &Block) -> bool {
    matches!(
        (left, right),
        (Block::Paragraph { .. }, Block::Paragraph { .. })
            | (Block::Preformatted { .. }, Block::Preformatted { .. })
    )
}

fn merge_inline_blocks(left: &mut Block, right: &mut Block, breaks: usize) -> bool {
    if !same_inline_block_kind(left, right) {
        return false;
    }
    let Some(left) = inline_children_mut(left) else {
        return false;
    };
    let Some(right) = inline_children_mut(right) else {
        return false;
    };
    left.extend((0..breaks).map(|_| Inline::LineBreak));
    left.append(right);
    true
}

fn prepend_line_breaks(block: &mut Block, breaks: usize) -> bool {
    let Some(children) = inline_children_mut(block) else {
        return false;
    };
    children.splice(0..0, (0..breaks).map(|_| Inline::LineBreak));
    true
}

fn remap_first_block(
    path: &mut [ContentBlockStep],
    base: usize,
    merged_first: bool,
) -> Result<(), MaterializationError> {
    let Some(ContentBlockStep::Block { index }) = path.first_mut() else {
        return Err(MaterializationError::MissingRelativeBlockPath);
    };
    let local = usize::try_from(*index).map_err(|_| MaterializationError::BlockIndexOverflow)?;
    let mapped = if merged_first {
        base.checked_add(local)
    } else {
        base.checked_add(local)
    }
    .ok_or(MaterializationError::BlockIndexOverflow)?;
    *index = u32::try_from(mapped).map_err(|_| MaterializationError::BlockIndexOverflow)?;
    Ok(())
}

#[derive(Debug, Default)]
pub(super) struct InlineProduct {
    pub(super) content: Vec<Inline>,
}

impl InlineProduct {
    fn append(&mut self, mut other: Self) {
        self.content.append(&mut other.content);
    }
}

#[derive(Debug)]
pub(super) enum NodeProduct {
    Blocks(MaterializedChunk),
    Inline(InlineProduct),
    MdocItem(MaterializedMdocItem),
    ManItem(MaterializedManItem),
    ManContinuation(MaterializedManContinuation),
}

#[derive(Debug, Default)]
pub(super) struct MaterializedDestinations {
    pub(super) root: Vec<Block>,
    pub(super) sections: BTreeMap<SectionKey, Vec<Block>>,
    pub(super) headings: BTreeMap<SectionKey, Vec<Inline>>,
    pub(super) bindings: SemanticBindingRegistry,
    chunks: BTreeMap<ProjectionDestination, MaterializedChunk>,
}

impl MaterializedDestinations {
    fn blocks_mut(
        &mut self,
        destination: ProjectionDestination,
    ) -> Result<&mut Vec<Block>, MaterializationError> {
        match destination {
            ProjectionDestination::Root => Ok(&mut self.root),
            ProjectionDestination::Section(key) => Ok(self.sections.entry(key).or_default()),
            ProjectionDestination::Deferred => Err(MaterializationError::DeferredDestination),
        }
    }

    fn append_chunk(
        &mut self,
        destination: ProjectionDestination,
        chunk: MaterializedChunk,
    ) -> Result<(), MaterializationError> {
        if destination == ProjectionDestination::Deferred {
            return Err(MaterializationError::DeferredDestination);
        }
        self.chunks.entry(destination).or_default().append(chunk)
    }

    fn finish_chunks(&mut self) -> Result<(), MaterializationError> {
        for (destination, mut chunk) in std::mem::take(&mut self.chunks) {
            chunk.finish_pending();
            for pending in chunk.semantic_items.drain(..) {
                let address = SemanticItemAddress::new(destination, pending.owner_path)
                    .map_err(MaterializationError::SemanticBinding)?;
                self.bindings
                    .register_item(pending.item, address)
                    .map_err(MaterializationError::SemanticBinding)?;
                for (ordinal, term) in pending.terms.into_iter().enumerate() {
                    let ordinal = u32::try_from(ordinal)
                        .map_err(|_| MaterializationError::BlockIndexOverflow)?;
                    self.bindings
                        .bind_owner(term.owner, pending.item, ordinal)
                        .map_err(MaterializationError::SemanticBinding)?;
                    self.bindings
                        .register_evidence(term.owner, term.evidence)
                        .map_err(MaterializationError::SemanticBinding)?;
                }
            }
            *self.blocks_mut(destination)? = chunk.blocks;
        }
        Ok(())
    }
}

/// Product output and the raw records actually used to create it.
#[derive(Debug)]
pub(super) struct Emission<T> {
    pub(super) value: T,
    pub(super) consumed: Vec<RawRecordRef>,
}

/// Typed proof that a specialized producer materialized the complete native
/// recipe graph.  Raw execution records stay private to the sealed arena;
/// successfully covering this graph redeems that arena-issued ticket.
#[derive(Debug)]
pub(super) struct SpecializedEmission<T> {
    pub(super) value: T,
    pub(super) covered: Vec<SpecializedRecipe>,
    pub(super) anchors: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamMode {
    Blocks,
    Inline,
}

pub(super) trait RecipeEmitter {
    type Error;

    fn heading(&mut self, node: &ProjectionNode) -> Result<Emission<Vec<Inline>>, Self::Error>;

    fn stream(
        &mut self,
        owner: &ProjectionNode,
        mode: StreamMode,
        events: &[StreamEvent],
    ) -> Result<Emission<NodeProduct>, Self::Error>;

    fn specialized_root(
        &mut self,
        owner: &ProjectionNode,
        recipe: SpecializedRecipe,
    ) -> Result<SpecializedEmission<MaterializedChunk>, Self::Error>;

    fn mdoc_item(
        &mut self,
        owner: &ProjectionNode,
        leading: InlineProduct,
        head: InlineProduct,
        bodies: Vec<MaterializedChunk>,
    ) -> Result<MaterializedMdocItem, Self::Error>;

    fn mdoc_list(
        &mut self,
        owner: &ProjectionNode,
        leading: MaterializedChunk,
        items: Vec<MaterializedMdocItem>,
    ) -> Result<MaterializedChunk, Self::Error>;

    fn man_item(
        &mut self,
        owner: &ProjectionNode,
        head: InlineProduct,
        body: MaterializedChunk,
        continuations: Vec<MaterializedManContinuation>,
    ) -> Result<MaterializedManItem, Self::Error>;

    fn man_run(
        &mut self,
        owner: &ProjectionNode,
        leading: MaterializedChunk,
        items: Vec<MaterializedManItem>,
    ) -> Result<MaterializedChunk, Self::Error>;

    fn ordinary_scope(
        &mut self,
        owner: &ProjectionNode,
        children: MaterializedChunk,
    ) -> Result<MaterializedChunk, Self::Error>;
}

#[derive(Debug)]
pub(super) enum MaterializationError {
    DeferredDestination,
    UnknownNode(ProjectionNodeKey),
    NodeVisitedTwice(ProjectionNodeKey),
    ChildDestinationMismatch {
        parent: ProjectionNodeKey,
        child: ProjectionNodeKey,
    },
    ChildRoleMismatch {
        parent: ProjectionNodeKey,
        child: ProjectionNodeKey,
        role: ChildRole,
    },
    ProductTypeMismatch {
        node: ProjectionNodeKey,
        expected: &'static str,
    },
    MissingHead(ProjectionNodeKey),
    DuplicateHead(ProjectionNodeKey),
    DuplicateBody(ProjectionNodeKey),
    MissingSpecializedRoot(ProjectionNodeKey),
    InvalidSpecializedRecipe {
        node: ProjectionNodeKey,
        recipe: SpecializedRecipe,
    },
    SpecializedDescendantEscaped {
        root: ProjectionNodeKey,
        descendant: ProjectionNodeKey,
    },
    MissingConsumerReceipt(FinalConsumer),
    ConsumerReceiptMismatch(FinalConsumer),
    ConsumerRecordUsedTwice {
        consumer: FinalConsumer,
        record: RawRecordRef,
    },
    ConsumerReceiptUsedTwice(FinalConsumer),
    UnusedConsumerReceipt(FinalConsumer),
    DefinitionWithoutTerms(StableSemanticItem),
    InvalidSemanticPath(StableSemanticItem),
    MissingRelativeBlockPath,
    BlockIndexOverflow,
    SemanticBinding(SemanticBindingError),
    Backend(String),
}

impl MaterializationError {
    fn backend(error: impl std::fmt::Display) -> Self {
        Self::Backend(error.to_string())
    }
}

impl std::fmt::Display for MaterializationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeferredDestination => formatter.write_str("projection destination is deferred"),
            Self::UnknownNode(node) => write!(formatter, "unknown projection node {node:?}"),
            Self::NodeVisitedTwice(node) => {
                write!(formatter, "projection node {node:?} was visited twice")
            }
            Self::ChildDestinationMismatch { parent, child } => write!(
                formatter,
                "child {child:?} has a different destination from parent {parent:?}"
            ),
            Self::ChildRoleMismatch {
                parent,
                child,
                role,
            } => write!(
                formatter,
                "child {child:?} does not satisfy role {role:?} for parent {parent:?}"
            ),
            Self::ProductTypeMismatch { node, expected } => {
                write!(formatter, "node {node:?} did not materialize as {expected}")
            }
            Self::MissingHead(node) => write!(formatter, "node {node:?} has no head"),
            Self::DuplicateHead(node) => write!(formatter, "node {node:?} has multiple heads"),
            Self::DuplicateBody(node) => write!(formatter, "node {node:?} has multiple bodies"),
            Self::MissingSpecializedRoot(node) => {
                write!(formatter, "specialized node {node:?} has no root recipe")
            }
            Self::InvalidSpecializedRecipe { node, recipe } => write!(
                formatter,
                "specialized node {node:?} cannot consume recipe {recipe:?}"
            ),
            Self::SpecializedDescendantEscaped { root, descendant } => write!(
                formatter,
                "specialized descendant {descendant:?} escaped root {root:?}"
            ),
            Self::MissingConsumerReceipt(consumer) => {
                write!(formatter, "consumer {consumer:?} has no sealed receipt")
            }
            Self::ConsumerReceiptMismatch(consumer) => {
                write!(
                    formatter,
                    "consumer {consumer:?} did not redeem its sealed receipt"
                )
            }
            Self::ConsumerRecordUsedTwice { consumer, record } => write!(
                formatter,
                "consumer {consumer:?} used record {record:?} more than once"
            ),
            Self::ConsumerReceiptUsedTwice(consumer) => {
                write!(
                    formatter,
                    "consumer {consumer:?} redeemed its receipt twice"
                )
            }
            Self::UnusedConsumerReceipt(consumer) => {
                write!(formatter, "consumer {consumer:?} left its receipt unused")
            }
            Self::DefinitionWithoutTerms(item) => {
                write!(formatter, "semantic item {item:?} has no terms")
            }
            Self::InvalidSemanticPath(item) => {
                write!(
                    formatter,
                    "semantic item {item:?} has an invalid owner path"
                )
            }
            Self::MissingRelativeBlockPath => {
                formatter.write_str("semantic owner path has no relative block")
            }
            Self::BlockIndexOverflow => formatter.write_str("materialized block index overflow"),
            Self::SemanticBinding(error) => write!(formatter, "semantic binding failed: {error:?}"),
            Self::Backend(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for MaterializationError {}

pub(super) fn section_index(
    report: &libmandoc_rs::NativeExecutionReport,
    projection: &NativeProjection,
    sections: &SectionPlan,
) -> Result<(SectionIndex, BTreeMap<SectionKey, Location>), MaterializationError> {
    if projection.headings.len() != sections.heading_locations.len() {
        return Err(MaterializationError::Backend(
            "native headings and section plan have different cardinality".to_owned(),
        ));
    }
    let mut locations = Vec::with_capacity(projection.headings.len());
    let mut output = BTreeMap::new();
    for (ordinal, (heading, location)) in projection
        .headings
        .iter()
        .zip(&sections.heading_locations)
        .enumerate()
    {
        let key = SectionKey(
            u32::try_from(ordinal).map_err(|_| MaterializationError::BlockIndexOverflow)?,
        );
        let wrappers = native_node_wrapper_index(report);
        let section_wrapper = wrappers
            .get(heading.section.0 as usize)
            .and_then(|wrapper| *wrapper)
            .ok_or_else(|| MaterializationError::Backend("section lacks native wrapper".into()))?;
        let section_wrapper = &report.wrappers()[section_wrapper];
        locations.push(SectionLocation {
            key,
            heading_wrapper: heading.wrapper,
            enter_sequence: section_wrapper.enter_sequence,
            leave_sequence: section_wrapper.leave_sequence,
        });
        output.insert(key, *location);
    }
    let index = SectionIndex::from_locations(locations)
        .map_err(|error| MaterializationError::Backend(format!("{error:?}")))?;
    Ok((index, output))
}

pub(super) fn materialize<E>(
    arena: &ProjectionArena,
    emitter: &mut E,
) -> Result<MaterializedDestinations, MaterializationError>
where
    E: RecipeEmitter,
    E::Error: std::fmt::Display,
{
    let receipts = arena
        .final_consumers
        .iter()
        .map(|receipt| (receipt.consumer, receipt.records.clone()))
        .collect::<BTreeMap<_, _>>();
    if receipts.len() != arena.final_consumers.len() {
        return Err(MaterializationError::Backend(
            "duplicate final-consumer receipt".to_owned(),
        ));
    }
    let mut walker = Materializer {
        arena,
        emitter,
        visited: BTreeSet::new(),
        consumed_receipts: BTreeSet::new(),
        receipts,
        output: MaterializedDestinations::default(),
    };
    let mut headings = arena
        .nodes
        .iter()
        .filter(|node| matches!(node.kind, ProjectionNodeKind::Heading { .. }))
        .map(|node| node.key)
        .collect::<Vec<_>>();
    headings.sort_by_key(|key| {
        let node = arena.node(*key);
        (node.span.enter_sequence, node.span.leave_sequence, key.0)
    });
    for heading in headings {
        walker.walk_heading(heading)?;
    }
    for root in &arena.roots {
        walker.walk_root(*root)?;
    }
    if walker.visited.len() != arena.nodes.len() {
        let missing = arena
            .nodes
            .iter()
            .find(|node| !walker.visited.contains(&node.key))
            .map(|node| node.key)
            .unwrap_or(ProjectionNodeKey(u32::MAX));
        return Err(MaterializationError::UnknownNode(missing));
    }
    if let Some(consumer) = walker
        .receipts
        .keys()
        .find(|consumer| !walker.consumed_receipts.contains(consumer))
        .copied()
    {
        return Err(MaterializationError::UnusedConsumerReceipt(consumer));
    }
    walker.output.finish_chunks()?;
    Ok(walker.output)
}

struct Materializer<'a, E> {
    arena: &'a ProjectionArena,
    emitter: &'a mut E,
    visited: BTreeSet<ProjectionNodeKey>,
    receipts: BTreeMap<FinalConsumer, Vec<RawRecordRef>>,
    consumed_receipts: BTreeSet<FinalConsumer>,
    output: MaterializedDestinations,
}

impl<E> Materializer<'_, E>
where
    E: RecipeEmitter,
    E::Error: std::fmt::Display,
{
    fn node(&self, key: ProjectionNodeKey) -> Result<&ProjectionNode, MaterializationError> {
        self.arena
            .nodes
            .get(key.0 as usize)
            .filter(|node| node.key == key)
            .ok_or(MaterializationError::UnknownNode(key))
    }

    fn enter(&mut self, key: ProjectionNodeKey) -> Result<(), MaterializationError> {
        if !self.visited.insert(key) {
            return Err(MaterializationError::NodeVisitedTwice(key));
        }
        Ok(())
    }

    fn consume<T>(
        &mut self,
        consumer: FinalConsumer,
        mut emission: Emission<T>,
    ) -> Result<T, MaterializationError> {
        if !self.consumed_receipts.insert(consumer) {
            return Err(MaterializationError::ConsumerReceiptUsedTwice(consumer));
        }
        let expected = self
            .receipts
            .get(&consumer)
            .ok_or(MaterializationError::MissingConsumerReceipt(consumer))?;
        emission.consumed.sort_unstable();
        if let Some(window) = emission
            .consumed
            .windows(2)
            .find(|window| window[0] == window[1])
        {
            return Err(MaterializationError::ConsumerRecordUsedTwice {
                consumer,
                record: window[0],
            });
        }
        if &emission.consumed != expected {
            return Err(MaterializationError::ConsumerReceiptMismatch(consumer));
        }
        Ok(emission.value)
    }

    fn consume_specialized<T>(
        &mut self,
        root: ProjectionNodeKey,
        mut emission: SpecializedEmission<T>,
    ) -> Result<T, MaterializationError> {
        let consumer = FinalConsumer::Specialized(root);
        if !self.consumed_receipts.insert(consumer) {
            return Err(MaterializationError::ConsumerReceiptUsedTwice(consumer));
        }
        let receipt = self
            .arena
            .specialized_absorptions
            .iter()
            .find(|receipt| receipt.root == root)
            .ok_or(MaterializationError::MissingConsumerReceipt(consumer))?;
        let mut expected = receipt
            .descendants
            .iter()
            .filter_map(|key| specialized_recipe_for_node(self.arena.node(*key).kind))
            .collect::<Vec<_>>();
        expected.sort_by_key(specialized_recipe_sort_key);
        emission.covered.sort_by_key(specialized_recipe_sort_key);
        if emission.covered != expected {
            return Err(MaterializationError::ConsumerReceiptMismatch(consumer));
        }
        let mut expected_anchors = receipt
            .records
            .iter()
            .filter_map(|record| match record {
                RawRecordRef::Anchor(key) => Some(*key),
                _ => None,
            })
            .collect::<Vec<_>>();
        expected_anchors.sort_unstable();
        emission.anchors.sort_unstable();
        if emission.anchors != expected_anchors {
            return Err(MaterializationError::ConsumerReceiptMismatch(consumer));
        }
        // The typed producer cannot forge this raw ticket: it never receives
        // the record list.  Coverage of the sealed recipe graph redeems it.
        if !self.receipts.contains_key(&consumer) {
            return Err(MaterializationError::MissingConsumerReceipt(consumer));
        }
        Ok(emission.value)
    }

    fn walk_heading(&mut self, key: ProjectionNodeKey) -> Result<(), MaterializationError> {
        self.enter(key)?;
        let node = self.node(key)?.clone();
        let ProjectionNodeKind::Heading { .. } = node.kind else {
            return Err(MaterializationError::UnknownNode(key));
        };
        let ProjectionDestination::Section(section) = node.destination else {
            return Err(MaterializationError::DeferredDestination);
        };
        let emission = self
            .emitter
            .heading(&node)
            .map_err(MaterializationError::backend)?;
        let heading = self.consume(FinalConsumer::Heading(key), emission)?;
        if self.output.headings.insert(section, heading).is_some() {
            return Err(MaterializationError::DuplicateHead(key));
        }
        self.absorb_children(&node)
    }

    fn walk_root(&mut self, key: ProjectionNodeKey) -> Result<(), MaterializationError> {
        let node = self.node(key)?.clone();
        if node.kind != ProjectionNodeKind::Root {
            let destination = node.destination;
            let product = self.walk_node(key)?;
            self.output
                .append_chunk(destination, expect_blocks(product, key)?)?;
            return Ok(());
        }
        self.enter(key)?;
        for (piece_index, piece) in node.pieces.iter().enumerate() {
            let destination = piece_destination(piece);
            let product = self.walk_piece(&node, piece_index, piece, StreamMode::Blocks)?;
            self.output
                .append_chunk(destination, expect_blocks(product, key)?)?;
        }
        Ok(())
    }

    fn walk_node(&mut self, key: ProjectionNodeKey) -> Result<NodeProduct, MaterializationError> {
        self.enter(key)?;
        let node = self.node(key)?.clone();
        match node.kind {
            ProjectionNodeKind::Heading { .. } => {
                Err(MaterializationError::NodeVisitedTwice(node.key))
            }
            ProjectionNodeKind::Table(_) | ProjectionNodeKind::Equation(_) => {
                self.walk_specialized_root(&node)
            }
            ProjectionNodeKind::TableRow(_)
            | ProjectionNodeKind::TableCellInvocation(_)
            | ProjectionNodeKind::EquationInvocation(_) => {
                Err(MaterializationError::SpecializedDescendantEscaped {
                    root: node.execution_parent.unwrap_or(node.key),
                    descendant: node.key,
                })
            }
            ProjectionNodeKind::DefinitionSegment { kind } => self.walk_segment(&node, kind),
            ProjectionNodeKind::MdocItem { .. } => self.walk_mdoc_item(&node),
            ProjectionNodeKind::MdocList { .. } => self.walk_mdoc_list(&node),
            ProjectionNodeKind::ManDefinitionItem { .. }
            | ProjectionNodeKind::ManHangingPair { .. } => self.walk_man_item(&node),
            ProjectionNodeKind::ManDefinitionContinuation { .. } => {
                self.walk_man_continuation(&node)
            }
            ProjectionNodeKind::ManDefinitionRun { .. } => self.walk_man_run(&node),
            #[cfg(test)]
            ProjectionNodeKind::Flow => self.walk_block_scope(&node),
            ProjectionNodeKind::Root | ProjectionNodeKind::NativeScope { .. } => {
                self.walk_block_scope(&node)
            }
        }
    }

    fn walk_piece(
        &mut self,
        owner: &ProjectionNode,
        piece_index: usize,
        piece: &ProjectionPiece,
        mode: StreamMode,
    ) -> Result<NodeProduct, MaterializationError> {
        match piece {
            ProjectionPiece::Stream { events, .. } => {
                let emission = self
                    .emitter
                    .stream(owner, mode, events)
                    .map_err(MaterializationError::backend)?;
                self.consume(
                    FinalConsumer::Stream {
                        owner: owner.key,
                        piece: u32::try_from(piece_index)
                            .map_err(|_| MaterializationError::BlockIndexOverflow)?,
                    },
                    emission,
                )
            }
            ProjectionPiece::Child {
                node,
                role,
                destination,
                ..
            } => {
                let child_destination = self.node(*node)?.destination;
                if *destination != child_destination {
                    return Err(MaterializationError::ChildDestinationMismatch {
                        parent: owner.key,
                        child: *node,
                    });
                }
                let product = self.walk_node(*node)?;
                validate_child_role(owner.key, *node, *role, &product)?;
                Ok(product)
            }
            ProjectionPiece::Specialized { recipe, .. } => {
                Err(MaterializationError::InvalidSpecializedRecipe {
                    node: owner.key,
                    recipe: *recipe,
                })
            }
        }
    }

    fn walk_block_scope(
        &mut self,
        node: &ProjectionNode,
    ) -> Result<NodeProduct, MaterializationError> {
        let mut chunk = MaterializedChunk::default();
        for (index, piece) in node.pieces.iter().enumerate() {
            let product = self.walk_piece(node, index, piece, StreamMode::Blocks)?;
            match product {
                NodeProduct::Blocks(value) => chunk.append(value)?,
                _ => {
                    return Err(MaterializationError::ProductTypeMismatch {
                        node: node.key,
                        expected: "block sequence",
                    });
                }
            }
        }
        if matches!(node.kind, ProjectionNodeKind::NativeScope { .. }) {
            chunk = self
                .emitter
                .ordinary_scope(node, chunk)
                .map_err(MaterializationError::backend)?;
        }
        Ok(NodeProduct::Blocks(chunk))
    }

    fn walk_segment(
        &mut self,
        node: &ProjectionNode,
        kind: DefinitionSegmentKind,
    ) -> Result<NodeProduct, MaterializationError> {
        let inline = matches!(
            kind,
            DefinitionSegmentKind::MdocHead | DefinitionSegmentKind::ManHead
        );
        if !inline {
            return self.walk_block_scope(node);
        }
        let mut output = InlineProduct::default();
        for (index, piece) in node.pieces.iter().enumerate() {
            let product = self.walk_piece(node, index, piece, StreamMode::Inline)?;
            match product {
                NodeProduct::Inline(value) => output.append(value),
                _ => {
                    return Err(MaterializationError::ProductTypeMismatch {
                        node: node.key,
                        expected: "definition head inline sequence",
                    });
                }
            }
        }
        Ok(NodeProduct::Inline(output))
    }

    fn walk_mdoc_item(
        &mut self,
        node: &ProjectionNode,
    ) -> Result<NodeProduct, MaterializationError> {
        let mut leading = InlineProduct::default();
        let mut head = None;
        let mut bodies = BTreeMap::<u32, MaterializedChunk>::new();
        for (index, piece) in node.pieces.iter().enumerate() {
            let mode = if matches!(piece_role(piece), Some(ChildRole::MdocBody { .. })) {
                StreamMode::Blocks
            } else {
                StreamMode::Inline
            };
            let product = self.walk_piece(node, index, piece, mode)?;
            match (piece_role(piece), product) {
                (None, NodeProduct::Inline(value)) => leading.append(value),
                (Some(ChildRole::MdocHead), NodeProduct::Inline(value)) => {
                    if head.replace(value).is_some() {
                        return Err(MaterializationError::DuplicateHead(node.key));
                    }
                }
                (Some(ChildRole::MdocBody { ordinal }), NodeProduct::Blocks(value)) => {
                    if bodies.insert(ordinal, value).is_some() {
                        return Err(MaterializationError::DuplicateBody(node.key));
                    }
                }
                _ => {
                    return Err(MaterializationError::ProductTypeMismatch {
                        node: node.key,
                        expected: "mdoc item head/body",
                    });
                }
            }
        }
        let item = self
            .emitter
            .mdoc_item(
                node,
                leading,
                head.ok_or(MaterializationError::MissingHead(node.key))?,
                bodies.into_values().collect(),
            )
            .map_err(MaterializationError::backend)?;
        Ok(NodeProduct::MdocItem(item))
    }

    fn walk_mdoc_list(
        &mut self,
        node: &ProjectionNode,
    ) -> Result<NodeProduct, MaterializationError> {
        let mut leading = MaterializedChunk::default();
        let mut items = Vec::new();
        for (index, piece) in node.pieces.iter().enumerate() {
            let product = self.walk_piece(node, index, piece, StreamMode::Blocks)?;
            match product {
                NodeProduct::Blocks(value) if piece_role(piece).is_none() => {
                    leading.append(value)?
                }
                NodeProduct::MdocItem(value) => items.push(value),
                _ => {
                    return Err(MaterializationError::ProductTypeMismatch {
                        node: node.key,
                        expected: "mdoc list item",
                    });
                }
            }
        }
        Ok(NodeProduct::Blocks(
            self.emitter
                .mdoc_list(node, leading, items)
                .map_err(MaterializationError::backend)?,
        ))
    }

    fn walk_man_item(
        &mut self,
        node: &ProjectionNode,
    ) -> Result<NodeProduct, MaterializationError> {
        let mut heads = Vec::new();
        let mut body = None;
        let mut continuations = Vec::new();
        for (index, piece) in node.pieces.iter().enumerate() {
            let role = piece_role(piece);
            let mode = if matches!(role, Some(ChildRole::ManHead)) {
                StreamMode::Inline
            } else {
                StreamMode::Blocks
            };
            let product = self.walk_piece(node, index, piece, mode)?;
            match (role, product) {
                (Some(ChildRole::ManHead), NodeProduct::Inline(value)) => heads.push(value),
                (Some(ChildRole::ManContinuation), NodeProduct::ManContinuation(value)) => {
                    continuations.push(value);
                }
                (Some(ChildRole::ManBody), NodeProduct::Blocks(value)) => {
                    if body.replace(value).is_some() {
                        return Err(MaterializationError::DuplicateBody(node.key));
                    }
                }
                (Some(ChildRole::ManBodyContinuation), NodeProduct::Blocks(value)) => {
                    body.get_or_insert_with(MaterializedChunk::default)
                        .append(value)?;
                }
                (None, NodeProduct::Blocks(value)) if value.is_empty() => {}
                (None, NodeProduct::Blocks(value)) => {
                    body.get_or_insert_with(MaterializedChunk::default)
                        .append(value)?;
                }
                _ => {
                    return Err(MaterializationError::ProductTypeMismatch {
                        node: node.key,
                        expected: "man definition item",
                    });
                }
            }
        }
        if heads.is_empty() {
            return Err(MaterializationError::MissingHead(node.key));
        }
        if heads.len() != 1 {
            return Err(MaterializationError::DuplicateHead(node.key));
        }
        Ok(NodeProduct::ManItem(
            self.emitter
                .man_item(
                    node,
                    heads.pop().expect("checked exactly one man head"),
                    body.unwrap_or_default(),
                    continuations,
                )
                .map_err(MaterializationError::backend)?,
        ))
    }

    fn walk_man_continuation(
        &mut self,
        node: &ProjectionNode,
    ) -> Result<NodeProduct, MaterializationError> {
        let ProjectionNodeKind::ManDefinitionContinuation { owner } = node.kind else {
            unreachable!();
        };
        let mut head = None;
        let mut body = None;
        for (index, piece) in node.pieces.iter().enumerate() {
            let role = piece_role(piece);
            let mode = if matches!(role, Some(ChildRole::ManHead)) {
                StreamMode::Inline
            } else {
                StreamMode::Blocks
            };
            let product = self.walk_piece(node, index, piece, mode)?;
            match (role, product) {
                (Some(ChildRole::ManHead), NodeProduct::Inline(value)) => {
                    if head.replace(value).is_some() {
                        return Err(MaterializationError::DuplicateHead(node.key));
                    }
                }
                (Some(ChildRole::ManBody), NodeProduct::Blocks(value)) => {
                    if body.replace(value).is_some() {
                        return Err(MaterializationError::DuplicateBody(node.key));
                    }
                }
                (None, NodeProduct::Blocks(value)) if value.is_empty() => {}
                (None, NodeProduct::Blocks(value)) => {
                    body.get_or_insert_with(MaterializedChunk::default)
                        .append(value)?;
                }
                _ => {
                    return Err(MaterializationError::ProductTypeMismatch {
                        node: node.key,
                        expected: "man continuation head/body",
                    });
                }
            }
        }
        Ok(NodeProduct::ManContinuation(MaterializedManContinuation {
            owner,
            head: head.ok_or(MaterializationError::MissingHead(node.key))?,
            body: body.unwrap_or_default(),
        }))
    }

    fn walk_man_run(&mut self, node: &ProjectionNode) -> Result<NodeProduct, MaterializationError> {
        let mut leading = MaterializedChunk::default();
        let mut items = Vec::new();
        for (index, piece) in node.pieces.iter().enumerate() {
            let product = self.walk_piece(node, index, piece, StreamMode::Blocks)?;
            match product {
                NodeProduct::Blocks(value) if piece_role(piece).is_none() => {
                    leading.append(value)?
                }
                NodeProduct::ManItem(value) => items.push(value),
                _ => {
                    return Err(MaterializationError::ProductTypeMismatch {
                        node: node.key,
                        expected: "man definition run item",
                    });
                }
            }
        }
        Ok(NodeProduct::Blocks(
            self.emitter
                .man_run(node, leading, items)
                .map_err(MaterializationError::backend)?,
        ))
    }

    fn walk_specialized_root(
        &mut self,
        node: &ProjectionNode,
    ) -> Result<NodeProduct, MaterializationError> {
        let expected = match node.kind {
            ProjectionNodeKind::Table(key) => SpecializedRecipe::Table(key),
            ProjectionNodeKind::Equation(key) => SpecializedRecipe::Equation(key),
            _ => unreachable!(),
        };
        let mut recipe = None;
        for piece in &node.pieces {
            match piece {
                ProjectionPiece::Specialized { recipe: value, .. }
                    if *value == expected && recipe.is_none() =>
                {
                    recipe = Some(*value);
                }
                ProjectionPiece::Child { node: child, .. } => {
                    self.absorb_node(*child)?;
                }
                ProjectionPiece::Stream { .. } => {}
                ProjectionPiece::Specialized { recipe, .. } => {
                    return Err(MaterializationError::InvalidSpecializedRecipe {
                        node: node.key,
                        recipe: *recipe,
                    });
                }
            }
        }
        let recipe = recipe.ok_or(MaterializationError::MissingSpecializedRoot(node.key))?;
        let emission = self
            .emitter
            .specialized_root(node, recipe)
            .map_err(MaterializationError::backend)?;
        Ok(NodeProduct::Blocks(
            self.consume_specialized(node.key, emission)?,
        ))
    }

    fn absorb_children(&mut self, node: &ProjectionNode) -> Result<(), MaterializationError> {
        for piece in &node.pieces {
            if let ProjectionPiece::Child { node, .. } = piece {
                self.absorb_node(*node)?;
            }
        }
        Ok(())
    }

    fn absorb_node(&mut self, key: ProjectionNodeKey) -> Result<(), MaterializationError> {
        self.enter(key)?;
        let node = self.node(key)?.clone();
        self.absorb_children(&node)
    }
}

fn validate_child_role(
    parent: ProjectionNodeKey,
    child: ProjectionNodeKey,
    role: ChildRole,
    product: &NodeProduct,
) -> Result<(), MaterializationError> {
    let valid = match role {
        ChildRole::Structural => matches!(product, NodeProduct::Blocks(_)),
        ChildRole::MdocItem => matches!(product, NodeProduct::MdocItem(_)),
        ChildRole::MdocHead | ChildRole::ManHead => {
            matches!(product, NodeProduct::Inline(_))
        }
        ChildRole::ManContinuation => matches!(product, NodeProduct::ManContinuation(_)),
        ChildRole::MdocBody { .. } | ChildRole::ManBody | ChildRole::ManBodyContinuation => {
            matches!(product, NodeProduct::Blocks(_))
        }
        ChildRole::ManDefinitionItem => matches!(product, NodeProduct::ManItem(_)),
    };
    if valid {
        Ok(())
    } else {
        Err(MaterializationError::ChildRoleMismatch {
            parent,
            child,
            role,
        })
    }
}

fn expect_blocks(
    product: NodeProduct,
    node: ProjectionNodeKey,
) -> Result<MaterializedChunk, MaterializationError> {
    match product {
        NodeProduct::Blocks(value) => Ok(value),
        _ => Err(MaterializationError::ProductTypeMismatch {
            node,
            expected: "block sequence",
        }),
    }
}

const fn specialized_recipe_for_node(kind: ProjectionNodeKind) -> Option<SpecializedRecipe> {
    match kind {
        ProjectionNodeKind::Table(key) => Some(SpecializedRecipe::Table(key)),
        ProjectionNodeKind::TableRow(key) => Some(SpecializedRecipe::TableRow(key)),
        ProjectionNodeKind::TableCellInvocation(key) => {
            Some(SpecializedRecipe::TableCellInvocation(key))
        }
        ProjectionNodeKind::Equation(key) => Some(SpecializedRecipe::Equation(key)),
        ProjectionNodeKind::EquationInvocation(key) => {
            Some(SpecializedRecipe::EquationInvocation(key))
        }
        _ => None,
    }
}

const fn specialized_recipe_sort_key(recipe: &SpecializedRecipe) -> (u8, u32) {
    match recipe {
        SpecializedRecipe::Table(key) => (0, key.0),
        SpecializedRecipe::TableRow(key) => (1, key.0),
        SpecializedRecipe::TableCellInvocation(key) => (2, key.0),
        SpecializedRecipe::Equation(key) => (3, key.0),
        SpecializedRecipe::EquationInvocation(key) => (4, key.0),
    }
}

const fn piece_destination(piece: &ProjectionPiece) -> ProjectionDestination {
    match piece {
        ProjectionPiece::Stream { destination, .. }
        | ProjectionPiece::Child { destination, .. }
        | ProjectionPiece::Specialized { destination, .. } => *destination,
    }
}

const fn piece_role(piece: &ProjectionPiece) -> Option<ChildRole> {
    match piece {
        ProjectionPiece::Child { role, .. } => Some(*role),
        ProjectionPiece::Stream { .. } | ProjectionPiece::Specialized { .. } => None,
    }
}

#[allow(dead_code)]
fn _inline_projector_is_a_consumer(_: &InlineProjector<'_>) {}
