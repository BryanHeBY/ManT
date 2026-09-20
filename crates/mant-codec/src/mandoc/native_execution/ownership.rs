//! Exclusive structural ownership for native execution projection.
//!
//! Fixed CVS executes both man(7) and mdoc(7) by nesting `pre`, child, and
//! `post` calls in `print_man_node()` and `print_mdoc_node()`.  The terminal
//! observer preserves that dynamic nesting in `ExecutionWrapper::parent`.
//! This module turns those facts into a forest before any IR is materialized;
//! parent atom envelopes never imply ownership of their descendants.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use libmandoc_rs::{
    AtomDisposition, BoundaryEffect, BoundaryRequest, ExecutionEquationInvocationKey,
    ExecutionEquationKey, ExecutionHeadingKind, ExecutionManBlockKind, ExecutionMdocListKind,
    ExecutionNodeKey, ExecutionOutputRole, ExecutionRegionKind, ExecutionTableCellInvocationKey,
    ExecutionTableKey, ExecutionTableRowKey, ExecutionWrapperKind, FragmentRole,
    GeometryOriginKind, NativeExecutionReport, NodeKind,
};

use super::NativeProjection;

/// Stable, arena-local identity of one structural projection owner.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct ProjectionNodeKey(pub(super) u32);

/// Stable, arena-local identity of one canonical formatter effect.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct EffectKey(pub(super) u32);

/// Native causal root from which a canonical formatter effect is derived.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum EffectCauseKey {
    BoundaryRoot(u32),
    StandaloneFlush(u32),
    StandaloneGeometry(u32),
}

/// Reference to one raw execution record contributing to a canonical effect.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum RawRecordRef {
    Atom(u32),
    Fragment(u32),
    Flush(u32),
    Boundary(u32),
    Control(u32),
    Geometry(u32),
    Placement(u32),
    Anchor(u32),
}

impl RawRecordRef {
    fn parts(self) -> (RecordKind, u32) {
        match self {
            Self::Atom(key) => (RecordKind::Atom, key),
            Self::Fragment(key) => (RecordKind::Fragment, key),
            Self::Flush(key) => (RecordKind::Flush, key),
            Self::Boundary(key) => (RecordKind::Boundary, key),
            Self::Control(key) => (RecordKind::Control, key),
            Self::Geometry(key) => (RecordKind::Geometry, key),
            Self::Placement(key) => (RecordKind::Placement, key),
            Self::Anchor(key) => (RecordKind::Anchor, key),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CanonicalEffectPlan {
    pub(super) key: EffectKey,
    pub(super) cause: EffectCauseKey,
    pub(super) records: Vec<RawRecordRef>,
    pub(super) sequence: u64,
    pub(super) output_role: ExecutionOutputRole,
    pub(super) outcome: CanonicalEffectOutcome,
}

/// Source-neutral result of one complete native causal closure.
///
/// The materializer consumes this result directly.  It must never rescan the
/// raw records and independently reinterpret the same formatter action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CanonicalEffectOutcome {
    StateOnly,
    /// Device spacing emitted by a structural presenter whose source-neutral
    /// equivalent is carried by the enclosing IR node (for example,
    /// `Block::DefinitionList::compact`).
    StructuralSpacing,
    /// Ordered device effects of one native causal closure.  This is kept as
    /// an execution trace because one request can first settle the current
    /// formatter line and then add one or more blank device rows.
    Trace(Vec<CanonicalEffectStep>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CanonicalEffectStep {
    pub(super) sequence: u64,
    pub(super) kind: CanonicalEffectKind,
    /// The event executed inside an inline block/enclosure rather than at a
    /// document block boundary.  The same native line commit is therefore an
    /// inline hard break instead of a standalone IR spacing block.
    pub(super) inline_flow: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CanonicalEffectKind {
    /// A logical formatter line was committed. `empty` distinguishes a real
    /// empty-row commitment (for example a pending `\p`) from merely ending
    /// a visible row before a structural child.
    LogicalLineCommit { empty: bool },
    /// Native field completion before structural ownership is known.  The
    /// ownership pass resolves this either to `FieldSettlement` for a typed
    /// definition field or to a real logical line commit everywhere else.
    FieldEnd { empty: bool },
    /// Completion of one native formatter field.  In fixed CVS this is the
    /// `term_flushln()` settlement between a definition HEAD and BODY (or a
    /// comparable field transition), not an additional logical content row.
    /// The owning structural recipe already represents that separation.
    FieldSettlement,
    /// One unconditional blank device row.
    BlankDeviceLine,
    /// A fixed-device wrap. Responsive IR deliberately does not project it.
    SoftDeviceWrap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EffectLedger {
    effects: Vec<CanonicalEffectPlan>,
    /// Geometry caused directly by content atoms/fragments belongs to Flow.
    flow_geometry: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BoundaryFact {
    key: u32,
    parent: Option<u32>,
    sequence: u64,
    request: BoundaryRequest,
    effect: BoundaryEffect,
    direct_device_lines: u32,
    line_commit_cause: Option<libmandoc_rs::LineCommitCause>,
    visual_before: i64,
    inline_flow: bool,
    output_role: ExecutionOutputRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FlushFact {
    key: u32,
    boundary: Option<u32>,
    sequence: u64,
    outcome: libmandoc_rs::FlushOutcome,
    logical_forced_break: bool,
    output_role: ExecutionOutputRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GeometryCause {
    None,
    Atom,
    Fragment,
    Flush(u32),
    Boundary(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GeometryFact {
    key: u32,
    cause: GeometryCause,
    sequence: u64,
    output_role: ExecutionOutputRole,
}

/// Exact execution interval of one owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ExecutionSpan {
    pub(super) atoms: Range<u32>,
    pub(super) enter_sequence: u64,
    pub(super) leave_sequence: u64,
}

impl ExecutionSpan {
    fn is_well_formed(&self, atom_count: usize) -> bool {
        self.atoms.start <= self.atoms.end
            && usize::try_from(self.atoms.end).is_ok_and(|end| end <= atom_count)
            && self.enter_sequence <= self.leave_sequence
    }

    fn contains(&self, child: &Self) -> bool {
        self.atoms.start <= child.atoms.start
            && child.atoms.end <= self.atoms.end
            && self.enter_sequence <= child.enter_sequence
            && child.leave_sequence <= self.leave_sequence
    }
}

/// Native scope kind retained without turning presentation scopes into text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProjectionScopeKind {
    MdocListItem(ExecutionMdocListKind),
    ManBlock(ExecutionManBlockKind),
    Region(ExecutionRegionKind),
}

/// Exact formatter segment within a native definition/list owner.
///
/// These are structural execution owners, not inferred IR roles.  Fixed CVS
/// executes each HEAD/BODY syntax node inside the surrounding `.It`, `.IP`,
/// `.TP`, or `.TQ` lifecycle; preserving the segment node prevents the
/// materializer from rediscovering term/body ownership from atom ranges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DefinitionSegmentKind {
    MdocHead,
    MdocBody { ordinal: u32 },
    ManHead,
    ManBody,
}

/// Structural owner kind.  Semantic facts and inline annotations are not
/// owners; they attach to one of these nodes after the forest is sealed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProjectionNodeKind {
    Root,
    #[cfg(test)]
    Flow,
    Heading {
        wrapper: u32,
        kind: ExecutionHeadingKind,
    },
    NativeScope {
        wrapper: u32,
        kind: ProjectionScopeKind,
    },
    /// One parser-owned mdoc list item.  The list/item/HEAD/BODY topology is
    /// retained before materialization; consumers never rediscover it from
    /// indentation or rendered block adjacency.
    MdocItem {
        owner: ExecutionNodeKey,
        flow_epoch: usize,
        kind: ExecutionMdocListKind,
        compact: bool,
    },
    DefinitionSegment {
        kind: DefinitionSegmentKind,
    },
    MdocList {
        owner: ExecutionNodeKey,
        kind: ExecutionMdocListKind,
    },
    /// A maximal source-order run of man definition items.  Paragraph and
    /// relative-indent wrappers terminate a run rather than being flattened
    /// into it.
    ManDefinitionRun {
        flow_epoch: usize,
    },
    /// One source-neutral man definition item.  A TP/IP starts the item and
    /// any immediately continued TQ owners are retained as typed children.
    ManDefinitionItem {
        primary: ExecutionNodeKey,
        kind: ExecutionManBlockKind,
    },
    /// A formatter-proven hanging pair: fixed CVS flushed `term` while
    /// entering the `.RS` scope that owns `scope`.  This is native execution
    /// topology, not an adjacency or indentation inference over final IR.
    ManHangingPair {
        term: ExecutionNodeKey,
        scope: ExecutionNodeKey,
        entry_boundary: u32,
        term_origin_columns: i32,
    },
    /// One `.TQ` lifecycle attached to the preceding TP item.
    ManDefinitionContinuation {
        owner: ExecutionNodeKey,
    },
    Table(ExecutionTableKey),
    TableRow(ExecutionTableRowKey),
    TableCellInvocation(ExecutionTableCellInvocationKey),
    Equation(ExecutionEquationKey),
    EquationInvocation(ExecutionEquationInvocationKey),
}

/// Typed materialization recipe for native subsystems whose structure is not
/// representable as a flat sequence of ordinary formatter flows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SpecializedRecipe {
    Table(ExecutionTableKey),
    TableRow(ExecutionTableRowKey),
    TableCellInvocation(ExecutionTableCellInvocationKey),
    Equation(ExecutionEquationKey),
    EquationInvocation(ExecutionEquationInvocationKey),
}

/// Output placement is deliberately independent from dynamic execution and
/// structural content nesting.  Section destinations are assigned later from
/// a deterministic location map and never participate in record ownership.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum ProjectionDestination {
    Root,
    Section(SectionKey),
    Deferred,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct SectionKey(pub(super) u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SectionLocation {
    pub(super) key: SectionKey,
    pub(super) heading_wrapper: u32,
    pub(super) enter_sequence: u64,
    pub(super) leave_sequence: u64,
}

/// Deterministic projection-location map built by the section planner.
/// Source nodes may be used while constructing this index, but never while
/// choosing execution parents or direct raw-record owners.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SectionIndex {
    headings: BTreeMap<u32, SectionKey>,
    transitions: Vec<(u64, ProjectionDestination)>,
}

impl SectionIndex {
    pub(super) fn from_locations(
        mut locations: Vec<SectionLocation>,
    ) -> Result<Self, ProjectionPlanError> {
        locations.sort_by_key(|location| {
            (
                location.enter_sequence,
                std::cmp::Reverse(location.leave_sequence),
                location.key,
            )
        });
        let mut wrappers = BTreeSet::new();
        let mut keys = BTreeSet::new();
        let mut headings = BTreeMap::new();
        for location in &locations {
            if location.enter_sequence >= location.leave_sequence {
                return Err(ProjectionPlanError::InvalidSectionInterval {
                    key: location.key,
                    enter_sequence: location.enter_sequence,
                    leave_sequence: location.leave_sequence,
                });
            }
            if !wrappers.insert(location.heading_wrapper) {
                return Err(ProjectionPlanError::DuplicateHeadingDestination {
                    wrapper: location.heading_wrapper,
                });
            }
            headings.insert(location.heading_wrapper, location.key);
            if !keys.insert(location.key) {
                return Err(ProjectionPlanError::DuplicateSectionKey(location.key));
            }
        }
        let mut stack = Vec::<SectionLocation>::new();
        for location in &locations {
            while stack
                .last()
                .is_some_and(|parent| parent.leave_sequence <= location.enter_sequence)
            {
                stack.pop();
            }
            if let Some(parent) = stack.last()
                && (location.leave_sequence > parent.leave_sequence
                    || (location.enter_sequence == parent.enter_sequence
                        && location.leave_sequence == parent.leave_sequence))
            {
                return Err(ProjectionPlanError::AmbiguousSectionIntervals {
                    first: parent.key,
                    second: location.key,
                });
            }
            stack.push(*location);
        }
        let mut events = BTreeMap::<u64, (Vec<SectionKey>, Vec<SectionLocation>)>::new();
        for location in &locations {
            events
                .entry(location.leave_sequence)
                .or_default()
                .0
                .push(location.key);
            events
                .entry(location.enter_sequence)
                .or_default()
                .1
                .push(*location);
        }
        let by_key = locations
            .iter()
            .map(|location| (location.key, *location))
            .collect::<BTreeMap<_, _>>();
        let mut active = BTreeSet::<(u64, SectionKey)>::new();
        let mut transitions = Vec::new();
        let mut previous = ProjectionDestination::Root;
        for (sequence, (leaves, enters)) in events {
            for key in leaves {
                let location = by_key[&key];
                active.remove(&(
                    location
                        .leave_sequence
                        .saturating_sub(location.enter_sequence),
                    key,
                ));
            }
            for location in enters {
                active.insert((
                    location
                        .leave_sequence
                        .saturating_sub(location.enter_sequence),
                    location.key,
                ));
            }
            let destination = active
                .first()
                .map_or(ProjectionDestination::Root, |(_, key)| {
                    ProjectionDestination::Section(*key)
                });
            if destination != previous {
                transitions.push((sequence, destination));
                previous = destination;
            }
        }
        Ok(Self {
            headings,
            transitions,
        })
    }

    fn for_heading(&self, wrapper: u32) -> Option<SectionKey> {
        self.headings.get(&wrapper).copied()
    }

    fn destination_for_sequence(&self, sequence: u64) -> ProjectionDestination {
        let count = self
            .transitions
            .partition_point(|(start, _)| *start <= sequence);
        count
            .checked_sub(1)
            .map_or(ProjectionDestination::Root, |index| {
                self.transitions[index].1
            })
    }

    fn cuts_inside(&self, enter: u64, leave: u64) -> impl Iterator<Item = u64> + '_ {
        let start = self
            .transitions
            .partition_point(|(sequence, _)| *sequence <= enter);
        let end = self
            .transitions
            .partition_point(|(sequence, _)| *sequence < leave);
        self.transitions[start..end]
            .iter()
            .map(|(sequence, _)| *sequence)
    }
}

/// Total ordering of content and zero-width events at one owner.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ProjectionOrder {
    pub(super) sequence: u64,
    pub(super) phase: ProjectionPhase,
    pub(super) stable: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ProjectionPhase {
    Before,
    Content,
}

/// Exact role of one structural child in its parent's typed recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ChildRole {
    Structural,
    MdocItem,
    MdocHead,
    MdocBody {
        ordinal: u32,
    },
    ManDefinitionItem,
    ManHead,
    ManBody,
    /// A native relative-indent scope continuing the current definition
    /// body's formatter context without becoming an additional term.
    ManBodyContinuation,
    ManContinuation,
}

/// One event in a formatter stream.  Events remain ordered and are settled
/// together by the materializer, so an anchor or boundary cannot be turned
/// into a standalone block merely because it has its own observer record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum StreamEvent {
    Flow {
        atoms: Range<u32>,
        fragments: Vec<u32>,
        geometry: Vec<u32>,
    },
    CanonicalEffect {
        key: EffectKey,
        records: Vec<RawRecordRef>,
        outcome: CanonicalEffectOutcome,
    },
    Anchor {
        key: u32,
    },
    Placement {
        key: u32,
    },
    /// One executed roff control request.  Its output-bearing consequences
    /// remain in the canonical boundary/effect closure; this event owns the
    /// request's typed state transition exactly once in total execution order.
    Control {
        key: u32,
    },
}

impl StreamEvent {
    pub(super) fn records(&self) -> Vec<RawRecordRef> {
        match self {
            Self::Flow {
                atoms,
                fragments,
                geometry,
            } => atoms
                .clone()
                .map(RawRecordRef::Atom)
                .chain(fragments.iter().copied().map(RawRecordRef::Fragment))
                .chain(geometry.iter().copied().map(RawRecordRef::Geometry))
                .collect(),
            Self::CanonicalEffect { records, .. } => records.clone(),
            Self::Anchor { key } => vec![RawRecordRef::Anchor(*key)],
            Self::Placement { key } => vec![RawRecordRef::Placement(*key)],
            Self::Control { key } => vec![RawRecordRef::Control(*key)],
        }
    }
}

/// One ordered recipe belonging directly to a structural owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ProjectionPiece {
    Stream {
        events: Vec<StreamEvent>,
        destination: ProjectionDestination,
        order: ProjectionOrder,
    },
    Child {
        node: ProjectionNodeKey,
        role: ChildRole,
        destination: ProjectionDestination,
        order: ProjectionOrder,
    },
    Specialized {
        recipe: SpecializedRecipe,
        records: Vec<RawRecordRef>,
        destination: ProjectionDestination,
        order: ProjectionOrder,
    },
}

impl ProjectionPiece {
    fn order(&self) -> ProjectionOrder {
        match self {
            Self::Stream { order, .. }
            | Self::Child { order, .. }
            | Self::Specialized { order, .. } => *order,
        }
    }

    fn destination(&self) -> ProjectionDestination {
        match self {
            Self::Stream { destination, .. }
            | Self::Child { destination, .. }
            | Self::Specialized { destination, .. } => *destination,
        }
    }

    fn set_destination(&mut self, destination: ProjectionDestination) {
        match self {
            Self::Stream {
                destination: current,
                ..
            }
            | Self::Child {
                destination: current,
                ..
            }
            | Self::Specialized {
                destination: current,
                ..
            } => *current = destination,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProjectionNode {
    pub(super) key: ProjectionNodeKey,
    pub(super) execution_parent: Option<ProjectionNodeKey>,
    pub(super) content_parent: Option<ProjectionNodeKey>,
    pub(super) destination: ProjectionDestination,
    pub(super) source_owner: Option<ExecutionNodeKey>,
    pub(super) span: ExecutionSpan,
    pub(super) kind: ProjectionNodeKind,
    pub(super) pieces: Vec<ProjectionPiece>,
}

/// An execution record is either owned exactly once or explicitly omitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RecordDisposition {
    Unassigned,
    Owned(ProjectionNodeKey),
    Omitted(OmissionReason),
}

/// Closed reasons for records that intentionally do not reach source-neutral
/// content.  Adding a new reason requires an explicit planner decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OmissionReason {
    ReplacedAtom,
    ConsumedAtom,
    TrailingDiscard,
    DeviceDecoration,
    MarginDecoration,
    #[cfg(test)]
    SoftDeviceWrap,
    #[cfg(test)]
    StateOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ExecutionOwnership {
    pub(super) atoms: Vec<RecordDisposition>,
    pub(super) fragments: Vec<RecordDisposition>,
    pub(super) flushes: Vec<RecordDisposition>,
    pub(super) boundaries: Vec<RecordDisposition>,
    pub(super) controls: Vec<RecordDisposition>,
    pub(super) geometry: Vec<RecordDisposition>,
    pub(super) placements: Vec<RecordDisposition>,
    pub(super) anchors: Vec<RecordDisposition>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ExecutionRecordCounts {
    pub(super) atoms: usize,
    pub(super) fragments: usize,
    pub(super) flushes: usize,
    pub(super) boundaries: usize,
    pub(super) controls: usize,
    pub(super) geometry: usize,
    pub(super) placements: usize,
    pub(super) anchors: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ProjectionArena {
    pub(super) nodes: Vec<ProjectionNode>,
    pub(super) roots: Vec<ProjectionNodeKey>,
    pub(super) ownership: ExecutionOwnership,
    pub(super) specialized_absorptions: Vec<SpecializedAbsorption>,
    pub(super) heading_absorptions: Vec<HeadingAbsorption>,
    pub(super) final_consumers: Vec<FinalConsumerReceipt>,
}

impl ProjectionArena {
    pub(super) fn node(&self, key: ProjectionNodeKey) -> &ProjectionNode {
        &self.nodes[key.0 as usize]
    }

    #[cfg(test)]
    pub(super) fn final_consumer(&self, consumer: FinalConsumer) -> Option<&FinalConsumerReceipt> {
        self.final_consumers
            .binary_search_by_key(&final_consumer_sort_key(consumer), |receipt| {
                final_consumer_sort_key(receipt.consumer)
            })
            .ok()
            .map(|index| &self.final_consumers[index])
    }
}

/// Explicit receipt proving that one typed table/equation recipe absorbs the
/// records of its complete renderer invocation subtree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SpecializedAbsorption {
    pub(super) root: ProjectionNodeKey,
    pub(super) descendants: Vec<ProjectionNodeKey>,
    pub(super) records: Vec<RawRecordRef>,
}

/// Explicit receipt proving that the dedicated heading recipe consumes the
/// complete execution subtree used to render one heading.  Heading wrappers
/// are execution-only parents, so their records cannot be replayed by the
/// ordinary content forest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HeadingAbsorption {
    pub(super) root: ProjectionNodeKey,
    pub(super) descendants: Vec<ProjectionNodeKey>,
    pub(super) records: Vec<RawRecordRef>,
}

/// The unique product-side consumer of one closed set of owned records.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum FinalConsumer {
    Stream {
        owner: ProjectionNodeKey,
        piece: u32,
    },
    Specialized(ProjectionNodeKey),
    Heading(ProjectionNodeKey),
}

/// Proof that every `Owned` raw record reaches one and only one materializer
/// consumer.  Omitted records remain proven by their closed ledger reason and
/// never appear in these receipts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FinalConsumerReceipt {
    pub(super) consumer: FinalConsumer,
    pub(super) records: Vec<RawRecordRef>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum RecordKind {
    Atom,
    Fragment,
    Flush,
    Boundary,
    Control,
    Geometry,
    Placement,
    Anchor,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ProjectionPlanError {
    InvalidSpan {
        atoms: Range<u32>,
        enter_sequence: u64,
        leave_sequence: u64,
        atom_count: usize,
    },
    NodeCapacityExceeded,
    UnknownNode(ProjectionNodeKey),
    NativeOwnerConflict(ExecutionNodeKey),
    ProjectionCardinalityMismatch {
        kind: &'static str,
        report: usize,
        projection: usize,
    },
    MissingNativeParent {
        kind: &'static str,
        key: u32,
    },
    InvalidCausalReference {
        kind: RecordKind,
        key: u32,
        parent: u32,
    },
    CausalCycle {
        kind: RecordKind,
        key: u32,
    },
    MixedEffectOutputRoles(EffectCauseKey),
    AmbiguousParent {
        child: ProjectionNodeKey,
        first: ProjectionNodeKey,
        second: ProjectionNodeKey,
    },
    OrphanedNode(ProjectionNodeKey),
    UnresolvedDestination(ProjectionNodeKey),
    UnresolvedPieceDestination {
        owner: ProjectionNodeKey,
        sequence: u64,
    },
    MissingHeadingDestination {
        wrapper: u32,
    },
    DuplicateHeadingDestination {
        wrapper: u32,
    },
    DuplicateSectionKey(SectionKey),
    AmbiguousSectionIntervals {
        first: SectionKey,
        second: SectionKey,
    },
    InvalidSectionInterval {
        key: SectionKey,
        enter_sequence: u64,
        leave_sequence: u64,
    },
    StructuralOwnerCrossesDestination {
        node: ProjectionNodeKey,
        kind: ProjectionNodeKind,
        source_owner: Option<ExecutionNodeKey>,
        span: ExecutionSpan,
    },
    ParentAlreadyAssigned(ProjectionNodeKey),
    ParentCycle {
        parent: ProjectionNodeKey,
        child: ProjectionNodeKey,
    },
    ParentDoesNotContainChild {
        parent: ProjectionNodeKey,
        child: ProjectionNodeKey,
    },
    OverlappingSiblings {
        left: ProjectionNodeKey,
        left_kind: ProjectionNodeKind,
        left_source_owner: Option<ExecutionNodeKey>,
        left_span: ExecutionSpan,
        right: ProjectionNodeKey,
        right_kind: ProjectionNodeKind,
        right_source_owner: Option<ExecutionNodeKey>,
        right_span: ExecutionSpan,
    },
    RecordOutOfBounds {
        kind: RecordKind,
        index: u32,
    },
    RecordAlreadyAssigned {
        kind: RecordKind,
        index: u32,
        disposition: RecordDisposition,
    },
    NoOwnerForRecord {
        kind: RecordKind,
        index: u32,
        sequence: u64,
    },
    AmbiguousRecordOwner {
        kind: RecordKind,
        index: u32,
        first: ProjectionNodeKey,
        first_kind: ProjectionNodeKind,
        second: ProjectionNodeKey,
        second_kind: ProjectionNodeKind,
    },
    InvalidOmission {
        kind: RecordKind,
        reason: OmissionReason,
    },
    UnassignedRecord {
        kind: RecordKind,
        index: u32,
    },
    RecordOutsideOwner {
        kind: RecordKind,
        index: u32,
        owner: ProjectionNodeKey,
    },
    PieceOutsideOwner {
        owner: ProjectionNodeKey,
        sequence: u64,
    },
    OwnedRecordUnexplained {
        kind: RecordKind,
        index: u32,
        owner: ProjectionNodeKey,
    },
    RecordExplainedTwice {
        record: RawRecordRef,
        first: ProjectionNodeKey,
        second: ProjectionNodeKey,
    },
    RecordExplanationOwnerMismatch {
        kind: RecordKind,
        index: u32,
        ledger_owner: ProjectionNodeKey,
        plan_owner: ProjectionNodeKey,
    },
    UnownedRecordExplained {
        kind: RecordKind,
        index: u32,
        plan_owner: ProjectionNodeKey,
    },
    OmittedRecordExplained {
        kind: RecordKind,
        index: u32,
        owner: ProjectionNodeKey,
        reason: OmissionReason,
    },
    ChildPieceMismatch(ProjectionNodeKey),
    InvalidRootCount(usize),
    InvalidSpecializedRecipe(ProjectionNodeKey),
    InvalidHeadingReceipt(ProjectionNodeKey),
    AbsorptionRecordMismatch(ProjectionNodeKey),
    RecordAbsorbedTwice(RawRecordRef),
    BufferedAtomAtSeal(u32),
}

impl std::fmt::Display for ProjectionPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProjectionPlanError {}

/// Whether a formatter effect belongs to an inline block/enclosure.
///
/// The fixed CVS executor wraps every syntax node.  `mdoc_term.c` assigns a
/// small, closed family of full-block macros to inline formatter handlers:
/// quote/enclosure scopes, `Eo`/`Xo`, font/keep scopes, and function scopes.
/// They retain one formatter flow even when a control request commits a line;
/// true document/list/display blocks do not.
fn is_mdoc_inline_block_macro(name: &str) -> bool {
    matches!(
        name,
        "Ao" | "Bo" | "Bro" | "Do" | "Eo" | "Oo" | "Po" | "Qo" | "So" | "Xo" | "Bf" | "Bk" | "Fo"
    )
}

fn wrapper_has_inline_block(report: &NativeExecutionReport, mut wrapper: Option<u32>) -> bool {
    while let Some(key) = wrapper {
        let current = &report.wrappers()[key as usize];
        match current.kind {
            ExecutionWrapperKind::Node => {
                if current.node.is_some_and(|node| {
                    let node = &report.nodes()[node.0 as usize];
                    node.kind == NodeKind::Block
                        && node
                            .macro_name
                            .as_deref()
                            .is_some_and(is_mdoc_inline_block_macro)
                }) {
                    return true;
                }
            }
            ExecutionWrapperKind::Heading
            | ExecutionWrapperKind::MdocListItem
            | ExecutionWrapperKind::ManBlock
            | ExecutionWrapperKind::Region => return false,
            ExecutionWrapperKind::Font => {}
        }
        wrapper = current.parent;
    }
    false
}

impl EffectLedger {
    fn from_native(report: &NativeExecutionReport) -> Result<Self, ProjectionPlanError> {
        let boundaries = report
            .boundaries()
            .iter()
            .map(|record| BoundaryFact {
                key: record.key,
                parent: record.parent,
                sequence: record.enter_sequence,
                request: record.request,
                effect: record.effect,
                direct_device_lines: record.direct_device_lines,
                line_commit_cause: record.line_commit_cause,
                visual_before: record.visual_before,
                // Only native control requests execute *inside* an inline
                // enclosure.  Structural presenters such as mdoc `Pp` can
                // be nested below `Bk`/`Eo` in the syntax tree, but their
                // `term_vspace()` remains a block-level paragraph boundary.
                // The native control identity is the positive execution
                // evidence; ancestry alone is not.
                inline_flow: record.control.is_some()
                    && wrapper_has_inline_block(report, record.wrapper),
                output_role: record.output_role,
            })
            .collect::<Vec<_>>();
        let flushes = report
            .flushes()
            .iter()
            .map(|record| FlushFact {
                key: record.key,
                boundary: record.boundary,
                sequence: record.outcome_sequence,
                outcome: record.outcome,
                logical_forced_break: record.logical_forced_break,
                output_role: record.output_role,
            })
            .collect::<Vec<_>>();
        let geometry = report
            .geometry()
            .iter()
            .map(|record| {
                let cause = match record.origin_kind {
                    GeometryOriginKind::None => GeometryCause::None,
                    GeometryOriginKind::Atom => GeometryCause::Atom,
                    GeometryOriginKind::Fragment => GeometryCause::Fragment,
                    GeometryOriginKind::Flush => GeometryCause::Flush(record.origin_key.ok_or(
                        ProjectionPlanError::InvalidCausalReference {
                            kind: RecordKind::Geometry,
                            key: record.key,
                            parent: u32::MAX,
                        },
                    )?),
                    GeometryOriginKind::Boundary => {
                        GeometryCause::Boundary(record.origin_key.ok_or(
                            ProjectionPlanError::InvalidCausalReference {
                                kind: RecordKind::Geometry,
                                key: record.key,
                                parent: u32::MAX,
                            },
                        )?)
                    }
                };
                Ok(GeometryFact {
                    key: record.key,
                    cause,
                    sequence: record.sequence,
                    output_role: record.output_role,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_facts(&boundaries, &flushes, &geometry)
    }

    fn from_facts(
        boundaries: &[BoundaryFact],
        flushes: &[FlushFact],
        geometry: &[GeometryFact],
    ) -> Result<Self, ProjectionPlanError> {
        let boundary_by_key = boundaries
            .iter()
            .map(|record| (record.key, *record))
            .collect::<BTreeMap<_, _>>();
        let flush_by_key = flushes
            .iter()
            .map(|record| (record.key, *record))
            .collect::<BTreeMap<_, _>>();
        let geometry_by_key = geometry
            .iter()
            .map(|record| (record.key, *record))
            .collect::<BTreeMap<_, _>>();
        if boundary_by_key.len() != boundaries.len() {
            return Err(ProjectionPlanError::CausalCycle {
                kind: RecordKind::Boundary,
                key: u32::MAX,
            });
        }
        if flush_by_key.len() != flushes.len() {
            return Err(ProjectionPlanError::CausalCycle {
                kind: RecordKind::Flush,
                key: u32::MAX,
            });
        }
        if geometry_by_key.len() != geometry.len() {
            return Err(ProjectionPlanError::CausalCycle {
                kind: RecordKind::Geometry,
                key: u32::MAX,
            });
        }

        let boundary_root = |key| boundary_root(key, &boundary_by_key);
        let flush_cause = |key: u32| -> Result<EffectCauseKey, ProjectionPlanError> {
            let record =
                flush_by_key
                    .get(&key)
                    .ok_or(ProjectionPlanError::InvalidCausalReference {
                        kind: RecordKind::Flush,
                        key,
                        parent: key,
                    })?;
            record
                .boundary
                .map_or(Ok(EffectCauseKey::StandaloneFlush(key)), |boundary| {
                    boundary_root(boundary).map(EffectCauseKey::BoundaryRoot)
                })
        };

        let mut groups = BTreeMap::<EffectCauseKey, (u64, Vec<RawRecordRef>)>::new();
        for record in boundaries {
            let cause = EffectCauseKey::BoundaryRoot(boundary_root(record.key)?);
            push_effect_record(
                &mut groups,
                cause,
                record.sequence,
                RawRecordRef::Boundary(record.key),
            );
        }
        for record in flushes {
            let cause = flush_cause(record.key)?;
            push_effect_record(
                &mut groups,
                cause,
                record.sequence,
                RawRecordRef::Flush(record.key),
            );
        }

        let mut flow_geometry = Vec::new();
        for record in geometry {
            let cause = match record.cause {
                GeometryCause::Atom | GeometryCause::Fragment => {
                    flow_geometry.push(record.key);
                    continue;
                }
                GeometryCause::Boundary(key) => EffectCauseKey::BoundaryRoot(boundary_root(key)?),
                GeometryCause::Flush(key) => flush_cause(key)?,
                GeometryCause::None => EffectCauseKey::StandaloneGeometry(record.key),
            };
            push_effect_record(
                &mut groups,
                cause,
                record.sequence,
                RawRecordRef::Geometry(record.key),
            );
        }

        let effects = groups
            .into_iter()
            .enumerate()
            .map(|(index, (cause, (sequence, mut records)))| {
                records.sort_unstable();
                let mut roles = records.iter().map(|record| match record {
                    RawRecordRef::Flush(key) => flush_by_key[key].output_role,
                    RawRecordRef::Boundary(key) => boundary_by_key[key].output_role,
                    RawRecordRef::Geometry(key) => geometry_by_key[key].output_role,
                    RawRecordRef::Atom(_)
                    | RawRecordRef::Fragment(_)
                    | RawRecordRef::Control(_)
                    | RawRecordRef::Placement(_)
                    | RawRecordRef::Anchor(_) => unreachable!(
                        "canonical effect groups contain only flush, boundary, and geometry records"
                    ),
                });
                let output_role =
                    roles
                        .next()
                        .ok_or(ProjectionPlanError::InvalidCausalReference {
                            kind: RecordKind::Boundary,
                            key: u32::MAX,
                            parent: u32::MAX,
                        })?;
                if roles.any(|role| role != output_role) {
                    return Err(ProjectionPlanError::MixedEffectOutputRoles(cause));
                }
                let outcome = match cause {
                    EffectCauseKey::BoundaryRoot(key) => {
                        let _root = boundary_by_key.get(&key).ok_or(
                            ProjectionPlanError::InvalidCausalReference {
                                kind: RecordKind::Boundary,
                                key,
                                parent: key,
                            },
                        )?;
                        let mut steps = records
                            .iter()
                            .filter_map(|record| {
                                let RawRecordRef::Boundary(boundary) = record else {
                                    return None;
                                };
                                let boundary = boundary_by_key.get(boundary)?;
                                let kind = match boundary.line_commit_cause? {
                                    libmandoc_rs::LineCommitCause::DirectDevice => {
                                        CanonicalEffectKind::LogicalLineCommit {
                                            empty: boundary.visual_before == 0,
                                        }
                                    }
                                    libmandoc_rs::LineCommitCause::FieldEnd => {
                                        CanonicalEffectKind::FieldEnd {
                                            empty: boundary.visual_before == 0,
                                        }
                                    }
                                    libmandoc_rs::LineCommitCause::FieldWrap => {
                                        CanonicalEffectKind::SoftDeviceWrap
                                    }
                                    libmandoc_rs::LineCommitCause::VerticalBlank => {
                                        CanonicalEffectKind::BlankDeviceLine
                                    }
                                };
                                Some(CanonicalEffectStep {
                                    sequence: boundary.sequence,
                                    kind,
                                    inline_flow: boundary.inline_flow,
                                })
                            })
                            .collect::<Vec<_>>();
                        steps.sort_by_key(|step| step.sequence);
                        if steps.is_empty() {
                            CanonicalEffectOutcome::StateOnly
                        } else {
                            CanonicalEffectOutcome::Trace(steps)
                        }
                    }
                    EffectCauseKey::StandaloneFlush(_) | EffectCauseKey::StandaloneGeometry(_) => {
                        CanonicalEffectOutcome::StateOnly
                    }
                };
                Ok(CanonicalEffectPlan {
                    key: EffectKey(
                        u32::try_from(index)
                            .map_err(|_| ProjectionPlanError::NodeCapacityExceeded)?,
                    ),
                    cause,
                    records,
                    sequence,
                    output_role,
                    outcome,
                })
            })
            .collect::<Result<Vec<_>, ProjectionPlanError>>()?;
        flow_geometry.sort_unstable();
        Ok(Self {
            effects,
            flow_geometry,
        })
    }
}

fn boundary_root(
    key: u32,
    boundaries: &BTreeMap<u32, BoundaryFact>,
) -> Result<u32, ProjectionPlanError> {
    let mut current = key;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current) {
            return Err(ProjectionPlanError::CausalCycle {
                kind: RecordKind::Boundary,
                key,
            });
        }
        let record =
            boundaries
                .get(&current)
                .ok_or(ProjectionPlanError::InvalidCausalReference {
                    kind: RecordKind::Boundary,
                    key,
                    parent: current,
                })?;
        let Some(parent) = record.parent else {
            return Ok(current);
        };
        current = parent;
    }
}

fn push_effect_record(
    groups: &mut BTreeMap<EffectCauseKey, (u64, Vec<RawRecordRef>)>,
    cause: EffectCauseKey,
    sequence: u64,
    record: RawRecordRef,
) {
    let group = groups.entry(cause).or_insert((sequence, Vec::new()));
    group.0 = group.0.min(sequence);
    group.1.push(record);
}

#[derive(Debug)]
pub(super) struct ProjectionArenaBuilder {
    nodes: Vec<ProjectionNode>,
    ownership: ExecutionOwnership,
    /// Anchors the typed specialized model can actually reproduce.
    ///
    /// This is intentionally narrower than the execution ledger.  A record
    /// may execute inside a table/equation subtree without having a field in
    /// the source-neutral typed model.  Such a record must remain visible to
    /// validation and fail closed instead of being "absorbed" on paper.
    specialized_anchor_consumers: BTreeMap<ProjectionNodeKey, BTreeSet<u32>>,
    specialized_absorptions: Vec<SpecializedAbsorption>,
    heading_absorptions: Vec<HeadingAbsorption>,
}

impl ProjectionArenaBuilder {
    pub(super) fn new(counts: ExecutionRecordCounts) -> Self {
        Self {
            nodes: Vec::new(),
            ownership: ExecutionOwnership {
                atoms: vec![RecordDisposition::Unassigned; counts.atoms],
                fragments: vec![RecordDisposition::Unassigned; counts.fragments],
                flushes: vec![RecordDisposition::Unassigned; counts.flushes],
                boundaries: vec![RecordDisposition::Unassigned; counts.boundaries],
                controls: vec![RecordDisposition::Unassigned; counts.controls],
                geometry: vec![RecordDisposition::Unassigned; counts.geometry],
                placements: vec![RecordDisposition::Unassigned; counts.placements],
                anchors: vec![RecordDisposition::Unassigned; counts.anchors],
            },
            specialized_anchor_consumers: BTreeMap::new(),
            specialized_absorptions: Vec::new(),
            heading_absorptions: Vec::new(),
        }
    }

    /// Seed the structural arena from report execution intervals.
    ///
    /// Native wrapper ancestry and explicit renderer edges establish known
    /// relationships.  All remaining owners are nested by their laminar
    /// execution intervals; source-tree ancestry is provenance only.
    #[allow(clippy::too_many_lines)]
    pub(super) fn from_native(
        report: &NativeExecutionReport,
        projection: &NativeProjection,
    ) -> Result<NativeArenaSeed, ProjectionPlanError> {
        let mut builder = Self::new(ExecutionRecordCounts {
            atoms: report.atoms().len(),
            fragments: report.fragments().len(),
            flushes: report.flushes().len(),
            boundaries: report.boundaries().len(),
            controls: report.controls().len(),
            geometry: report.geometry().len(),
            placements: report.placements().len(),
            anchors: report.anchors().len(),
        });
        validate_projection_cardinality(
            "anchors",
            report.anchors().len(),
            projection.anchors.len(),
        )?;
        validate_projection_cardinality("tables", report.tables().len(), projection.tables.len())?;
        validate_projection_cardinality(
            "equations",
            report.equations().len(),
            projection.equations.len(),
        )?;

        let root = builder.add_node(
            ProjectionNodeKind::Root,
            None,
            ExecutionSpan {
                atoms: 0..u32::try_from(report.atoms().len())
                    .map_err(|_| ProjectionPlanError::NodeCapacityExceeded)?,
                enter_sequence: 0,
                leave_sequence: report_leave_sequence(report),
            },
        )?;

        let mdoc_items = projection
            .mdoc_lists
            .iter()
            .flat_map(|list| list.items.iter())
            .map(|item| (item.wrapper, item))
            .collect::<BTreeMap<_, _>>();
        let man_blocks = projection
            .man_blocks
            .iter()
            .map(|block| (block.wrapper, block))
            .collect::<BTreeMap<_, _>>();
        for wrapper in report.wrappers() {
            match wrapper.kind {
                ExecutionWrapperKind::MdocListItem if !mdoc_items.contains_key(&wrapper.key) => {
                    return Err(ProjectionPlanError::MissingNativeParent {
                        kind: "mdoc-list-item-fact",
                        key: wrapper.key,
                    });
                }
                ExecutionWrapperKind::ManBlock if !man_blocks.contains_key(&wrapper.key) => {
                    return Err(ProjectionPlanError::MissingNativeParent {
                        kind: "man-block-fact",
                        key: wrapper.key,
                    });
                }
                _ => {}
            }
        }
        let mut wrapper_nodes = vec![None; report.wrappers().len()];
        for wrapper in report.wrappers() {
            let node_kind = if wrapper.kind == ExecutionWrapperKind::Heading {
                ProjectionNodeKind::Heading {
                    wrapper: wrapper.key,
                    kind: wrapper
                        .heading_kind
                        .ok_or(ProjectionPlanError::MissingNativeParent {
                            kind: "heading-kind",
                            key: wrapper.key,
                        })?,
                }
            } else if let Some(item) = mdoc_items.get(&wrapper.key) {
                ProjectionNodeKind::MdocItem {
                    owner: item.owner,
                    flow_epoch: item.flow_epoch,
                    kind: item.kind,
                    compact: item.compact,
                }
            } else if let Some(block) = man_blocks.get(&wrapper.key)
                && matches!(
                    block.kind,
                    ExecutionManBlockKind::IndentedParagraph
                        | ExecutionManBlockKind::TaggedParagraph
                        | ExecutionManBlockKind::AdditionalTag
                )
            {
                if block.kind == ExecutionManBlockKind::AdditionalTag {
                    ProjectionNodeKind::ManDefinitionContinuation { owner: block.owner }
                } else {
                    ProjectionNodeKind::ManDefinitionItem {
                        primary: block.owner,
                        kind: block.kind,
                    }
                }
            } else {
                let Some(kind) = native_scope_kind(wrapper) else {
                    continue;
                };
                ProjectionNodeKind::NativeScope {
                    wrapper: wrapper.key,
                    kind,
                }
            };
            let key = builder.add_node(
                node_kind,
                wrapper.node,
                ExecutionSpan {
                    atoms: wrapper.enter_atom..wrapper.leave_atom,
                    enter_sequence: wrapper.enter_sequence,
                    leave_sequence: wrapper.leave_sequence,
                },
            )?;
            wrapper_nodes[wrapper.key as usize] = Some(key);
        }

        let node_wrappers = super::native_node_wrapper_index(report);
        let mut list_nodes = Vec::with_capacity(projection.mdoc_lists.len());
        for list in &projection.mdoc_lists {
            let wrapper_index = node_wrappers
                .get(list.owner.0 as usize)
                .and_then(|value| *value)
                .ok_or(ProjectionPlanError::MissingNativeParent {
                    kind: "mdoc-list-block-wrapper",
                    key: list.owner.0,
                })?;
            let block_wrapper = &report.wrappers()[wrapper_index];
            list_nodes.push(builder.add_node(
                ProjectionNodeKind::MdocList {
                    owner: list.owner,
                    kind: list.kind,
                },
                Some(list.owner),
                ExecutionSpan {
                    atoms: block_wrapper.enter_atom..block_wrapper.leave_atom,
                    enter_sequence: block_wrapper.enter_sequence,
                    leave_sequence: block_wrapper.leave_sequence,
                },
            )?);
        }

        let mut list_by_item = vec![None; report.nodes().len()];
        for (list_index, list) in projection.mdoc_lists.iter().enumerate() {
            for item in &list.items {
                let slot = &mut list_by_item[item.owner.0 as usize];
                if slot.replace(list_nodes[list_index]).is_some() {
                    return Err(ProjectionPlanError::NativeOwnerConflict(item.owner));
                }
            }
        }

        // Create exact definition HEAD/BODY owners before attaching nested
        // regions or lists.  This mirrors the fixed-CVS pre/children/post
        // execution hierarchy: nested structures inside a BODY become
        // children of that segment instead of siblings later reassigned from
        // final indentation.
        // Syntax HEAD/BODY wrappers are the stable structural address of a
        // definition segment.  Keep a dedicated wrapper -> owner map and let
        // descendants resolve it through the native dynamic wrapper chain;
        // execution intervals are not identities (macro expansion routinely
        // gives distinct owners identical envelopes).
        let mut structural_wrapper_nodes = wrapper_nodes.clone();
        for list in &projection.mdoc_lists {
            for item in &list.items {
                let item_scope = wrapper_nodes[item.wrapper as usize].ok_or(
                    ProjectionPlanError::MissingNativeParent {
                        kind: "mdoc-list-item-scope",
                        key: item.owner.0,
                    },
                )?;
                let mut add_segment = |builder: &mut Self,
                                       segment: &super::NativeMdocListSegment,
                                       kind: DefinitionSegmentKind|
                 -> Result<(), ProjectionPlanError> {
                    let wrapper_index = node_wrappers
                        .get(segment.node.0 as usize)
                        .and_then(|value| *value)
                        .ok_or(ProjectionPlanError::MissingNativeParent {
                            kind: "mdoc-definition-segment-wrapper",
                            key: segment.node.0,
                        })?;
                    let wrapper = &report.wrappers()[wrapper_index];
                    let key = builder.add_node(
                        ProjectionNodeKind::DefinitionSegment { kind },
                        Some(segment.node),
                        ExecutionSpan {
                            atoms: wrapper.enter_atom..wrapper.leave_atom,
                            enter_sequence: wrapper.enter_sequence,
                            leave_sequence: wrapper.leave_sequence,
                        },
                    )?;
                    builder.attach_structural(item_scope, key)?;
                    let slot = &mut structural_wrapper_nodes[wrapper_index];
                    if slot.replace(key).is_some() {
                        return Err(ProjectionPlanError::NativeOwnerConflict(segment.node));
                    }
                    Ok(())
                };
                add_segment(&mut builder, &item.head, DefinitionSegmentKind::MdocHead)?;
                for (ordinal, body) in item.bodies.iter().enumerate() {
                    add_segment(
                        &mut builder,
                        body,
                        DefinitionSegmentKind::MdocBody {
                            ordinal: u32::try_from(ordinal)
                                .map_err(|_| ProjectionPlanError::NodeCapacityExceeded)?,
                        },
                    )?;
                }
            }
        }
        for block in &projection.man_blocks {
            // Fixed CVS gives every man block a HEAD/BODY syntax shape, but
            // only IP/TP/TQ execute that shape as a definition field.  PP/P/
            // LP/HP heads are control operands (often empty), not inline
            // products to be replayed by an ordinary block scope.
            if !matches!(
                block.kind,
                ExecutionManBlockKind::IndentedParagraph
                    | ExecutionManBlockKind::TaggedParagraph
                    | ExecutionManBlockKind::AdditionalTag
            ) {
                continue;
            }
            let block_scope = wrapper_nodes[block.wrapper as usize].ok_or(
                ProjectionPlanError::MissingNativeParent {
                    kind: "man-block-scope",
                    key: block.owner.0,
                },
            )?;
            for (segment, kind) in [
                (&block.head, DefinitionSegmentKind::ManHead),
                (&block.body, DefinitionSegmentKind::ManBody),
            ] {
                let wrapper_index = node_wrappers
                    .get(segment.node.0 as usize)
                    .and_then(|value| *value)
                    .ok_or(ProjectionPlanError::MissingNativeParent {
                        kind: "man-definition-segment-wrapper",
                        key: segment.node.0,
                    })?;
                let wrapper = &report.wrappers()[wrapper_index];
                let key = builder.add_node(
                    ProjectionNodeKind::DefinitionSegment { kind },
                    Some(segment.node),
                    ExecutionSpan {
                        atoms: wrapper.enter_atom..wrapper.leave_atom,
                        enter_sequence: wrapper.enter_sequence,
                        leave_sequence: wrapper.leave_sequence,
                    },
                )?;
                builder.attach_structural(block_scope, key)?;
                let slot = &mut structural_wrapper_nodes[wrapper_index];
                if slot.replace(key).is_some() {
                    return Err(ProjectionPlanError::NativeOwnerConflict(segment.node));
                }
            }
        }

        for wrapper in report.wrappers() {
            let Some(child) = wrapper_nodes[wrapper.key as usize] else {
                continue;
            };
            let parent = wrapper
                .node
                .and_then(|node| list_by_item[node.0 as usize])
                .or_else(|| {
                    nearest_structural_parent(report, &structural_wrapper_nodes, wrapper.parent)
                });
            if let Some(parent) = parent {
                if builder.nodes[child.0 as usize].execution_parent.is_some() {
                    // Definition segments were attached to their item/block
                    // scope above.  Do not attach those owning wrappers again.
                    continue;
                }
                if wrapper.kind == ExecutionWrapperKind::Heading {
                    builder.attach_execution(parent, child)?;
                } else {
                    builder.attach_structural(parent, child)?;
                }
            }
        }
        for (list_index, list) in projection.mdoc_lists.iter().enumerate() {
            let wrapper_index = node_wrappers[list.owner.0 as usize].ok_or(
                ProjectionPlanError::MissingNativeParent {
                    kind: "mdoc-list-block-wrapper",
                    key: list.owner.0,
                },
            )?;
            let wrapper = &report.wrappers()[wrapper_index];
            if let Some(parent) =
                nearest_structural_parent(report, &structural_wrapper_nodes, wrapper.parent)
            {
                builder.attach_structural(parent, list_nodes[list_index])?;
            }
        }

        let mut table_nodes = vec![None; report.tables().len()];
        for table in report.tables() {
            let key = builder.add_node(
                ProjectionNodeKind::Table(table.key),
                Some(table.first_row_node),
                ExecutionSpan {
                    atoms: table.atoms.clone(),
                    enter_sequence: table.enter_sequence,
                    leave_sequence: table.leave_sequence,
                },
            )?;
            if let Some(parent) =
                nearest_structural_parent(report, &structural_wrapper_nodes, table.wrapper)
            {
                builder.attach_structural(parent, key)?;
            }
            table_nodes[table.key.0 as usize] = Some(key);
        }
        for table in &projection.tables {
            let owner = table_nodes
                .get(table.key.0 as usize)
                .and_then(|value| *value)
                .ok_or(ProjectionPlanError::MissingNativeParent {
                    kind: "typed-table-anchor-consumer",
                    key: table.key.0,
                })?;
            let mut anchors = BTreeSet::new();
            for anchor in &table.leading_anchors {
                if usize::try_from(*anchor)
                    .ok()
                    .is_none_or(|index| index >= report.anchors().len())
                {
                    return Err(ProjectionPlanError::RecordOutOfBounds {
                        kind: RecordKind::Anchor,
                        index: *anchor,
                    });
                }
                anchors.insert(*anchor);
            }
            if builder
                .specialized_anchor_consumers
                .insert(owner, anchors)
                .is_some()
            {
                return Err(ProjectionPlanError::InvalidSpecializedRecipe(owner));
            }
        }
        for row in report.table_rows() {
            let key = builder.add_node(
                ProjectionNodeKind::TableRow(row.key),
                Some(row.node),
                ExecutionSpan {
                    atoms: row.atoms.clone(),
                    enter_sequence: row.enter_sequence,
                    leave_sequence: row.leave_sequence,
                },
            )?;
            let table = table_nodes
                .get(row.table.0 as usize)
                .and_then(|value| *value)
                .ok_or(ProjectionPlanError::MissingNativeParent {
                    kind: "table-row",
                    key: row.key.0,
                })?;
            builder.attach_structural(table, key)?;
            for invocation in
                &report.table_cell_invocations()[usize::try_from(row.cell_invocations.start)
                    .unwrap()
                    ..usize::try_from(row.cell_invocations.end).unwrap()]
            {
                let cell_key = builder.add_node(
                    ProjectionNodeKind::TableCellInvocation(invocation.key),
                    Some(invocation.node),
                    ExecutionSpan {
                        atoms: invocation.atoms.clone(),
                        enter_sequence: invocation.enter_sequence,
                        leave_sequence: invocation.leave_sequence,
                    },
                )?;
                builder.attach_structural(key, cell_key)?;
            }
        }

        let mut equation_nodes = vec![None; report.equations().len()];
        for equation in report.equations() {
            let key = builder.add_node(
                ProjectionNodeKind::Equation(equation.key),
                Some(equation.node),
                ExecutionSpan {
                    atoms: equation.atoms.clone(),
                    enter_sequence: equation.enter_sequence,
                    leave_sequence: equation.leave_sequence,
                },
            )?;
            let wrapper = node_wrappers
                .get(equation.node.0 as usize)
                .and_then(|value| *value)
                .map(|value| {
                    u32::try_from(value).map_err(|_| ProjectionPlanError::NodeCapacityExceeded)
                })
                .transpose()?;
            if let Some(parent) =
                nearest_structural_parent(report, &structural_wrapper_nodes, wrapper)
            {
                builder.attach_structural(parent, key)?;
            }
            equation_nodes[equation.key.0 as usize] = Some(key);
        }

        let mut equation_invocation_nodes = vec![None; report.equation_invocations().len()];
        for invocation in report.equation_invocations() {
            let key = builder.add_node(
                ProjectionNodeKind::EquationInvocation(invocation.key),
                None,
                ExecutionSpan {
                    atoms: invocation.atoms.clone(),
                    enter_sequence: invocation.enter_sequence,
                    leave_sequence: invocation.leave_sequence,
                },
            )?;
            equation_invocation_nodes[invocation.key.0 as usize] = Some(key);
        }
        for invocation in report.equation_invocations() {
            let child = equation_invocation_nodes[invocation.key.0 as usize].ok_or(
                ProjectionPlanError::MissingNativeParent {
                    kind: "equation-invocation",
                    key: invocation.key.0,
                },
            )?;
            let parent = if let Some(parent) = invocation.parent {
                equation_invocation_nodes
                    .get(parent.0 as usize)
                    .and_then(|value| *value)
            } else {
                equation_nodes
                    .get(invocation.equation.0 as usize)
                    .and_then(|value| *value)
            }
            .ok_or(ProjectionPlanError::MissingNativeParent {
                kind: "equation-invocation-parent",
                key: invocation.key.0,
            })?;
            builder.attach_structural(parent, child)?;
        }

        builder.attach_remaining_by_interval(root)?;
        builder.establish_man_definition_topology(
            report,
            projection,
            &structural_wrapper_nodes,
            root,
        )?;
        let atom_owners = build_atom_owner_index(&builder.nodes, report, |node| {
            !matches!(node.kind, ProjectionNodeKind::ManDefinitionRun { .. })
        })?;
        let specialized_atom_owners = build_atom_owner_index(&builder.nodes, report, |node| {
            specialized_recipe(node.kind).is_some()
        })?;
        let specialized_nodes = builder
            .nodes
            .iter()
            .filter(|node| specialized_recipe(node.kind).is_some())
            .map(|node| node.key)
            .collect();

        let effect_ledger = EffectLedger::from_native(report)?;
        Ok(NativeArenaSeed {
            builder,
            // Direct record ownership must see the exact HEAD/BODY owner,
            // not merely its enclosing item/block scope.
            wrapper_nodes: structural_wrapper_nodes,
            node_wrappers,
            atom_owners,
            specialized_atom_owners,
            specialized_nodes,
            effect_ledger,
        })
    }

    fn add_node(
        &mut self,
        kind: ProjectionNodeKind,
        source_owner: Option<ExecutionNodeKey>,
        span: ExecutionSpan,
    ) -> Result<ProjectionNodeKey, ProjectionPlanError> {
        if !span.is_well_formed(self.ownership.atoms.len()) {
            return Err(ProjectionPlanError::InvalidSpan {
                atoms: span.atoms,
                enter_sequence: span.enter_sequence,
                leave_sequence: span.leave_sequence,
                atom_count: self.ownership.atoms.len(),
            });
        }
        let key = ProjectionNodeKey(
            u32::try_from(self.nodes.len())
                .map_err(|_| ProjectionPlanError::NodeCapacityExceeded)?,
        );
        self.nodes.push(ProjectionNode {
            key,
            execution_parent: None,
            content_parent: None,
            destination: if matches!(kind, ProjectionNodeKind::Root) {
                ProjectionDestination::Root
            } else {
                ProjectionDestination::Deferred
            },
            source_owner,
            span,
            kind,
            pieces: Vec::new(),
        });
        Ok(key)
    }

    fn attach_structural(
        &mut self,
        parent: ProjectionNodeKey,
        child: ProjectionNodeKey,
    ) -> Result<(), ProjectionPlanError> {
        self.preflight_execution_attachment(parent, child)?;
        if self.nodes[child.0 as usize].content_parent.is_some() {
            return Err(ProjectionPlanError::ParentAlreadyAssigned(child));
        }
        self.nodes[child.0 as usize].execution_parent = Some(parent);
        self.nodes[child.0 as usize].content_parent = Some(parent);
        let order = ProjectionOrder {
            sequence: self.nodes[child.0 as usize].span.enter_sequence,
            phase: ProjectionPhase::Content,
            stable: child.0,
        };
        let role = child_role(self.nodes[child.0 as usize].kind);
        self.nodes[parent.0 as usize]
            .pieces
            .push(ProjectionPiece::Child {
                node: child,
                role,
                destination: ProjectionDestination::Deferred,
                order,
            });
        Ok(())
    }

    fn attach_execution(
        &mut self,
        parent: ProjectionNodeKey,
        child: ProjectionNodeKey,
    ) -> Result<(), ProjectionPlanError> {
        self.preflight_execution_attachment(parent, child)?;
        self.nodes[child.0 as usize].execution_parent = Some(parent);
        Ok(())
    }

    fn preflight_execution_attachment(
        &self,
        parent: ProjectionNodeKey,
        child: ProjectionNodeKey,
    ) -> Result<(), ProjectionPlanError> {
        let Some(parent_node) = self.nodes.get(parent.0 as usize) else {
            return Err(ProjectionPlanError::UnknownNode(parent));
        };
        let Some(child_node) = self.nodes.get(child.0 as usize) else {
            return Err(ProjectionPlanError::UnknownNode(child));
        };
        if child_node.execution_parent.is_some() {
            return Err(ProjectionPlanError::ParentAlreadyAssigned(child));
        }
        let mut ancestor = Some(parent);
        while let Some(key) = ancestor {
            if key == child {
                return Err(ProjectionPlanError::ParentCycle { parent, child });
            }
            ancestor = self.nodes[key.0 as usize].execution_parent;
        }
        if !parent_node.span.contains(&child_node.span) {
            return Err(ProjectionPlanError::ParentDoesNotContainChild { parent, child });
        }
        Ok(())
    }

    #[cfg(test)]
    fn claim_flow(
        &mut self,
        owner: ProjectionNodeKey,
        atoms: Range<u32>,
        order: ProjectionOrder,
    ) -> Result<(), ProjectionPlanError> {
        self.claim_flow_records(owner, atoms, Vec::new(), Vec::new(), order)
    }

    fn claim_flow_records(
        &mut self,
        owner: ProjectionNodeKey,
        atoms: Range<u32>,
        fragments: Vec<u32>,
        geometry: Vec<u32>,
        order: ProjectionOrder,
    ) -> Result<(), ProjectionPlanError> {
        let node = self
            .nodes
            .get(owner.0 as usize)
            .ok_or(ProjectionPlanError::UnknownNode(owner))?;
        if atoms.start < node.span.atoms.start || node.span.atoms.end < atoms.end {
            return Err(ProjectionPlanError::RecordOutsideOwner {
                kind: RecordKind::Atom,
                index: atoms.start,
                owner,
            });
        }
        validate_piece_order(node, order)?;
        self.preflight_claim_range(RecordKind::Atom, atoms.clone())?;
        for &fragment in &fragments {
            self.preflight_claim_range(RecordKind::Fragment, fragment..fragment + 1)?;
        }
        for &record in &geometry {
            self.preflight_claim_range(RecordKind::Geometry, record..record + 1)?;
        }
        self.assign_range(RecordKind::Atom, owner, atoms.clone());
        for &fragment in &fragments {
            self.assign_range(RecordKind::Fragment, owner, fragment..fragment + 1);
        }
        for &record in &geometry {
            self.assign_range(RecordKind::Geometry, owner, record..record + 1);
        }
        self.node_mut(owner)?.pieces.push(ProjectionPiece::Stream {
            events: vec![StreamEvent::Flow {
                atoms,
                fragments,
                geometry,
            }],
            destination: ProjectionDestination::Deferred,
            order,
        });
        Ok(())
    }

    #[cfg(test)]
    fn claim_fragment(
        &mut self,
        owner: ProjectionNodeKey,
        key: u32,
        order: ProjectionOrder,
    ) -> Result<(), ProjectionPlanError> {
        let node = self
            .nodes
            .get(owner.0 as usize)
            .ok_or(ProjectionPlanError::UnknownNode(owner))?;
        validate_piece_order(node, order)?;
        self.claim_one(RecordKind::Fragment, owner, key)?;
        Ok(())
    }

    fn claim_placement(
        &mut self,
        owner: ProjectionNodeKey,
        key: u32,
        order: ProjectionOrder,
    ) -> Result<(), ProjectionPlanError> {
        let node = self
            .nodes
            .get(owner.0 as usize)
            .ok_or(ProjectionPlanError::UnknownNode(owner))?;
        validate_piece_order(node, order)?;
        self.claim_one(RecordKind::Placement, owner, key)?;
        self.node_mut(owner)?.pieces.push(ProjectionPiece::Stream {
            events: vec![StreamEvent::Placement { key }],
            destination: ProjectionDestination::Deferred,
            order,
        });
        Ok(())
    }

    fn claim_control(
        &mut self,
        owner: ProjectionNodeKey,
        key: u32,
        order: ProjectionOrder,
    ) -> Result<(), ProjectionPlanError> {
        let node = self
            .nodes
            .get(owner.0 as usize)
            .ok_or(ProjectionPlanError::UnknownNode(owner))?;
        validate_piece_order(node, order)?;
        self.claim_one(RecordKind::Control, owner, key)?;
        self.node_mut(owner)?.pieces.push(ProjectionPiece::Stream {
            events: vec![StreamEvent::Control { key }],
            destination: ProjectionDestination::Deferred,
            order,
        });
        Ok(())
    }

    /// Claim a report-derived causal closure as one source-neutral formatter
    /// effect.  Callers cannot assemble arbitrary raw-record groups.
    ///
    /// Fixed CVS can emit several observer records for one semantic boundary
    /// (for example, a field settlement followed by its origin transition).
    /// They are projected once through this canonical piece, never replayed as
    /// independent whitespace or geometry nodes.
    fn claim_effect_plan(
        &mut self,
        owner: ProjectionNodeKey,
        plan: &CanonicalEffectPlan,
    ) -> Result<(), ProjectionPlanError> {
        let order = ProjectionOrder {
            sequence: plan.sequence,
            phase: ProjectionPhase::Content,
            stable: plan.key.0,
        };
        let node = self
            .nodes
            .get(owner.0 as usize)
            .ok_or(ProjectionPlanError::UnknownNode(owner))?;
        validate_piece_order(node, order)?;

        let mut unique = BTreeSet::new();
        for record in &plan.records {
            let (kind, index) = record.parts();
            if !unique.insert((kind, index)) {
                return Err(ProjectionPlanError::RecordAlreadyAssigned {
                    kind,
                    index,
                    disposition: RecordDisposition::Owned(owner),
                });
            }
            let disposition = ledger(&self.ownership, kind)
                .get(index as usize)
                .copied()
                .ok_or(ProjectionPlanError::RecordOutOfBounds { kind, index })?;
            if disposition != RecordDisposition::Unassigned {
                return Err(ProjectionPlanError::RecordAlreadyAssigned {
                    kind,
                    index,
                    disposition,
                });
            }
        }
        for record in &plan.records {
            let (kind, index) = record.parts();
            self.claim_one(kind, owner, index)?;
        }
        self.node_mut(owner)?.pieces.push(ProjectionPiece::Stream {
            events: vec![StreamEvent::CanonicalEffect {
                key: plan.key,
                records: plan.records.clone(),
                outcome: plan.outcome.clone(),
            }],
            destination: ProjectionDestination::Deferred,
            order,
        });
        Ok(())
    }

    fn claim_anchor(
        &mut self,
        owner: ProjectionNodeKey,
        key: u32,
        order: ProjectionOrder,
    ) -> Result<(), ProjectionPlanError> {
        let node = self
            .nodes
            .get(owner.0 as usize)
            .ok_or(ProjectionPlanError::UnknownNode(owner))?;
        validate_piece_order(node, order)?;
        self.claim_one(RecordKind::Anchor, owner, key)?;
        self.node_mut(owner)?.pieces.push(ProjectionPiece::Stream {
            events: vec![StreamEvent::Anchor { key }],
            destination: ProjectionDestination::Deferred,
            order,
        });
        Ok(())
    }

    fn omit_atom_range(
        &mut self,
        atoms: Range<u32>,
        reason: OmissionReason,
    ) -> Result<(), ProjectionPlanError> {
        if !matches!(
            reason,
            OmissionReason::ReplacedAtom
                | OmissionReason::ConsumedAtom
                | OmissionReason::TrailingDiscard
                | OmissionReason::DeviceDecoration
                | OmissionReason::MarginDecoration
        ) {
            return Err(ProjectionPlanError::InvalidOmission {
                kind: RecordKind::Atom,
                reason,
            });
        }
        self.omit_range(RecordKind::Atom, atoms, reason)
    }

    #[cfg(test)]
    fn omit_boundary(
        &mut self,
        key: u32,
        reason: OmissionReason,
    ) -> Result<(), ProjectionPlanError> {
        if !matches!(
            reason,
            OmissionReason::SoftDeviceWrap | OmissionReason::StateOnly
        ) {
            return Err(ProjectionPlanError::InvalidOmission {
                kind: RecordKind::Boundary,
                reason,
            });
        }
        self.omit_one(RecordKind::Boundary, key, reason)
    }

    fn omit_fragment(
        &mut self,
        key: u32,
        reason: OmissionReason,
    ) -> Result<(), ProjectionPlanError> {
        if !matches!(
            reason,
            OmissionReason::DeviceDecoration | OmissionReason::MarginDecoration
        ) {
            return Err(ProjectionPlanError::InvalidOmission {
                kind: RecordKind::Fragment,
                reason,
            });
        }
        self.omit_one(RecordKind::Fragment, key, reason)
    }

    fn omit_effect_plan(
        &mut self,
        plan: &CanonicalEffectPlan,
        reason: OmissionReason,
    ) -> Result<(), ProjectionPlanError> {
        if !matches!(
            reason,
            OmissionReason::DeviceDecoration | OmissionReason::MarginDecoration
        ) {
            return Err(ProjectionPlanError::InvalidOmission {
                kind: RecordKind::Boundary,
                reason,
            });
        }
        for record in &plan.records {
            let (kind, key) = record.parts();
            if !matches!(
                kind,
                RecordKind::Flush | RecordKind::Boundary | RecordKind::Geometry
            ) {
                return Err(ProjectionPlanError::InvalidOmission { kind, reason });
            }
            self.omit_one(kind, key, reason)?;
        }
        Ok(())
    }

    fn omit_geometry(
        &mut self,
        key: u32,
        reason: OmissionReason,
    ) -> Result<(), ProjectionPlanError> {
        let valid = matches!(
            reason,
            OmissionReason::DeviceDecoration
                | OmissionReason::MarginDecoration
                | OmissionReason::ReplacedAtom
                | OmissionReason::ConsumedAtom
                | OmissionReason::TrailingDiscard
        );
        #[cfg(test)]
        let valid = valid
            || matches!(
                reason,
                OmissionReason::SoftDeviceWrap | OmissionReason::StateOnly
            );
        if !valid {
            return Err(ProjectionPlanError::InvalidOmission {
                kind: RecordKind::Geometry,
                reason,
            });
        }
        self.omit_one(RecordKind::Geometry, key, reason)
    }

    fn finish(mut self) -> Result<ProjectionArena, ProjectionPlanError> {
        for node in &self.nodes {
            if node.execution_parent.is_none() && !matches!(node.kind, ProjectionNodeKind::Root) {
                return Err(ProjectionPlanError::OrphanedNode(node.key));
            }
            if node.destination == ProjectionDestination::Deferred {
                return Err(ProjectionPlanError::UnresolvedDestination(node.key));
            }
        }
        for node in &self.nodes {
            if let Some(piece) = node
                .pieces
                .iter()
                .find(|piece| piece.destination() == ProjectionDestination::Deferred)
            {
                return Err(ProjectionPlanError::UnresolvedPieceDestination {
                    owner: node.key,
                    sequence: piece.order().sequence,
                });
            }
        }
        self.validate_children()?;
        validate_total(RecordKind::Atom, &self.ownership.atoms)?;
        validate_total(RecordKind::Fragment, &self.ownership.fragments)?;
        validate_total(RecordKind::Flush, &self.ownership.flushes)?;
        validate_total(RecordKind::Boundary, &self.ownership.boundaries)?;
        validate_total(RecordKind::Control, &self.ownership.controls)?;
        validate_total(RecordKind::Geometry, &self.ownership.geometry)?;
        validate_total(RecordKind::Placement, &self.ownership.placements)?;
        validate_total(RecordKind::Anchor, &self.ownership.anchors)?;
        validate_specialized_recipes(&self.nodes)?;
        validate_absorption_receipts(
            &self.nodes,
            &self.ownership,
            &self.specialized_anchor_consumers,
            &self.specialized_absorptions,
            &self.heading_absorptions,
        )?;
        for node in &mut self.nodes {
            node.pieces.sort_by_key(ProjectionPiece::order);
            coalesce_streams(&mut node.pieces);
        }
        let final_consumers = final_consumer_receipts(
            &self.nodes,
            &self.ownership,
            &self.specialized_absorptions,
            &self.heading_absorptions,
        )?;
        let roots = self
            .nodes
            .iter()
            .filter(|node| node.execution_parent.is_none())
            .map(|node| node.key)
            .collect::<Vec<_>>();
        if roots.len() != 1
            || !matches!(
                self.nodes[roots[0].0 as usize].kind,
                ProjectionNodeKind::Root
            )
        {
            return Err(ProjectionPlanError::InvalidRootCount(roots.len()));
        }
        Ok(ProjectionArena {
            nodes: self.nodes,
            roots,
            ownership: self.ownership,
            specialized_absorptions: self.specialized_absorptions,
            heading_absorptions: self.heading_absorptions,
            final_consumers,
        })
    }

    fn node_mut(
        &mut self,
        key: ProjectionNodeKey,
    ) -> Result<&mut ProjectionNode, ProjectionPlanError> {
        self.nodes
            .get_mut(key.0 as usize)
            .ok_or(ProjectionPlanError::UnknownNode(key))
    }

    fn claim_range(
        &mut self,
        kind: RecordKind,
        owner: ProjectionNodeKey,
        range: Range<u32>,
    ) -> Result<(), ProjectionPlanError> {
        if self.nodes.get(owner.0 as usize).is_none() {
            return Err(ProjectionPlanError::UnknownNode(owner));
        }
        self.preflight_claim_range(kind, range.clone())?;
        self.assign_range(kind, owner, range);
        Ok(())
    }

    fn preflight_claim_range(
        &self,
        kind: RecordKind,
        range: Range<u32>,
    ) -> Result<(), ProjectionPlanError> {
        let slots = ledger(&self.ownership, kind);
        let start = usize::try_from(range.start).unwrap();
        let end = usize::try_from(range.end).unwrap();
        if start > end || end > slots.len() {
            return Err(ProjectionPlanError::RecordOutOfBounds {
                kind,
                index: range.end,
            });
        }
        for (offset, disposition) in slots[start..end].iter().enumerate() {
            if *disposition != RecordDisposition::Unassigned {
                return Err(ProjectionPlanError::RecordAlreadyAssigned {
                    kind,
                    index: range.start + u32::try_from(offset).unwrap(),
                    disposition: *disposition,
                });
            }
        }
        Ok(())
    }

    fn assign_range(&mut self, kind: RecordKind, owner: ProjectionNodeKey, range: Range<u32>) {
        let start = usize::try_from(range.start).unwrap();
        let end = usize::try_from(range.end).unwrap();
        ledger_mut(&mut self.ownership, kind)[start..end].fill(RecordDisposition::Owned(owner));
    }

    fn claim_one(
        &mut self,
        kind: RecordKind,
        owner: ProjectionNodeKey,
        key: u32,
    ) -> Result<(), ProjectionPlanError> {
        let end = key
            .checked_add(1)
            .ok_or(ProjectionPlanError::RecordOutOfBounds { kind, index: key })?;
        self.claim_range(kind, owner, key..end)
    }

    fn omit_range(
        &mut self,
        kind: RecordKind,
        range: Range<u32>,
        reason: OmissionReason,
    ) -> Result<(), ProjectionPlanError> {
        let slots = ledger_mut(&mut self.ownership, kind);
        let start = usize::try_from(range.start).unwrap();
        let end = usize::try_from(range.end).unwrap();
        if start > end || end > slots.len() {
            return Err(ProjectionPlanError::RecordOutOfBounds {
                kind,
                index: range.end,
            });
        }
        for (offset, disposition) in slots[start..end].iter().enumerate() {
            if *disposition != RecordDisposition::Unassigned {
                return Err(ProjectionPlanError::RecordAlreadyAssigned {
                    kind,
                    index: range.start + u32::try_from(offset).unwrap(),
                    disposition: *disposition,
                });
            }
        }
        slots[start..end].fill(RecordDisposition::Omitted(reason));
        Ok(())
    }

    fn omit_one(
        &mut self,
        kind: RecordKind,
        key: u32,
        reason: OmissionReason,
    ) -> Result<(), ProjectionPlanError> {
        let end = key
            .checked_add(1)
            .ok_or(ProjectionPlanError::RecordOutOfBounds { kind, index: key })?;
        self.omit_range(kind, key..end, reason)
    }

    fn validate_children(&self) -> Result<(), ProjectionPlanError> {
        for parent in &self.nodes {
            let mut children = parent
                .pieces
                .iter()
                .filter_map(|piece| match piece {
                    ProjectionPiece::Child { node, .. } => Some(*node),
                    _ => None,
                })
                .collect::<Vec<_>>();
            for child in &children {
                if self.nodes[child.0 as usize].content_parent != Some(parent.key) {
                    return Err(ProjectionPlanError::ChildPieceMismatch(*child));
                }
            }
            children.sort_by_key(|key| {
                let span = &self.nodes[key.0 as usize].span;
                (span.enter_sequence, span.leave_sequence, key.0)
            });
            for pair in children.windows(2) {
                let [left, right] = pair else { unreachable!() };
                if spans_overlap(
                    &self.nodes[left.0 as usize].span,
                    &self.nodes[right.0 as usize].span,
                ) && !matches!(parent.kind, ProjectionNodeKind::ManHangingPair { .. })
                {
                    let left_node = &self.nodes[left.0 as usize];
                    let right_node = &self.nodes[right.0 as usize];
                    return Err(ProjectionPlanError::OverlappingSiblings {
                        left: *left,
                        left_kind: left_node.kind,
                        left_source_owner: left_node.source_owner,
                        left_span: left_node.span.clone(),
                        right: *right,
                        right_kind: right_node.kind,
                        right_source_owner: right_node.source_owner,
                        right_span: right_node.span.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn collapse_specialized_pieces(&mut self) {
        let consumer_roots = self
            .nodes
            .iter()
            .map(|node| top_level_specialized_root(&self.nodes, node.key))
            .collect::<Vec<_>>();
        for (node_index, node) in self.nodes.iter_mut().enumerate() {
            let Some(recipe) = specialized_recipe(node.kind) else {
                continue;
            };
            let anchor_consumer = consumer_roots[node_index]
                .and_then(|root| self.specialized_anchor_consumers.get(&root));
            let mut records = Vec::new();
            let mut retained = Vec::new();
            let mut first_order = ProjectionOrder {
                sequence: node.span.enter_sequence,
                phase: ProjectionPhase::Content,
                stable: node.key.0,
            };
            let mut observed_order = false;
            for piece in std::mem::take(&mut node.pieces) {
                match piece {
                    ProjectionPiece::Stream {
                        events,
                        destination,
                        order,
                    } => {
                        if !observed_order || order < first_order {
                            first_order = order;
                            observed_order = true;
                        }
                        let mut retained_events = Vec::new();
                        for event in events {
                            match event {
                                StreamEvent::Anchor { key }
                                    if !anchor_consumer
                                        .is_some_and(|anchors| anchors.contains(&key)) =>
                                {
                                    retained_events.push(StreamEvent::Anchor { key });
                                }
                                other => records.extend(other.records()),
                            }
                        }
                        if !retained_events.is_empty() {
                            retained.push(ProjectionPiece::Stream {
                                events: retained_events,
                                destination,
                                order,
                            });
                        }
                    }
                    other => retained.push(other),
                }
            }
            records.sort_unstable();
            records.dedup();
            retained.push(ProjectionPiece::Specialized {
                recipe,
                records,
                destination: ProjectionDestination::Deferred,
                order: first_order,
            });
            node.pieces = retained;
        }
    }

    fn absorb_specialized_descendants(&mut self) -> Result<(), ProjectionPlanError> {
        let roots = self
            .nodes
            .iter()
            .filter(|node| match node.kind {
                ProjectionNodeKind::Table(_) => true,
                ProjectionNodeKind::Equation(_) => !has_table_ancestor(&self.nodes, node.key),
                _ => false,
            })
            .map(|node| node.key)
            .collect::<Vec<_>>();
        let mut receipts = Vec::with_capacity(roots.len());
        for root in roots {
            let mut descendants = Vec::new();
            collect_specialized_descendants(&self.nodes, root, &mut descendants)?;
            for descendant in &descendants {
                if *descendant != root
                    && self.nodes[descendant.0 as usize]
                        .pieces
                        .iter()
                        .any(|piece| matches!(piece, ProjectionPiece::Stream { .. }))
                {
                    return Err(ProjectionPlanError::InvalidSpecializedRecipe(*descendant));
                }
            }
            let mut records = descendants
                .iter()
                .flat_map(|key| {
                    self.nodes[key.0 as usize]
                        .pieces
                        .iter()
                        .filter_map(|piece| match piece {
                            ProjectionPiece::Specialized { records, .. } => {
                                Some(records.as_slice())
                            }
                            _ => None,
                        })
                        .flatten()
                        .copied()
                })
                .collect::<Vec<_>>();
            records.sort_unstable();
            records.dedup();
            let root_node = &mut self.nodes[root.0 as usize];
            let root_records = root_node
                .pieces
                .iter_mut()
                .find_map(|piece| match piece {
                    ProjectionPiece::Specialized { records, .. } => Some(records),
                    _ => None,
                })
                .ok_or(ProjectionPlanError::InvalidSpecializedRecipe(root))?;
            *root_records = records.clone();
            receipts.push(SpecializedAbsorption {
                root,
                descendants,
                records,
            });
        }
        self.specialized_absorptions = receipts;
        Ok(())
    }

    fn absorb_heading_descendants(&mut self) -> Result<(), ProjectionPlanError> {
        let roots = self
            .nodes
            .iter()
            .filter(|node| matches!(node.kind, ProjectionNodeKind::Heading { .. }))
            .map(|node| node.key)
            .collect::<Vec<_>>();
        let mut receipts = Vec::with_capacity(roots.len());
        let by_owner = records_by_owner(&self.ownership)?;
        let mut execution_children = vec![Vec::<ProjectionNodeKey>::new(); self.nodes.len()];
        for node in &self.nodes {
            if let Some(parent) = node.execution_parent {
                execution_children[parent.0 as usize].push(node.key);
            }
        }
        for root in roots {
            let mut descendants = Vec::new();
            collect_execution_descendants(&execution_children, root, &mut descendants);
            let records = collect_owner_records(&by_owner, &descendants);
            receipts.push(HeadingAbsorption {
                root,
                descendants,
                records,
            });
        }
        self.heading_absorptions = receipts;
        Ok(())
    }

    fn attach_remaining_by_interval(
        &mut self,
        root: ProjectionNodeKey,
    ) -> Result<(), ProjectionPlanError> {
        let depths = self
            .nodes
            .iter()
            .map(|node| node_depth(&self.nodes, node.key))
            .collect::<Vec<_>>();
        let mut ordered = self.nodes.iter().map(|node| node.key).collect::<Vec<_>>();
        ordered.sort_by_key(|key| {
            let node = &self.nodes[key.0 as usize];
            (
                node.span.enter_sequence,
                std::cmp::Reverse(node.span.leave_sequence),
                node.span.atoms.start,
                std::cmp::Reverse(node.span.atoms.end),
                depths[key.0 as usize],
                key.0,
            )
        });
        let mut stack = Vec::<ProjectionNodeKey>::new();
        for child in ordered {
            if child == root {
                stack.clear();
                stack.push(root);
                continue;
            }
            let child_span = self.nodes[child.0 as usize].span.clone();
            while let Some(&candidate) = stack.last() {
                if strictly_contains(&self.nodes[candidate.0 as usize].span, &child_span)
                    || is_ancestor(&self.nodes, candidate, child)
                {
                    break;
                }
                stack.pop();
            }
            if self.nodes[child.0 as usize].execution_parent.is_none() {
                let Some(&parent) = stack.last() else {
                    return Err(ProjectionPlanError::OrphanedNode(child));
                };
                if self.nodes[parent.0 as usize].span == child_span
                    && !is_ancestor(&self.nodes, parent, child)
                {
                    return Err(ProjectionPlanError::AmbiguousParent {
                        child,
                        first: parent,
                        second: child,
                    });
                }
                if matches!(
                    self.nodes[child.0 as usize].kind,
                    ProjectionNodeKind::Heading { .. }
                ) {
                    self.attach_execution(parent, child)?;
                } else {
                    self.attach_structural(parent, child)?;
                }
            }
            stack.push(child);
        }
        Ok(())
    }

    fn establish_man_definition_topology(
        &mut self,
        report: &NativeExecutionReport,
        projection: &NativeProjection,
        wrapper_nodes: &[Option<ProjectionNodeKey>],
        root: ProjectionNodeKey,
    ) -> Result<(), ProjectionPlanError> {
        #[derive(Clone, Copy)]
        struct ActiveRun {
            node: ProjectionNodeKey,
            item: ProjectionNodeKey,
            flow_epoch: usize,
            label_origin: i32,
            item_body_empty: bool,
            last_leave: u64,
        }

        let mut blocks = projection.man_blocks.iter().collect::<Vec<_>>();
        blocks.sort_by_key(|block| report.wrappers()[block.wrapper as usize].enter_sequence);
        let label_origins = projection
            .definitions
            .iter()
            .map(|definition| (definition.owner, definition.responsive.label_origin_columns))
            .collect::<BTreeMap<_, _>>();
        let mut heading_enters = report
            .wrappers()
            .iter()
            .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::Heading)
            .map(|wrapper| wrapper.enter_sequence)
            .collect::<Vec<_>>();
        heading_enters.sort_unstable();
        // One native formatter context can remain suspended while an `.RS`
        // child context executes.  Keep the open declaration run by its
        // structural parent instead of using one document-global cursor.
        // This mirrors the fixed-CVS `mtermp` offset stack: `pre_RS()` enters
        // a new origin and `post_RS()` restores the surrounding context.
        let mut active_by_parent = BTreeMap::<ProjectionNodeKey, ActiveRun>::new();
        for block in blocks {
            let wrapper = &report.wrappers()[block.wrapper as usize];
            let owner = wrapper_nodes[block.wrapper as usize];
            let parent = if let Some(owner) = owner {
                self.nodes[owner.0 as usize]
                    .content_parent
                    .ok_or(ProjectionPlanError::OrphanedNode(owner))?
            } else {
                // PP/P/LP are native boundary lifecycles rather than content
                // containers.  They still terminate a declaration run in the
                // nearest real structural context.
                nearest_structural_parent(report, wrapper_nodes, wrapper.parent).unwrap_or(root)
            };
            active_by_parent.retain(|_, current| {
                let index =
                    heading_enters.partition_point(|sequence| *sequence < current.last_leave);
                !heading_enters
                    .get(index)
                    .is_some_and(|sequence| *sequence < wrapper.enter_sequence)
            });
            if !matches!(
                block.kind,
                ExecutionManBlockKind::IndentedParagraph
                    | ExecutionManBlockKind::TaggedParagraph
                    | ExecutionManBlockKind::AdditionalTag
            ) {
                if block.kind == ExecutionManBlockKind::RelativeIndent {
                    let owner = owner.ok_or(ProjectionPlanError::MissingNativeParent {
                        kind: "man-relative-indent-owner",
                        key: block.owner.0,
                    })?;
                    if let Some(current) = active_by_parent.get_mut(&parent) {
                        // `pre_RS()` first settles the surrounding formatter
                        // line and then changes only the offset stack.  The
                        // scope is consequently a structured continuation of
                        // the open definition item, not a new peer paragraph.
                        // Retain the scope as a child so its own origin and
                        // nested structure stay intact.
                        self.reparent_content(owner, current.item, ChildRole::ManBodyContinuation)?;
                        let child = self.nodes[owner.0 as usize].span.clone();
                        extend_span(&mut self.nodes[current.item.0 as usize].span, &child);
                        extend_span(&mut self.nodes[current.node.0 as usize].span, &child);
                        current.last_leave = wrapper.leave_sequence;
                    } else if let Some(predecessor) = relative_indent_predecessor(report, block) {
                        self.establish_man_hanging_pair(
                            report,
                            parent,
                            owner,
                            wrapper,
                            block.flow_epoch,
                            predecessor,
                        )?;
                    }
                } else {
                    active_by_parent.remove(&parent);
                }
                continue;
            }
            let owner = owner.ok_or(ProjectionPlanError::MissingNativeParent {
                kind: "man-definition-owner",
                key: block.owner.0,
            })?;
            let label_origin = label_origins.get(&block.owner).copied().ok_or(
                ProjectionPlanError::MissingNativeParent {
                    kind: "man-definition-layout",
                    key: block.owner.0,
                },
            )?;
            if block.kind == ExecutionManBlockKind::AdditionalTag {
                if let Some(current) = active_by_parent.get(&parent).copied().filter(|current| {
                    current.flow_epoch == block.flow_epoch
                        && current.label_origin == label_origin
                        && current.item_body_empty
                }) {
                    self.reparent_content(owner, current.item, ChildRole::ManContinuation)?;
                    let child = self.nodes[owner.0 as usize].span.clone();
                    extend_span(&mut self.nodes[current.node.0 as usize].span, &child);
                    active_by_parent.insert(
                        parent,
                        ActiveRun {
                            last_leave: wrapper.leave_sequence,
                            item_body_empty: man_body_is_empty(block),
                            ..current
                        },
                    );
                    continue;
                }
                // Fixed CVS renders a TQ through the TP handler even when no
                // compatible open item precedes it.  Preserve that standalone
                // lifecycle as a typed item instead of rejecting the page or
                // attaching it to an unrelated prior declaration.
                self.nodes[owner.0 as usize].kind = ProjectionNodeKind::ManDefinitionItem {
                    primary: block.owner,
                    kind: block.kind,
                };
            }

            let run = if let Some(current) =
                active_by_parent.get(&parent).copied().filter(|current| {
                    current.flow_epoch == block.flow_epoch && current.label_origin == label_origin
                }) {
                current.node
            } else {
                let run = self.add_node(
                    ProjectionNodeKind::ManDefinitionRun {
                        flow_epoch: block.flow_epoch,
                    },
                    None,
                    ExecutionSpan {
                        atoms: wrapper.enter_atom..wrapper.leave_atom,
                        enter_sequence: wrapper.enter_sequence,
                        leave_sequence: wrapper.leave_sequence,
                    },
                )?;
                self.nodes[run.0 as usize].execution_parent = Some(parent);
                self.nodes[run.0 as usize].content_parent = Some(parent);
                let order = ProjectionOrder {
                    sequence: wrapper.enter_sequence,
                    phase: ProjectionPhase::Content,
                    stable: run.0,
                };
                self.nodes[parent.0 as usize]
                    .pieces
                    .push(ProjectionPiece::Child {
                        node: run,
                        role: ChildRole::Structural,
                        destination: ProjectionDestination::Deferred,
                        order,
                    });
                run
            };
            self.reparent_content(owner, run, ChildRole::ManDefinitionItem)?;
            let child = self.nodes[owner.0 as usize].span.clone();
            extend_span(&mut self.nodes[run.0 as usize].span, &child);
            active_by_parent.insert(
                parent,
                ActiveRun {
                    node: run,
                    item: owner,
                    flow_epoch: block.flow_epoch,
                    label_origin,
                    item_body_empty: man_body_is_empty(block),
                    last_leave: wrapper.leave_sequence,
                },
            );
        }
        Ok(())
    }

    fn establish_man_hanging_pair(
        &mut self,
        report: &NativeExecutionReport,
        parent: ProjectionNodeKey,
        scope_node: ProjectionNodeKey,
        scope_wrapper: &libmandoc_rs::ExecutionWrapper,
        flow_epoch: usize,
        predecessor: ManHangingPredecessor,
    ) -> Result<(), ProjectionPlanError> {
        let first = &report.atoms()[predecessor.atoms.start as usize];
        let last = &report.atoms()[predecessor.atoms.end as usize - 1];
        let term_origin_columns = predecessor
            .origin_columns
            .saturating_sub(i32::try_from(report.content_indent_columns()).unwrap_or(i32::MAX));
        let pair_span = ExecutionSpan {
            atoms: predecessor.atoms.start..scope_wrapper.leave_atom,
            enter_sequence: first.sequence.min(scope_wrapper.enter_sequence),
            leave_sequence: last
                .sequence
                .saturating_add(1)
                .max(scope_wrapper.leave_sequence),
        };
        let run = self.add_node(
            ProjectionNodeKind::ManDefinitionRun { flow_epoch },
            None,
            pair_span.clone(),
        )?;
        self.attach_structural(parent, run)?;
        let pair = self.add_node(
            ProjectionNodeKind::ManHangingPair {
                term: predecessor.term,
                scope: self.nodes[scope_node.0 as usize].source_owner.ok_or(
                    ProjectionPlanError::MissingNativeParent {
                        kind: "man-hanging-scope",
                        key: scope_node.0,
                    },
                )?,
                entry_boundary: predecessor.entry_boundary,
                term_origin_columns,
            },
            None,
            pair_span,
        )?;
        self.attach_structural(run, pair)?;
        let head = self.add_node(
            ProjectionNodeKind::DefinitionSegment {
                kind: DefinitionSegmentKind::ManHead,
            },
            Some(predecessor.term),
            ExecutionSpan {
                atoms: predecessor.atoms,
                enter_sequence: first.sequence,
                leave_sequence: last.sequence.saturating_add(1),
            },
        )?;
        self.attach_structural(pair, head)?;
        self.reparent_content(scope_node, pair, ChildRole::ManBody)?;
        Ok(())
    }

    fn reparent_content(
        &mut self,
        child: ProjectionNodeKey,
        parent: ProjectionNodeKey,
        role: ChildRole,
    ) -> Result<(), ProjectionPlanError> {
        let previous = self.nodes[child.0 as usize]
            .content_parent
            .ok_or(ProjectionPlanError::OrphanedNode(child))?;
        let previous_pieces = &mut self.nodes[previous.0 as usize].pieces;
        let Some(index) = previous_pieces.iter().position(
            |piece| matches!(piece, ProjectionPiece::Child { node, .. } if *node == child),
        ) else {
            return Err(ProjectionPlanError::ChildPieceMismatch(child));
        };
        previous_pieces.remove(index);
        self.nodes[child.0 as usize].content_parent = Some(parent);
        let order = ProjectionOrder {
            sequence: self.nodes[child.0 as usize].span.enter_sequence,
            phase: ProjectionPhase::Content,
            stable: child.0,
        };
        self.nodes[parent.0 as usize]
            .pieces
            .push(ProjectionPiece::Child {
                node: child,
                role,
                destination: ProjectionDestination::Deferred,
                order,
            });
        Ok(())
    }
}

fn extend_span(span: &mut ExecutionSpan, child: &ExecutionSpan) {
    span.atoms.start = span.atoms.start.min(child.atoms.start);
    span.atoms.end = span.atoms.end.max(child.atoms.end);
    span.enter_sequence = span.enter_sequence.min(child.enter_sequence);
    span.leave_sequence = span.leave_sequence.max(child.leave_sequence);
}

fn man_body_is_empty(block: &super::NativeManBlock) -> bool {
    block.body.runs.is_empty()
        && block.body.anchors.is_empty()
        && !block
            .body
            .boundaries
            .iter()
            .any(|boundary| boundary.effect == libmandoc_rs::BoundaryEffect::AddedVerticalSpace)
}

#[derive(Clone, Debug)]
struct ManHangingPredecessor {
    term: ExecutionNodeKey,
    atoms: Range<u32>,
    entry_boundary: u32,
    origin_columns: i32,
}

/// Return the exact field fixed CVS settles in `man_term.c::pre_RS()`.
///
/// The native `term_newln()` boundary is authoritative.  Buffer generation
/// and scanned slots prove which atoms occupied that field; neither source
/// coordinates nor final indentation participate in ownership.
fn relative_indent_predecessor(
    report: &NativeExecutionReport,
    native: &super::NativeManBlock,
) -> Option<ManHangingPredecessor> {
    let boundary = native.boundaries.iter().find(|boundary| {
        boundary.node == Some(native.owner)
            && boundary.parent.is_none()
            && boundary.request == BoundaryRequest::Newline
    })?;
    let flushes = report
        .flushes()
        .iter()
        .filter(|flush| flush.boundary == Some(boundary.key))
        .collect::<Vec<_>>();
    if flushes.is_empty() {
        return None;
    }
    let origin_columns = flushes.iter().find_map(|flush| {
        (flush.cell_bu > 0)
            .then(|| i32::try_from(flush.offset_bu / flush.cell_bu).unwrap_or(i32::MAX))
    })?;
    let belongs = |atom: &libmandoc_rs::ExecutionAtom| {
        let (Some(generation), Some(slot)) = (atom.buffer_generation, atom.slot) else {
            return false;
        };
        flushes
            .iter()
            .any(|flush| flush.buffer_generation == generation && flush.scanned.contains(&slot))
    };
    let mut matching = report
        .atoms()
        .iter()
        .enumerate()
        .filter(|(_, atom)| belongs(atom));
    let (start, first) = matching.next()?;
    let mut end = start + 1;
    let mut term = first.node.filter(|node| *node != native.owner);
    for (index, atom) in matching {
        end = index + 1;
        term = term.or_else(|| atom.node.filter(|node| *node != native.owner));
    }
    if report.atoms()[start..end].iter().any(|atom| !belongs(atom)) {
        return None;
    }
    Some(ManHangingPredecessor {
        term: term?,
        atoms: u32::try_from(start).ok()?..u32::try_from(end).ok()?,
        entry_boundary: boundary.key,
        origin_columns,
    })
}

fn coalesce_streams(pieces: &mut Vec<ProjectionPiece>) {
    let mut coalesced = Vec::with_capacity(pieces.len());
    for piece in std::mem::take(pieces) {
        match piece {
            ProjectionPiece::Stream {
                mut events,
                destination,
                order,
            } => {
                if let Some(ProjectionPiece::Stream {
                    events: previous,
                    destination: previous_destination,
                    ..
                }) = coalesced.last_mut()
                    && *previous_destination == destination
                {
                    previous.append(&mut events);
                } else {
                    coalesced.push(ProjectionPiece::Stream {
                        events,
                        destination,
                        order,
                    });
                }
            }
            other => coalesced.push(other),
        }
    }
    *pieces = coalesced;
}

fn validate_projection_cardinality(
    kind: &'static str,
    report: usize,
    projection: usize,
) -> Result<(), ProjectionPlanError> {
    if report == projection {
        Ok(())
    } else {
        Err(ProjectionPlanError::ProjectionCardinalityMismatch {
            kind,
            report,
            projection,
        })
    }
}

const fn specialized_recipe(kind: ProjectionNodeKind) -> Option<SpecializedRecipe> {
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
        #[cfg(test)]
        ProjectionNodeKind::Flow => None,
        ProjectionNodeKind::Root
        | ProjectionNodeKind::Heading { .. }
        | ProjectionNodeKind::NativeScope { .. }
        | ProjectionNodeKind::MdocItem { .. }
        | ProjectionNodeKind::DefinitionSegment { .. }
        | ProjectionNodeKind::MdocList { .. }
        | ProjectionNodeKind::ManDefinitionRun { .. }
        | ProjectionNodeKind::ManDefinitionItem { .. }
        | ProjectionNodeKind::ManHangingPair { .. }
        | ProjectionNodeKind::ManDefinitionContinuation { .. } => None,
    }
}

/// Partially constructed forest retaining stable mappings to native records.
pub(super) struct NativeArenaSeed {
    builder: ProjectionArenaBuilder,
    wrapper_nodes: Vec<Option<ProjectionNodeKey>>,
    node_wrappers: Vec<Option<usize>>,
    atom_owners: Vec<Option<ProjectionNodeKey>>,
    specialized_atom_owners: Vec<Option<ProjectionNodeKey>>,
    specialized_nodes: Vec<ProjectionNodeKey>,
    effect_ledger: EffectLedger,
}

impl NativeArenaSeed {
    /// Seal report-derived direct ownership without exposing a range-claiming
    /// API to projection consumers.  Section placement remains a later,
    /// deterministic location-map operation and cannot change this forest.
    #[allow(clippy::too_many_lines)]
    fn seal(
        mut self,
        report: &NativeExecutionReport,
        sections: &SectionIndex,
    ) -> Result<ProjectionArena, ProjectionPlanError> {
        let profile = std::env::var_os("MANT_NATIVE_PROFILE").is_some();
        let mut phase = std::time::Instant::now();
        let mark = |name: &str, phase: &mut std::time::Instant| {
            if profile {
                eprintln!("native-profile ownership-{name} {:?}", phase.elapsed());
            }
            *phase = std::time::Instant::now();
        };
        let mut specialized_anchor_owners = BTreeMap::<u32, ProjectionNodeKey>::new();
        for (owner, anchors) in &self.builder.specialized_anchor_consumers {
            for anchor in anchors {
                if specialized_anchor_owners.insert(*anchor, *owner).is_some() {
                    return Err(ProjectionPlanError::RecordAbsorbedTwice(
                        RawRecordRef::Anchor(*anchor),
                    ));
                }
            }
        }
        let mut direct_atoms = BTreeMap::<ProjectionNodeKey, Vec<u32>>::new();
        // A content fragment is the device-side consequence of its origin
        // atom.  `term_flushln()` may emit that fragment after the structural
        // wrapper that caused the atom has left, so sequence containment is
        // not an ownership relation.  Keep the causal atom owner while the
        // arena is still being sealed; the public ownership ledger is only
        // populated later when the atom runs are claimed.
        let mut direct_atom_owners = vec![None; report.atoms().len()];
        for atom in report.atoms() {
            let index = atom.key.0;
            if let Some(reason) = output_role_omission(atom.output_role) {
                self.builder.omit_atom_range(index..index + 1, reason)?;
                continue;
            }
            if let Some(reason) = atom_omission(atom.disposition, index)? {
                self.builder.omit_atom_range(index..index + 1, reason)?;
                continue;
            }
            let owner = owner_with_native_evidence(
                &self.builder.nodes,
                report,
                &self.wrapper_nodes,
                atom.wrapper,
                self.specialized_atom_owners[index as usize],
                self.atom_owners[index as usize],
                &self.specialized_nodes,
                atom.sequence,
                Some(index),
                RecordKind::Atom,
                index,
            )?;
            direct_atom_owners[index as usize] = Some(owner);
            direct_atoms.entry(owner).or_default().push(index);
        }
        mark("atoms", &mut phase);

        let mut direct_fragments = BTreeMap::<ProjectionNodeKey, Vec<u32>>::new();
        for fragment in report.fragments() {
            let index = fragment.key.0;
            if let Some(reason) = fragment_omission(fragment.role) {
                self.builder.omit_fragment(index, reason)?;
                continue;
            }
            match direct_atom_owners[fragment.atom.0 as usize] {
                Some(owner) => {
                    direct_fragments.entry(owner).or_default().push(index);
                }
                None => {
                    return Err(ProjectionPlanError::NoOwnerForRecord {
                        kind: RecordKind::Fragment,
                        index,
                        sequence: fragment.sequence,
                    });
                }
            }
        }

        let mut direct_geometry = BTreeMap::<ProjectionNodeKey, Vec<u32>>::new();
        for &key in &self.effect_ledger.flow_geometry {
            let record = &report.geometry()[key as usize];
            if let Some(reason) = output_role_omission(record.output_role) {
                self.builder.omit_geometry(key, reason)?;
                continue;
            }
            let origin_atom = geometry_origin_atom(report, key);
            match origin_atom.and_then(|atom| direct_atom_owners[atom as usize]) {
                Some(owner) => {
                    direct_geometry.entry(owner).or_default().push(key);
                }
                None => {
                    if let Some(RecordDisposition::Omitted(reason)) = origin_atom
                        .and_then(|atom| self.builder.ownership.atoms.get(atom as usize))
                        .copied()
                    {
                        self.builder.omit_geometry(key, reason)?;
                        continue;
                    }
                    return Err(ProjectionPlanError::NoOwnerForRecord {
                        kind: RecordKind::Geometry,
                        index: key,
                        sequence: record.sequence,
                    });
                }
            }
        }

        for plan in &self.effect_ledger.effects {
            if let Some(reason) = output_role_omission(plan.output_role) {
                self.builder.omit_effect_plan(plan, reason)?;
                continue;
            }
            let wrapper = effect_wrapper(report, plan).or_else(|| {
                effect_source_node(report, plan).and_then(|node| {
                    self.node_wrappers
                        .get(node.0 as usize)
                        .and_then(|wrapper| *wrapper)
                        .and_then(|wrapper| u32::try_from(wrapper).ok())
                })
            });
            let owner = owner_with_native_evidence(
                &self.builder.nodes,
                report,
                &self.wrapper_nodes,
                wrapper,
                None,
                None,
                &self.specialized_nodes,
                plan.sequence,
                None,
                RecordKind::Boundary,
                plan.key.0,
            )?;
            let mut owned_effect = plan.clone();
            if let CanonicalEffectOutcome::Trace(steps) = &mut owned_effect.outcome {
                let definition_field = matches!(
                    self.builder.nodes[owner.0 as usize].kind,
                    ProjectionNodeKind::MdocItem { .. }
                        | ProjectionNodeKind::ManDefinitionItem { .. }
                        | ProjectionNodeKind::ManDefinitionContinuation { .. }
                        | ProjectionNodeKind::DefinitionSegment { .. }
                );
                let hanging_entry = self.builder.nodes[owner.0 as usize]
                    .content_parent
                    .and_then(|parent| match self.builder.nodes[parent.0 as usize].kind {
                        ProjectionNodeKind::ManHangingPair { entry_boundary, .. } => {
                            Some(entry_boundary)
                        }
                        _ => None,
                    })
                    .is_some_and(|entry_boundary| {
                        owned_effect
                            .records
                            .contains(&RawRecordRef::Boundary(entry_boundary))
                    });
                for step in steps {
                    match step.kind {
                        CanonicalEffectKind::FieldEnd { empty } => {
                            step.kind = if definition_field || hanging_entry {
                                CanonicalEffectKind::FieldSettlement
                            } else {
                                CanonicalEffectKind::LogicalLineCommit { empty }
                            };
                        }
                        CanonicalEffectKind::LogicalLineCommit { .. } if hanging_entry => {
                            step.kind = CanonicalEffectKind::FieldSettlement;
                        }
                        _ => {}
                    }
                }
            }
            // Fixed CVS mdoc_term.c::termp_it_pre() calls print_bvspace()
            // while executing the It BLOCK, before entering HEAD/BODY.  That
            // spacing is the presentation of `Bl.comp`, not inline content of
            // the item and not an authored `.sp`.  Normalize it while both
            // the causal effect and its exact structural owner are available;
            // the emitter must not rediscover this distinction from context.
            let has_blank_device_line = match &owned_effect.outcome {
                CanonicalEffectOutcome::Trace(steps) => steps
                    .iter()
                    .any(|step| step.kind == CanonicalEffectKind::BlankDeviceLine),
                CanonicalEffectOutcome::StateOnly | CanonicalEffectOutcome::StructuralSpacing => {
                    false
                }
            };
            let structural_vertical = has_blank_device_line
                && (matches!(
                    self.builder.nodes[owner.0 as usize].kind,
                    ProjectionNodeKind::MdocItem { compact: false, .. }
                ) || effect_source_node(report, &owned_effect).is_some_and(|node| {
                    report.nodes()[node.0 as usize]
                        .macro_name
                        .as_deref()
                        .is_some_and(|name| matches!(name, "Sh" | "Ss" | "SH" | "SS"))
                }) || matches!(
                    self.builder.nodes[owner.0 as usize].kind,
                    ProjectionNodeKind::ManDefinitionItem {
                        kind: ExecutionManBlockKind::IndentedParagraph
                            | ExecutionManBlockKind::TaggedParagraph,
                        ..
                    }
                ));
            if structural_vertical {
                if let CanonicalEffectOutcome::Trace(steps) = &mut owned_effect.outcome {
                    steps.retain(|step| step.kind != CanonicalEffectKind::BlankDeviceLine);
                    if steps.is_empty() {
                        owned_effect.outcome = CanonicalEffectOutcome::StructuralSpacing;
                    }
                } else {
                    owned_effect.outcome = CanonicalEffectOutcome::StructuralSpacing;
                }
            }
            self.builder.claim_effect_plan(owner, &owned_effect)?;
        }
        mark("effects", &mut phase);

        // A control is the typed execution action; its boundary, flush, and
        // geometry consequences are separate causal records already owned by
        // the canonical effect ledger.  Keep the action itself in total
        // execution order so even zero-output controls have one owner and one
        // final consumer without replaying their effects.
        for control in report.controls() {
            let owner = owner_with_native_evidence(
                &self.builder.nodes,
                report,
                &self.wrapper_nodes,
                Some(control.wrapper),
                None,
                None,
                &self.specialized_nodes,
                control.enter_sequence,
                None,
                RecordKind::Control,
                control.key,
            )?;
            self.builder.claim_control(
                owner,
                control.key,
                ProjectionOrder {
                    sequence: control.enter_sequence,
                    phase: ProjectionPhase::Content,
                    stable: control.key,
                },
            )?;
        }

        for placement in report.placements() {
            let owner = owner_with_native_evidence(
                &self.builder.nodes,
                report,
                &self.wrapper_nodes,
                Some(placement.wrapper),
                None,
                None,
                &self.specialized_nodes,
                placement.sequence,
                None,
                RecordKind::Placement,
                placement.key,
            )?;
            self.builder.claim_placement(
                owner,
                placement.key,
                ProjectionOrder {
                    sequence: placement.sequence,
                    phase: ProjectionPhase::Content,
                    stable: placement.key,
                },
            )?;
        }

        for anchor in report.anchors() {
            let wrapper = self
                .node_wrappers
                .get(anchor.node.0 as usize)
                .and_then(|value| *value)
                .map(|value| {
                    u32::try_from(value).map_err(|_| ProjectionPlanError::NodeCapacityExceeded)
                })
                .transpose()?;
            let owner = if let Some(owner) = specialized_anchor_owners.get(&anchor.key) {
                *owner
            } else {
                owner_with_native_evidence(
                    &self.builder.nodes,
                    report,
                    &self.wrapper_nodes,
                    wrapper,
                    None,
                    None,
                    &self.specialized_nodes,
                    anchor.sequence,
                    None,
                    RecordKind::Anchor,
                    anchor.key,
                )?
            };
            // A table-leading target can execute immediately before term_tbl
            // enters its wrapper.  The typed table is nevertheless its sole
            // final consumer.  Place the recipe-local piece at the table's
            // BEFORE edge while preserving the raw sequence in the ledger.
            let sequence = if specialized_anchor_owners.contains_key(&anchor.key) {
                self.builder.nodes[owner.0 as usize].span.enter_sequence
            } else {
                anchor.sequence
            };
            self.builder.claim_anchor(
                owner,
                anchor.key,
                ProjectionOrder {
                    sequence,
                    phase: ProjectionPhase::Before,
                    stable: anchor.key,
                },
            )?;
        }
        mark("zero-width", &mut phase);

        for (owner, mut atoms) in direct_atoms {
            atoms.sort_unstable();
            let fragments = direct_fragments.remove(&owner).unwrap_or_default();
            let geometry = direct_geometry.remove(&owner).unwrap_or_default();
            let mut fragments_by_atom = BTreeMap::<u32, Vec<u32>>::new();
            for key in fragments {
                fragments_by_atom
                    .entry(report.fragments()[key as usize].atom.0)
                    .or_default()
                    .push(key);
            }
            let mut geometry_by_atom = BTreeMap::<u32, Vec<u32>>::new();
            for key in geometry {
                if let Some(atom) = geometry_origin_atom(report, key) {
                    geometry_by_atom.entry(atom).or_default().push(key);
                }
            }
            for run in direct_execution_runs(
                &atoms,
                &self.builder.nodes[owner.0 as usize],
                report,
                sections,
            ) {
                let run_fragments = fragments_by_atom
                    .range(run.clone())
                    .flat_map(|(_, keys)| keys.iter().copied())
                    .collect::<Vec<_>>();
                let run_geometry = geometry_by_atom
                    .range(run.clone())
                    .flat_map(|(_, keys)| keys.iter().copied())
                    .collect::<Vec<_>>();
                let sequence = report.atoms()[run.start as usize].sequence;
                self.builder.claim_flow_records(
                    owner,
                    run.clone(),
                    run_fragments,
                    run_geometry,
                    ProjectionOrder {
                        sequence,
                        phase: ProjectionPhase::Content,
                        stable: run.start,
                    },
                )?;
            }
        }
        mark("flows", &mut phase);
        if let Some((&owner, records)) = direct_fragments.first_key_value() {
            let index = records[0];
            return Err(ProjectionPlanError::OwnedRecordUnexplained {
                kind: RecordKind::Fragment,
                index,
                owner,
            });
        }
        if let Some((&owner, records)) = direct_geometry.first_key_value() {
            let index = records[0];
            return Err(ProjectionPlanError::OwnedRecordUnexplained {
                kind: RecordKind::Geometry,
                index,
                owner,
            });
        }

        self.builder.collapse_specialized_pieces();
        self.builder.absorb_specialized_descendants()?;
        self.builder.absorb_heading_descendants()?;
        mark("absorb", &mut phase);
        bind_node_destinations(&mut self.builder.nodes, sections)?;
        mark("destinations", &mut phase);
        let arena = self.builder.finish()?;
        mark("finish", &mut phase);
        Ok(arena)
    }
}

/// Build and seal the unique structural projection plan for one native report.
pub(super) fn plan_native(
    report: &NativeExecutionReport,
    projection: &NativeProjection,
    sections: &SectionIndex,
) -> Result<ProjectionArena, ProjectionPlanError> {
    let started = std::time::Instant::now();
    let seed = ProjectionArenaBuilder::from_native(report, projection)?;
    if std::env::var_os("MANT_NATIVE_PROFILE").is_some() {
        eprintln!("native-profile ownership-build {:?}", started.elapsed());
    }
    seed.seal(report, sections)
}

fn bind_node_destinations(
    nodes: &mut [ProjectionNode],
    sections: &SectionIndex,
) -> Result<(), ProjectionPlanError> {
    let destinations = nodes
        .iter()
        .map(|node| {
            Ok(match node.kind {
                ProjectionNodeKind::Root => ProjectionDestination::Root,
                ProjectionNodeKind::Heading { wrapper, .. } => ProjectionDestination::Section(
                    sections
                        .for_heading(wrapper)
                        .ok_or(ProjectionPlanError::MissingHeadingDestination { wrapper })?,
                ),
                _ => sections.destination_for_sequence(node.span.enter_sequence),
            })
        })
        .collect::<Result<Vec<_>, ProjectionPlanError>>()?;
    for (index, node) in nodes.iter_mut().enumerate() {
        node.destination = destinations[index];
        if !matches!(
            node.kind,
            ProjectionNodeKind::Root | ProjectionNodeKind::Heading { .. }
        ) {
            let mut probes = vec![node.span.enter_sequence];
            if node.span.leave_sequence > node.span.enter_sequence {
                probes.push(node.span.leave_sequence - 1);
            }
            for cut in sections.cuts_inside(node.span.enter_sequence, node.span.leave_sequence) {
                probes.push(cut);
                probes.push(cut - 1);
            }
            probes.extend(node.pieces.iter().map(|piece| piece.order().sequence));
            if probes
                .into_iter()
                .any(|sequence| sections.destination_for_sequence(sequence) != node.destination)
            {
                return Err(ProjectionPlanError::StructuralOwnerCrossesDestination {
                    node: node.key,
                    kind: node.kind,
                    source_owner: node.source_owner,
                    span: node.span.clone(),
                });
            }
        }
        for piece in &mut node.pieces {
            let destination = match piece {
                ProjectionPiece::Child { node: child, .. } => destinations[child.0 as usize],
                ProjectionPiece::Stream { order, .. }
                | ProjectionPiece::Specialized { order, .. } => {
                    sections.destination_for_sequence(order.sequence)
                }
            };
            piece.set_destination(destination);
        }
    }
    Ok(())
}

fn native_scope_kind(wrapper: &libmandoc_rs::ExecutionWrapper) -> Option<ProjectionScopeKind> {
    match wrapper.kind {
        ExecutionWrapperKind::Heading | ExecutionWrapperKind::Node | ExecutionWrapperKind::Font => {
            None
        }
        ExecutionWrapperKind::MdocListItem => wrapper
            .mdoc_list_kind
            .map(ProjectionScopeKind::MdocListItem),
        ExecutionWrapperKind::ManBlock => wrapper.man_block_kind.and_then(|kind| {
            // Fixed CVS `pre_PP()` executes PP/P/LP as paragraph-boundary and
            // origin transitions.  Their syntax BODY can envelop later block
            // nodes, but `print_man_node()` does not make that envelope an
            // independently rendered content container.  Keeping it as an
            // owner would overlap a definition run whose formatter lifecycle
            // legitimately continues past the paragraph wrapper.  The
            // boundary and placement records already carry the complete
            // paragraph effect, so only true structural man scopes remain
            // projection owners here.
            (!matches!(
                kind,
                ExecutionManBlockKind::Paragraph
                    | ExecutionManBlockKind::ParagraphP
                    | ExecutionManBlockKind::ParagraphLp
            ))
            .then_some(ProjectionScopeKind::ManBlock(kind))
        }),
        ExecutionWrapperKind::Region => {
            wrapper
                .region_kind
                .and_then(|kind| match region_policy(kind) {
                    RegionPolicy::Container => Some(ProjectionScopeKind::Region(kind)),
                    RegionPolicy::StateTransition => None,
                })
        }
    }
}

fn child_role(kind: ProjectionNodeKind) -> ChildRole {
    match kind {
        ProjectionNodeKind::MdocItem { .. } => ChildRole::MdocItem,
        ProjectionNodeKind::DefinitionSegment {
            kind: DefinitionSegmentKind::MdocHead,
        } => ChildRole::MdocHead,
        ProjectionNodeKind::DefinitionSegment {
            kind: DefinitionSegmentKind::MdocBody { ordinal },
        } => ChildRole::MdocBody { ordinal },
        ProjectionNodeKind::DefinitionSegment {
            kind: DefinitionSegmentKind::ManHead,
        } => ChildRole::ManHead,
        ProjectionNodeKind::DefinitionSegment {
            kind: DefinitionSegmentKind::ManBody,
        } => ChildRole::ManBody,
        ProjectionNodeKind::ManDefinitionItem { .. } => ChildRole::ManDefinitionItem,
        ProjectionNodeKind::ManHangingPair { .. } => ChildRole::ManDefinitionItem,
        ProjectionNodeKind::ManDefinitionContinuation { .. } => ChildRole::ManContinuation,
        #[cfg(test)]
        ProjectionNodeKind::Flow => ChildRole::Structural,
        ProjectionNodeKind::Root
        | ProjectionNodeKind::Heading { .. }
        | ProjectionNodeKind::NativeScope { .. }
        | ProjectionNodeKind::MdocList { .. }
        | ProjectionNodeKind::ManDefinitionRun { .. }
        | ProjectionNodeKind::Table(_)
        | ProjectionNodeKind::TableRow(_)
        | ProjectionNodeKind::TableCellInvocation(_)
        | ProjectionNodeKind::Equation(_)
        | ProjectionNodeKind::EquationInvocation(_) => ChildRole::Structural,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegionPolicy {
    Container,
    StateTransition,
}

const fn region_policy(kind: ExecutionRegionKind) -> RegionPolicy {
    match kind {
        ExecutionRegionKind::ManSynopsisCommand
        | ExecutionRegionKind::MdocSynopsisItem
        | ExecutionRegionKind::MdocDisplayFilled
        | ExecutionRegionKind::MdocDisplayUnfilled
        | ExecutionRegionKind::MdocDisplayLiteral
        | ExecutionRegionKind::MdocDisplayRagged
        | ExecutionRegionKind::MdocDisplayCentered
        | ExecutionRegionKind::MdocDisplayOneLine
        | ExecutionRegionKind::MdocDisplayOneLineLiteral
        | ExecutionRegionKind::CenteredLines
        | ExecutionRegionKind::RightJustifiedLines => RegionPolicy::Container,
        ExecutionRegionKind::ManSynopsisSection
        | ExecutionRegionKind::MdocSynopsisSection
        | ExecutionRegionKind::ManLiteralBegin
        | ExecutionRegionKind::ManLiteralEnd => RegionPolicy::StateTransition,
    }
}

const fn atom_omission(
    disposition: AtomDisposition,
    index: u32,
) -> Result<Option<OmissionReason>, ProjectionPlanError> {
    Ok(match disposition {
        AtomDisposition::Emitted => None,
        // `execution_report_seal()` resolves every pending cell before the C
        // report crosses the FFI boundary. Seeing Buffered here is therefore
        // a producer-contract violation, never an omission policy.
        AtomDisposition::Buffered => return Err(ProjectionPlanError::BufferedAtomAtSeal(index)),
        AtomDisposition::Consumed => Some(OmissionReason::ConsumedAtom),
        AtomDisposition::Replaced => Some(OmissionReason::ReplacedAtom),
        AtomDisposition::TrailingDiscard => Some(OmissionReason::TrailingDiscard),
    })
}

const fn output_role_omission(role: ExecutionOutputRole) -> Option<OmissionReason> {
    match role {
        ExecutionOutputRole::Content => None,
        ExecutionOutputRole::MarginDecoration => Some(OmissionReason::MarginDecoration),
        ExecutionOutputRole::PageDecoration => Some(OmissionReason::DeviceDecoration),
    }
}

const fn fragment_omission(role: FragmentRole) -> Option<OmissionReason> {
    match role {
        FragmentRole::Content => None,
        FragmentRole::FontDecoration | FragmentRole::PageDecoration => {
            Some(OmissionReason::DeviceDecoration)
        }
        FragmentRole::MarginDecoration => Some(OmissionReason::MarginDecoration),
    }
}

fn nearest_structural_parent(
    report: &NativeExecutionReport,
    wrapper_nodes: &[Option<ProjectionNodeKey>],
    mut wrapper: Option<u32>,
) -> Option<ProjectionNodeKey> {
    while let Some(key) = wrapper {
        if let Some(owner) = wrapper_nodes[key as usize] {
            return Some(owner);
        }
        wrapper = report.wrappers()[key as usize].parent;
    }
    None
}

fn has_table_ancestor(nodes: &[ProjectionNode], mut key: ProjectionNodeKey) -> bool {
    while let Some(parent) = nodes[key.0 as usize].content_parent {
        if matches!(nodes[parent.0 as usize].kind, ProjectionNodeKind::Table(_)) {
            return true;
        }
        key = parent;
    }
    false
}

/// Return the typed root that is solely responsible for projecting this
/// specialized node.  Equations nested in table cells belong to the table's
/// typed model; standalone equations own their own model.
fn top_level_specialized_root(
    nodes: &[ProjectionNode],
    mut key: ProjectionNodeKey,
) -> Option<ProjectionNodeKey> {
    let mut root = specialized_recipe(nodes[key.0 as usize].kind).map(|_| key);
    while let Some(parent) = nodes[key.0 as usize].content_parent {
        if matches!(
            nodes[parent.0 as usize].kind,
            ProjectionNodeKind::Table(_) | ProjectionNodeKind::Equation(_)
        ) {
            root = Some(parent);
        }
        key = parent;
    }
    root
}

fn collect_specialized_descendants(
    nodes: &[ProjectionNode],
    root: ProjectionNodeKey,
    output: &mut Vec<ProjectionNodeKey>,
) -> Result<(), ProjectionPlanError> {
    output.push(root);
    for child in nodes[root.0 as usize]
        .pieces
        .iter()
        .filter_map(|piece| match piece {
            ProjectionPiece::Child { node, .. } => Some(*node),
            _ => None,
        })
    {
        if !matches!(
            nodes[child.0 as usize].kind,
            ProjectionNodeKind::TableRow(_)
                | ProjectionNodeKind::TableCellInvocation(_)
                | ProjectionNodeKind::Equation(_)
                | ProjectionNodeKind::EquationInvocation(_)
        ) {
            return Err(ProjectionPlanError::InvalidSpecializedRecipe(child));
        }
        collect_specialized_descendants(nodes, child, output)?;
    }
    Ok(())
}

fn collect_execution_descendants(
    children: &[Vec<ProjectionNodeKey>],
    root: ProjectionNodeKey,
    output: &mut Vec<ProjectionNodeKey>,
) {
    output.push(root);
    for child in &children[root.0 as usize] {
        collect_execution_descendants(children, *child, output);
    }
}

fn records_by_owner(
    ownership: &ExecutionOwnership,
) -> Result<BTreeMap<ProjectionNodeKey, Vec<RawRecordRef>>, ProjectionPlanError> {
    let mut by_owner = BTreeMap::<ProjectionNodeKey, Vec<RawRecordRef>>::new();
    for (kind, ledger) in [
        (RecordKind::Atom, ownership.atoms.as_slice()),
        (RecordKind::Fragment, ownership.fragments.as_slice()),
        (RecordKind::Flush, ownership.flushes.as_slice()),
        (RecordKind::Boundary, ownership.boundaries.as_slice()),
        (RecordKind::Control, ownership.controls.as_slice()),
        (RecordKind::Geometry, ownership.geometry.as_slice()),
        (RecordKind::Placement, ownership.placements.as_slice()),
        (RecordKind::Anchor, ownership.anchors.as_slice()),
    ] {
        for (index, disposition) in ledger.iter().enumerate() {
            let RecordDisposition::Owned(owner) = disposition else {
                continue;
            };
            let index =
                u32::try_from(index).map_err(|_| ProjectionPlanError::NodeCapacityExceeded)?;
            let record = match kind {
                RecordKind::Atom => RawRecordRef::Atom(index),
                RecordKind::Fragment => RawRecordRef::Fragment(index),
                RecordKind::Flush => RawRecordRef::Flush(index),
                RecordKind::Boundary => RawRecordRef::Boundary(index),
                RecordKind::Control => RawRecordRef::Control(index),
                RecordKind::Geometry => RawRecordRef::Geometry(index),
                RecordKind::Placement => RawRecordRef::Placement(index),
                RecordKind::Anchor => RawRecordRef::Anchor(index),
            };
            by_owner.entry(*owner).or_default().push(record);
        }
    }
    Ok(by_owner)
}

fn collect_owner_records(
    by_owner: &BTreeMap<ProjectionNodeKey, Vec<RawRecordRef>>,
    owners: &[ProjectionNodeKey],
) -> Vec<RawRecordRef> {
    let mut records = owners
        .iter()
        .filter_map(|owner| by_owner.get(owner))
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    records.sort_unstable();
    records.dedup();
    records
}

fn validate_absorption_receipts(
    nodes: &[ProjectionNode],
    ownership: &ExecutionOwnership,
    specialized_anchor_consumers: &BTreeMap<ProjectionNodeKey, BTreeSet<u32>>,
    specialized: &[SpecializedAbsorption],
    headings: &[HeadingAbsorption],
) -> Result<(), ProjectionPlanError> {
    let expected_specialized = nodes
        .iter()
        .filter(|node| {
            matches!(node.kind, ProjectionNodeKind::Table(_))
                || matches!(node.kind, ProjectionNodeKind::Equation(_))
                    && !has_table_ancestor(nodes, node.key)
        })
        .map(|node| node.key)
        .collect::<BTreeSet<_>>();
    let actual_specialized = specialized
        .iter()
        .map(|receipt| receipt.root)
        .collect::<BTreeSet<_>>();
    if specialized.len() != actual_specialized.len() {
        let mut seen = BTreeSet::new();
        let duplicate = specialized
            .iter()
            .map(|receipt| receipt.root)
            .find(|root| !seen.insert(*root))
            .unwrap_or(ProjectionNodeKey(u32::MAX));
        return Err(ProjectionPlanError::InvalidSpecializedRecipe(duplicate));
    }
    if expected_specialized != actual_specialized {
        return Err(ProjectionPlanError::InvalidSpecializedRecipe(
            expected_specialized
                .symmetric_difference(&actual_specialized)
                .next()
                .copied()
                .unwrap_or(ProjectionNodeKey(u32::MAX)),
        ));
    }

    let expected_headings = nodes
        .iter()
        .filter(|node| matches!(node.kind, ProjectionNodeKind::Heading { .. }))
        .map(|node| node.key)
        .collect::<BTreeSet<_>>();
    let actual_headings = headings
        .iter()
        .map(|receipt| receipt.root)
        .collect::<BTreeSet<_>>();
    if headings.len() != actual_headings.len() {
        let mut seen = BTreeSet::new();
        let duplicate = headings
            .iter()
            .map(|receipt| receipt.root)
            .find(|root| !seen.insert(*root))
            .unwrap_or(ProjectionNodeKey(u32::MAX));
        return Err(ProjectionPlanError::InvalidHeadingReceipt(duplicate));
    }
    if expected_headings != actual_headings {
        return Err(ProjectionPlanError::InvalidHeadingReceipt(
            expected_headings
                .symmetric_difference(&actual_headings)
                .next()
                .copied()
                .unwrap_or(ProjectionNodeKey(u32::MAX)),
        ));
    }

    let mut globally_absorbed = BTreeSet::new();
    let by_owner = records_by_owner(ownership)?;
    for receipt in specialized {
        // A specialized root can retain recipe-external anchors as an
        // ordinary stream.  Only records explicitly transferred into typed
        // specialized pieces belong to the absorption receipt; the final
        // consumer ledger separately proves that retained records are used.
        let mut expected = receipt
            .descendants
            .iter()
            .flat_map(|key| &nodes[key.0 as usize].pieces)
            .filter_map(|piece| match piece {
                ProjectionPiece::Specialized { records, .. } => Some(records.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        expected.sort_unstable();
        expected.dedup();
        if expected != receipt.records {
            return Err(ProjectionPlanError::AbsorptionRecordMismatch(receipt.root));
        }
        let root_records = nodes[receipt.root.0 as usize]
            .pieces
            .iter()
            .find_map(|piece| match piece {
                ProjectionPiece::Specialized { records, .. } => Some(records),
                _ => None,
            })
            .ok_or(ProjectionPlanError::InvalidSpecializedRecipe(receipt.root))?;
        if *root_records != receipt.records {
            return Err(ProjectionPlanError::AbsorptionRecordMismatch(receipt.root));
        }
        let actual_anchors = receipt
            .records
            .iter()
            .filter_map(|record| match record {
                RawRecordRef::Anchor(key) => Some(*key),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let consumable_anchors = specialized_anchor_consumers
            .get(&receipt.root)
            .cloned()
            .unwrap_or_default();
        if actual_anchors != consumable_anchors {
            return Err(ProjectionPlanError::AbsorptionRecordMismatch(receipt.root));
        }
        for record in &receipt.records {
            if !globally_absorbed.insert(*record) {
                return Err(ProjectionPlanError::RecordAbsorbedTwice(*record));
            }
        }
    }
    for receipt in headings {
        if !matches!(
            nodes[receipt.root.0 as usize].kind,
            ProjectionNodeKind::Heading { .. }
        ) {
            return Err(ProjectionPlanError::InvalidHeadingReceipt(receipt.root));
        }
        let expected = collect_owner_records(&by_owner, &receipt.descendants);
        if expected != receipt.records {
            return Err(ProjectionPlanError::AbsorptionRecordMismatch(receipt.root));
        }
        for record in &receipt.records {
            if !globally_absorbed.insert(*record) {
                return Err(ProjectionPlanError::RecordAbsorbedTwice(*record));
            }
        }
    }
    Ok(())
}

fn final_consumer_receipts(
    nodes: &[ProjectionNode],
    ownership: &ExecutionOwnership,
    specialized: &[SpecializedAbsorption],
    headings: &[HeadingAbsorption],
) -> Result<Vec<FinalConsumerReceipt>, ProjectionPlanError> {
    let heading_nodes = headings
        .iter()
        .flat_map(|receipt| receipt.descendants.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut receipts = Vec::new();
    for node in nodes {
        if heading_nodes.contains(&node.key) {
            continue;
        }
        for (piece, recipe) in node.pieces.iter().enumerate() {
            let ProjectionPiece::Stream { events, .. } = recipe else {
                continue;
            };
            let mut records = events
                .iter()
                .flat_map(StreamEvent::records)
                .collect::<Vec<_>>();
            records.sort_unstable();
            records.dedup();
            if !records.is_empty() {
                receipts.push(FinalConsumerReceipt {
                    consumer: FinalConsumer::Stream {
                        owner: node.key,
                        piece: u32::try_from(piece)
                            .map_err(|_| ProjectionPlanError::NodeCapacityExceeded)?,
                    },
                    records,
                });
            }
        }
    }
    receipts.extend(specialized.iter().map(|receipt| FinalConsumerReceipt {
        consumer: FinalConsumer::Specialized(receipt.root),
        records: receipt.records.clone(),
    }));
    receipts.extend(headings.iter().map(|receipt| FinalConsumerReceipt {
        consumer: FinalConsumer::Heading(receipt.root),
        records: receipt.records.clone(),
    }));

    let mut explained = empty_explanation_ledger(ownership);
    for receipt in &receipts {
        for record in &receipt.records {
            let owner = match receipt.consumer {
                FinalConsumer::Stream { owner, .. } => owner,
                FinalConsumer::Specialized(root) | FinalConsumer::Heading(root) => {
                    let (kind, index) = record.parts();
                    match ledger(ownership, kind).get(index as usize) {
                        Some(RecordDisposition::Owned(owner)) => *owner,
                        Some(RecordDisposition::Unassigned | RecordDisposition::Omitted(_))
                        | None => root,
                    }
                }
            };
            register_explanation(&mut explained, *record, owner)?;
        }
    }
    validate_explanations(&explained, ownership)?;
    receipts.sort_by_key(|receipt| final_consumer_sort_key(receipt.consumer));
    Ok(receipts)
}

fn final_consumer_sort_key(consumer: FinalConsumer) -> (u8, u32, u32) {
    match consumer {
        FinalConsumer::Stream { owner, piece } => (0, owner.0, piece),
        FinalConsumer::Specialized(owner) => (1, owner.0, 0),
        FinalConsumer::Heading(owner) => (2, owner.0, 0),
    }
}

fn report_leave_sequence(report: &NativeExecutionReport) -> u64 {
    let sequence = report
        .wrappers()
        .iter()
        .map(|record| record.leave_sequence)
        .chain(report.tables().iter().map(|record| record.leave_sequence))
        .chain(
            report
                .table_rows()
                .iter()
                .map(|record| record.leave_sequence),
        )
        .chain(
            report
                .table_cell_invocations()
                .iter()
                .map(|record| record.leave_sequence),
        )
        .chain(
            report
                .equations()
                .iter()
                .map(|record| record.leave_sequence),
        )
        .chain(
            report
                .equation_invocations()
                .iter()
                .map(|record| record.leave_sequence),
        )
        .chain(
            report
                .boundaries()
                .iter()
                .map(|record| record.leave_sequence),
        )
        .chain(report.atoms().iter().map(|record| record.sequence))
        .chain(report.fragments().iter().map(|record| record.sequence))
        .chain(
            report
                .flushes()
                .iter()
                .map(|record| record.outcome_sequence),
        )
        .chain(report.geometry().iter().map(|record| record.sequence))
        .chain(report.anchors().iter().map(|record| record.sequence))
        .max()
        .unwrap_or(0);
    sequence.saturating_add(1)
}

fn deepest_owner_at(
    nodes: &[ProjectionNode],
    sequence: u64,
    atom: Option<u32>,
    kind: RecordKind,
    index: u32,
) -> Result<ProjectionNodeKey, ProjectionPlanError> {
    let mut best = None::<(usize, ProjectionNodeKey)>;
    for node in nodes {
        if matches!(node.kind, ProjectionNodeKind::ManDefinitionRun { .. }) {
            continue;
        }
        if !(node.span.enter_sequence <= sequence && sequence < node.span.leave_sequence) {
            continue;
        }
        if atom.is_some_and(|atom| !node.span.atoms.contains(&atom)) {
            continue;
        }
        let depth = node_depth(nodes, node.key);
        match best {
            None => best = Some((depth, node.key)),
            Some((best_depth, _)) if depth > best_depth => best = Some((depth, node.key)),
            Some((best_depth, best_key)) if depth == best_depth && best_key != node.key => {
                return Err(ProjectionPlanError::AmbiguousRecordOwner {
                    kind,
                    index,
                    first: best_key,
                    first_kind: nodes[best_key.0 as usize].kind,
                    second: node.key,
                    second_kind: node.kind,
                });
            }
            _ => {}
        }
    }
    best.map(|(_, key)| key)
        .ok_or(ProjectionPlanError::NoOwnerForRecord {
            kind,
            index,
            sequence,
        })
}

fn owner_with_native_evidence(
    nodes: &[ProjectionNode],
    report: &NativeExecutionReport,
    wrapper_nodes: &[Option<ProjectionNodeKey>],
    wrapper: Option<u32>,
    specialized_atom_owner: Option<ProjectionNodeKey>,
    fallback_owner: Option<ProjectionNodeKey>,
    specialized_nodes: &[ProjectionNodeKey],
    sequence: u64,
    atom: Option<u32>,
    kind: RecordKind,
    index: u32,
) -> Result<ProjectionNodeKey, ProjectionPlanError> {
    let explicit = nearest_structural_parent(report, wrapper_nodes, wrapper);
    if let Some(owner) = explicit {
        let span = &nodes[owner.0 as usize].span;
        if !(span.enter_sequence <= sequence
            && sequence < span.leave_sequence
            && atom.is_none_or(|atom| span.atoms.contains(&atom)))
        {
            return Err(ProjectionPlanError::RecordOutsideOwner { kind, index, owner });
        }
    }
    let specialized = if atom.is_some() {
        specialized_atom_owner
    } else {
        deepest_specialized_owner_at(nodes, specialized_nodes, sequence, kind, index)?
    };
    match (explicit, specialized) {
        (Some(owner), Some(specialized)) if is_ancestor(nodes, owner, specialized) => {
            Ok(specialized)
        }
        (Some(owner), Some(specialized)) if owner == specialized => Ok(owner),
        (Some(owner), None) => Ok(owner),
        (None, Some(specialized)) => Ok(specialized),
        (Some(owner), Some(specialized)) => Err(ProjectionPlanError::AmbiguousRecordOwner {
            kind,
            index,
            first: owner,
            first_kind: nodes[owner.0 as usize].kind,
            second: specialized,
            second_kind: nodes[specialized.0 as usize].kind,
        }),
        (None, None) => {
            fallback_owner.map_or_else(|| deepest_owner_at(nodes, sequence, atom, kind, index), Ok)
        }
    }
}

fn deepest_specialized_owner_at(
    nodes: &[ProjectionNode],
    specialized_nodes: &[ProjectionNodeKey],
    sequence: u64,
    kind: RecordKind,
    index: u32,
) -> Result<Option<ProjectionNodeKey>, ProjectionPlanError> {
    let mut specialized = None::<(usize, ProjectionNodeKey)>;
    for &key in specialized_nodes {
        let node = &nodes[key.0 as usize];
        if !(node.span.enter_sequence <= sequence && sequence < node.span.leave_sequence) {
            continue;
        }
        let candidate = (node_depth(nodes, node.key), node.key);
        match specialized {
            None => specialized = Some(candidate),
            Some((depth, _)) if candidate.0 > depth => specialized = Some(candidate),
            Some((depth, first)) if candidate.0 == depth && first != candidate.1 => {
                return Err(ProjectionPlanError::AmbiguousRecordOwner {
                    kind,
                    index,
                    first,
                    first_kind: nodes[first.0 as usize].kind,
                    second: candidate.1,
                    second_kind: nodes[candidate.1.0 as usize].kind,
                });
            }
            _ => {}
        }
    }
    Ok(specialized.map(|(_, key)| key))
}

fn build_atom_owner_index(
    nodes: &[ProjectionNode],
    report: &NativeExecutionReport,
    include: impl Fn(&ProjectionNode) -> bool,
) -> Result<Vec<Option<ProjectionNodeKey>>, ProjectionPlanError> {
    let mut owners = vec![None::<ProjectionNodeKey>; report.atoms().len()];
    let depths = nodes
        .iter()
        .map(|node| node_depth(nodes, node.key))
        .collect::<Vec<_>>();
    let mut starts = BTreeMap::<u32, Vec<ProjectionNodeKey>>::new();
    let mut ends = BTreeMap::<u32, Vec<ProjectionNodeKey>>::new();
    for node in nodes.iter().filter(|node| include(node)) {
        if node.span.atoms.is_empty() {
            continue;
        }
        starts
            .entry(node.span.atoms.start)
            .or_default()
            .push(node.key);
        ends.entry(node.span.atoms.end).or_default().push(node.key);
    }
    let mut active = BTreeSet::<(usize, ProjectionNodeKey)>::new();
    'atoms: for (atom_index, slot) in owners.iter_mut().enumerate() {
        let atom =
            u32::try_from(atom_index).map_err(|_| ProjectionPlanError::NodeCapacityExceeded)?;
        if let Some(leaving) = ends.get(&atom) {
            for key in leaving {
                active.remove(&(depths[key.0 as usize], *key));
            }
        }
        if let Some(entering) = starts.get(&atom) {
            for key in entering {
                active.insert((depths[key.0 as usize], *key));
            }
        }
        let sequence = report.atoms()[atom_index].sequence;
        let mut candidates = active.iter().rev().filter(|(_, key)| {
            let span = &nodes[key.0 as usize].span;
            span.enter_sequence <= sequence && sequence < span.leave_sequence
        });
        if let Some(&(depth, key)) = candidates.next() {
            if let Some(&(_, previous)) = candidates
                .next()
                .filter(|(candidate, _)| *candidate == depth)
            {
                // Explicit native wrapper evidence can still disambiguate an
                // atom whose interval envelope has two equally deep owners.
                // Leave only that atom unresolved so the fallback path emits
                // the full diagnostic if no such evidence exists.
                let _ = previous;
                continue 'atoms;
            }
            *slot = Some(key);
        }
    }
    Ok(owners)
}

fn effect_wrapper(report: &NativeExecutionReport, plan: &CanonicalEffectPlan) -> Option<u32> {
    plan.records.iter().find_map(|record| match record {
        RawRecordRef::Boundary(key) => report.boundaries()[*key as usize].wrapper,
        RawRecordRef::Atom(_)
        | RawRecordRef::Fragment(_)
        | RawRecordRef::Flush(_)
        | RawRecordRef::Control(_)
        | RawRecordRef::Geometry(_)
        | RawRecordRef::Placement(_)
        | RawRecordRef::Anchor(_) => None,
    })
}

fn effect_source_node(
    report: &NativeExecutionReport,
    plan: &CanonicalEffectPlan,
) -> Option<ExecutionNodeKey> {
    plan.records.iter().find_map(|record| match record {
        RawRecordRef::Boundary(key) => report.boundaries()[*key as usize].node,
        RawRecordRef::Flush(key) => report.flushes()[*key as usize].node,
        RawRecordRef::Geometry(key) => report.geometry()[*key as usize].node,
        RawRecordRef::Atom(_)
        | RawRecordRef::Fragment(_)
        | RawRecordRef::Control(_)
        | RawRecordRef::Placement(_)
        | RawRecordRef::Anchor(_) => None,
    })
}

fn node_depth(nodes: &[ProjectionNode], mut key: ProjectionNodeKey) -> usize {
    let mut depth = 0;
    while let Some(parent) = nodes[key.0 as usize].execution_parent {
        depth += 1;
        key = parent;
    }
    depth
}

fn direct_execution_runs(
    indices: &[u32],
    owner: &ProjectionNode,
    report: &NativeExecutionReport,
    sections: &SectionIndex,
) -> Vec<Range<u32>> {
    direct_execution_runs_with(
        indices,
        owner,
        |index| report.atoms()[index as usize].sequence,
        |sequence| sections.destination_for_sequence(sequence),
    )
}

fn direct_execution_runs_with(
    indices: &[u32],
    owner: &ProjectionNode,
    atom_sequence: impl Fn(u32) -> u64,
    destination: impl Fn(u64) -> ProjectionDestination,
) -> Vec<Range<u32>> {
    let mut runs = Vec::new();
    let Some(&first) = indices.first() else {
        return runs;
    };
    // At this stage the owner already contains structural children,
    // specialized invocations, canonical effects, and anchors, but no atom
    // flows yet.  Every one of those events is an execution-order barrier.
    // A flow spanning across one would force the materializer to replay the
    // event either before or after the complete atom run, which is precisely
    // how paragraph distance used to migrate behind its body.
    let mut barriers = owner
        .pieces
        .iter()
        .map(ProjectionPiece::order)
        .collect::<Vec<_>>();
    barriers.sort_unstable();
    let mut next_barrier = 0;
    let mut start = first;
    let mut previous = first;
    for &index in &indices[1..] {
        let previous_order = ProjectionOrder {
            sequence: atom_sequence(previous),
            phase: ProjectionPhase::Content,
            stable: previous,
        };
        let current_order = ProjectionOrder {
            sequence: atom_sequence(index),
            phase: ProjectionPhase::Content,
            stable: index,
        };
        while barriers
            .get(next_barrier)
            .is_some_and(|barrier| *barrier <= previous_order)
        {
            next_barrier += 1;
        }
        let has_barrier = barriers
            .get(next_barrier)
            .is_some_and(|barrier| *barrier <= current_order);
        let changes_destination =
            destination(previous_order.sequence) != destination(current_order.sequence);
        if index != previous.saturating_add(1) || has_barrier || changes_destination {
            runs.push(start..previous.saturating_add(1));
            start = index;
        }
        previous = index;
    }
    runs.push(start..previous.saturating_add(1));
    runs
}

fn geometry_origin_atom(report: &NativeExecutionReport, key: u32) -> Option<u32> {
    let geometry = &report.geometry()[key as usize];
    match geometry.origin_kind {
        GeometryOriginKind::Atom => geometry.origin_key,
        GeometryOriginKind::Fragment => geometry
            .origin_key
            .and_then(|key| report.fragments().get(key as usize))
            .map(|fragment| fragment.atom.0),
        GeometryOriginKind::None | GeometryOriginKind::Flush | GeometryOriginKind::Boundary => None,
    }
}

fn ledger_mut(ownership: &mut ExecutionOwnership, kind: RecordKind) -> &mut [RecordDisposition] {
    match kind {
        RecordKind::Atom => &mut ownership.atoms,
        RecordKind::Fragment => &mut ownership.fragments,
        RecordKind::Flush => &mut ownership.flushes,
        RecordKind::Boundary => &mut ownership.boundaries,
        RecordKind::Control => &mut ownership.controls,
        RecordKind::Geometry => &mut ownership.geometry,
        RecordKind::Placement => &mut ownership.placements,
        RecordKind::Anchor => &mut ownership.anchors,
    }
}

fn ledger(ownership: &ExecutionOwnership, kind: RecordKind) -> &[RecordDisposition] {
    match kind {
        RecordKind::Atom => &ownership.atoms,
        RecordKind::Fragment => &ownership.fragments,
        RecordKind::Flush => &ownership.flushes,
        RecordKind::Boundary => &ownership.boundaries,
        RecordKind::Control => &ownership.controls,
        RecordKind::Geometry => &ownership.geometry,
        RecordKind::Placement => &ownership.placements,
        RecordKind::Anchor => &ownership.anchors,
    }
}

fn validate_piece_order(
    node: &ProjectionNode,
    order: ProjectionOrder,
) -> Result<(), ProjectionPlanError> {
    // Fixed CVS pops structural wrappers before observing records at
    // `leave_sequence`; execution intervals are uniformly half-open.
    if order.sequence < node.span.enter_sequence || node.span.leave_sequence <= order.sequence {
        return Err(ProjectionPlanError::PieceOutsideOwner {
            owner: node.key,
            sequence: order.sequence,
        });
    }
    Ok(())
}

fn empty_explanation_ledger(ownership: &ExecutionOwnership) -> ExecutionOwnership {
    ExecutionOwnership {
        atoms: vec![RecordDisposition::Unassigned; ownership.atoms.len()],
        fragments: vec![RecordDisposition::Unassigned; ownership.fragments.len()],
        flushes: vec![RecordDisposition::Unassigned; ownership.flushes.len()],
        boundaries: vec![RecordDisposition::Unassigned; ownership.boundaries.len()],
        controls: vec![RecordDisposition::Unassigned; ownership.controls.len()],
        geometry: vec![RecordDisposition::Unassigned; ownership.geometry.len()],
        placements: vec![RecordDisposition::Unassigned; ownership.placements.len()],
        anchors: vec![RecordDisposition::Unassigned; ownership.anchors.len()],
    }
}

fn validate_explanations(
    explained: &ExecutionOwnership,
    ownership: &ExecutionOwnership,
) -> Result<(), ProjectionPlanError> {
    for kind in [
        RecordKind::Atom,
        RecordKind::Fragment,
        RecordKind::Flush,
        RecordKind::Boundary,
        RecordKind::Control,
        RecordKind::Geometry,
        RecordKind::Placement,
        RecordKind::Anchor,
    ] {
        validate_ledger_explanations(kind, ledger(ownership, kind), ledger(explained, kind))?;
    }
    Ok(())
}

fn validate_ledger_explanations(
    kind: RecordKind,
    ledger: &[RecordDisposition],
    explained: &[RecordDisposition],
) -> Result<(), ProjectionPlanError> {
    for (index, (disposition, explanation)) in ledger.iter().zip(explained).enumerate() {
        let index = u32::try_from(index).map_err(|_| ProjectionPlanError::NodeCapacityExceeded)?;
        match (*disposition, *explanation) {
            (RecordDisposition::Owned(owner), RecordDisposition::Owned(actual))
                if owner == actual => {}
            (RecordDisposition::Owned(owner), RecordDisposition::Unassigned) => {
                return Err(ProjectionPlanError::OwnedRecordUnexplained { kind, index, owner });
            }
            (RecordDisposition::Owned(ledger_owner), RecordDisposition::Owned(plan_owner)) => {
                return Err(ProjectionPlanError::RecordExplanationOwnerMismatch {
                    kind,
                    index,
                    ledger_owner,
                    plan_owner,
                });
            }
            (RecordDisposition::Omitted(reason), RecordDisposition::Owned(owner)) => {
                return Err(ProjectionPlanError::OmittedRecordExplained {
                    kind,
                    index,
                    owner,
                    reason,
                });
            }
            (RecordDisposition::Unassigned, RecordDisposition::Owned(owner)) => {
                return Err(ProjectionPlanError::UnownedRecordExplained {
                    kind,
                    index,
                    plan_owner: owner,
                });
            }
            (
                RecordDisposition::Unassigned | RecordDisposition::Omitted(_),
                RecordDisposition::Unassigned,
            ) => {}
            (_, RecordDisposition::Omitted(_)) => unreachable!("explanation ledger never omits"),
        }
    }
    Ok(())
}

fn register_explanation(
    explained: &mut ExecutionOwnership,
    record: RawRecordRef,
    owner: ProjectionNodeKey,
) -> Result<(), ProjectionPlanError> {
    let (kind, index) = record.parts();
    let Some(slot) = ledger_mut(explained, kind).get_mut(index as usize) else {
        return Err(ProjectionPlanError::RecordOutOfBounds { kind, index });
    };
    match *slot {
        RecordDisposition::Unassigned => {
            *slot = RecordDisposition::Owned(owner);
            Ok(())
        }
        RecordDisposition::Owned(first) => Err(ProjectionPlanError::RecordExplainedTwice {
            record,
            first,
            second: owner,
        }),
        RecordDisposition::Omitted(_) => unreachable!("explanation ledger never omits"),
    }
}

fn validate_specialized_recipes(nodes: &[ProjectionNode]) -> Result<(), ProjectionPlanError> {
    for node in nodes {
        let expected = specialized_recipe(node.kind);
        let recipes = node
            .pieces
            .iter()
            .filter_map(|piece| match piece {
                ProjectionPiece::Specialized { recipe, .. } => Some(*recipe),
                _ => None,
            })
            .collect::<Vec<_>>();
        match (expected, recipes.as_slice()) {
            (None, []) => {}
            (Some(expected), [actual]) if expected == *actual => {}
            _ => {
                return Err(ProjectionPlanError::InvalidSpecializedRecipe(node.key));
            }
        }
    }
    Ok(())
}

fn validate_total(
    kind: RecordKind,
    records: &[RecordDisposition],
) -> Result<(), ProjectionPlanError> {
    records
        .iter()
        .position(|record| *record == RecordDisposition::Unassigned)
        .map_or(Ok(()), |index| {
            Err(ProjectionPlanError::UnassignedRecord {
                kind,
                index: u32::try_from(index).unwrap(),
            })
        })
}

fn spans_overlap(left: &ExecutionSpan, right: &ExecutionSpan) -> bool {
    let atoms_overlap = left.atoms.start < right.atoms.end && right.atoms.start < left.atoms.end;
    let execution_overlaps =
        left.enter_sequence < right.leave_sequence && right.enter_sequence < left.leave_sequence;
    atoms_overlap || execution_overlaps
}

fn strictly_contains(parent: &ExecutionSpan, child: &ExecutionSpan) -> bool {
    parent.contains(child)
        && (parent.atoms != child.atoms
            || parent.enter_sequence != child.enter_sequence
            || parent.leave_sequence != child.leave_sequence)
}

fn is_ancestor(
    nodes: &[ProjectionNode],
    ancestor: ProjectionNodeKey,
    mut descendant: ProjectionNodeKey,
) -> bool {
    while let Some(parent) = nodes[descendant.0 as usize].execution_parent {
        if parent == ancestor {
            return true;
        }
        descendant = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(atoms: Range<u32>, enter_sequence: u64, leave_sequence: u64) -> ExecutionSpan {
        ExecutionSpan {
            atoms,
            enter_sequence,
            leave_sequence,
        }
    }

    fn order(sequence: u64, stable: u32) -> ProjectionOrder {
        ProjectionOrder {
            sequence,
            phase: ProjectionPhase::Content,
            stable,
        }
    }

    fn counts(atoms: usize, boundaries: usize, anchors: usize) -> ExecutionRecordCounts {
        ExecutionRecordCounts {
            atoms,
            boundaries,
            anchors,
            ..ExecutionRecordCounts::default()
        }
    }

    fn effect(key: u32, sequence: u64, records: Vec<RawRecordRef>) -> CanonicalEffectPlan {
        CanonicalEffectPlan {
            key: EffectKey(key),
            cause: EffectCauseKey::StandaloneGeometry(key),
            records,
            sequence,
            output_role: ExecutionOutputRole::Content,
            outcome: CanonicalEffectOutcome::StateOnly,
        }
    }

    fn resolve_to_root(builder: &mut ProjectionArenaBuilder) {
        for node in &mut builder.nodes {
            if node.destination == ProjectionDestination::Deferred {
                node.destination = ProjectionDestination::Root;
            }
            for piece in &mut node.pieces {
                if piece.destination() == ProjectionDestination::Deferred {
                    piece.set_destination(ProjectionDestination::Root);
                }
            }
        }
    }

    #[test]
    fn nested_equal_envelopes_are_legal_only_through_explicit_parentage() {
        // Fixed CVS `print_mdoc_node()` enters node, list-item, and region
        // wrappers before executing children, then leaves them in reverse.
        // Equal atom envelopes are therefore valid when the wrapper parent
        // relation proves nesting; flat competing owners are not.
        let mut builder = ProjectionArenaBuilder::new(counts(2, 0, 0));
        let outer = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..2, 1, 10))
            .unwrap();
        let inner = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..2, 2, 9))
            .unwrap();
        builder.attach_structural(outer, inner).unwrap();
        builder.claim_flow(inner, 0..2, order(3, 0)).unwrap();
        resolve_to_root(&mut builder);
        let arena = builder.finish().unwrap();
        assert_eq!(arena.roots, [outer]);
        assert_eq!(arena.ownership.atoms, [RecordDisposition::Owned(inner); 2]);
    }

    #[test]
    fn structural_leave_sequence_is_not_owned_by_the_closed_child() {
        // Fixed CVS `ADVANCE_STRUCTURAL_WRAPPERS` pops wrappers whose leave
        // sequence is <= the next record sequence.  The record at 5 belongs
        // to the still-open parent, never the child ending at 5.
        let mut builder = ProjectionArenaBuilder::new(counts(0, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 10))
            .unwrap();
        let child = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..0, 1, 5))
            .unwrap();
        builder.attach_structural(root, child).unwrap();
        assert_eq!(
            deepest_owner_at(&builder.nodes, 5, None, RecordKind::Anchor, 0),
            Ok(root)
        );
        assert_eq!(
            validate_piece_order(&builder.nodes[child.0 as usize], order(5, 0)),
            Err(ProjectionPlanError::PieceOutsideOwner {
                owner: child,
                sequence: 5,
            })
        );
    }

    #[test]
    fn crossing_or_contained_siblings_are_rejected_instead_of_prioritized() {
        let mut builder = ProjectionArenaBuilder::new(counts(4, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..4, 0, 20))
            .unwrap();
        let left = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..3, 1, 12))
            .unwrap();
        let right = builder
            .add_node(ProjectionNodeKind::Flow, None, span(2..4, 10, 18))
            .unwrap();
        builder.attach_structural(root, left).unwrap();
        builder.attach_structural(root, right).unwrap();
        builder.claim_flow(left, 0..2, order(2, 0)).unwrap();
        builder.claim_flow(right, 2..4, order(11, 0)).unwrap();
        resolve_to_root(&mut builder);
        assert!(matches!(
            builder.finish(),
            Err(ProjectionPlanError::OverlappingSiblings {
                left: actual_left,
                right: actual_right,
                ..
            }) if actual_left == left && actual_right == right
        ));
    }

    #[test]
    fn inline_equation_is_nested_under_the_executing_table_invocation() {
        // The pinned reference for `/tmp/k23-table-eqn.1` executes the inline
        // equation while `term_tbl()` is inside the cell invocation.  The
        // interval forest must therefore be Table -> Row -> Invocation ->
        // Equation -> EquationInvocation, so the expression cannot also be
        // flattened as surrounding table text.
        let mut builder = ProjectionArenaBuilder::new(counts(3, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..3, 0, 100))
            .unwrap();
        let table = builder
            .add_node(
                ProjectionNodeKind::Table(ExecutionTableKey(0)),
                None,
                span(0..3, 10, 90),
            )
            .unwrap();
        let row = builder
            .add_node(
                ProjectionNodeKind::TableRow(ExecutionTableRowKey(0)),
                None,
                span(0..3, 12, 88),
            )
            .unwrap();
        let cell = builder
            .add_node(
                ProjectionNodeKind::TableCellInvocation(ExecutionTableCellInvocationKey(0)),
                None,
                span(0..3, 20, 80),
            )
            .unwrap();
        let equation = builder
            .add_node(
                ProjectionNodeKind::Equation(ExecutionEquationKey(0)),
                None,
                span(1..2, 30, 70),
            )
            .unwrap();
        let equation_invocation = builder
            .add_node(
                ProjectionNodeKind::EquationInvocation(ExecutionEquationInvocationKey(0)),
                None,
                span(1..2, 35, 65),
            )
            .unwrap();
        builder.attach_structural(table, row).unwrap();
        builder.attach_structural(row, cell).unwrap();
        builder
            .attach_structural(equation, equation_invocation)
            .unwrap();
        builder.attach_remaining_by_interval(root).unwrap();
        assert_eq!(builder.nodes[table.0 as usize].execution_parent, Some(root));
        assert_eq!(
            builder.nodes[equation.0 as usize].execution_parent,
            Some(cell)
        );
        assert_eq!(
            builder.nodes[equation_invocation.0 as usize].execution_parent,
            Some(equation)
        );
    }

    #[test]
    fn heading_execution_owner_has_section_destination_but_no_body_content_parent() {
        // Fixed CVS executes heading glyphs inside the dedicated heading
        // wrapper. They must be consumed by the heading recipe exactly once;
        // section placement is a separate location-map decision.
        let mut builder = ProjectionArenaBuilder::new(counts(1, 0, 1));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..1, 0, 20))
            .unwrap();
        let heading = builder
            .add_node(
                ProjectionNodeKind::Heading {
                    wrapper: 7,
                    kind: ExecutionHeadingKind::MdocSection,
                },
                None,
                span(0..1, 2, 8),
            )
            .unwrap();
        builder.attach_execution(root, heading).unwrap();
        builder.claim_flow(heading, 0..1, order(3, 0)).unwrap();
        builder.claim_anchor(heading, 0, order(4, 0)).unwrap();
        let sections = SectionIndex::from_locations(vec![SectionLocation {
            key: SectionKey(11),
            heading_wrapper: 7,
            enter_sequence: 2,
            leave_sequence: 20,
        }])
        .unwrap();
        bind_node_destinations(&mut builder.nodes, &sections).unwrap();
        builder.absorb_heading_descendants().unwrap();

        let arena = builder.finish().unwrap();
        assert_eq!(arena.node(heading).execution_parent, Some(root));
        assert_eq!(arena.node(heading).content_parent, None);
        assert_eq!(
            arena.node(heading).destination,
            ProjectionDestination::Section(SectionKey(11))
        );
        assert!(arena.node(root).pieces.is_empty());
        assert_eq!(arena.ownership.atoms, [RecordDisposition::Owned(heading)]);
        assert_eq!(arena.ownership.anchors, [RecordDisposition::Owned(heading)]);
        assert_eq!(arena.heading_absorptions.len(), 1);
        assert_eq!(
            arena.heading_absorptions[0].records,
            [RawRecordRef::Atom(0), RawRecordRef::Anchor(0)]
        );
    }

    #[test]
    fn destination_cut_inside_equal_endpoints_is_rejected() {
        let mut builder = ProjectionArenaBuilder::new(counts(0, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 100))
            .unwrap();
        let owner = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..0, 10, 40))
            .unwrap();
        builder.attach_structural(root, owner).unwrap();
        let sections = SectionIndex::from_locations(vec![
            SectionLocation {
                key: SectionKey(1),
                heading_wrapper: 1,
                enter_sequence: 1,
                leave_sequence: 90,
            },
            SectionLocation {
                key: SectionKey(2),
                heading_wrapper: 2,
                enter_sequence: 20,
                leave_sequence: 30,
            },
        ])
        .unwrap();
        assert!(matches!(
            bind_node_destinations(&mut builder.nodes, &sections),
            Err(ProjectionPlanError::StructuralOwnerCrossesDestination { node, .. })
                if node == owner
        ));
    }

    #[test]
    fn explicit_definition_segments_own_nested_content_without_interval_guessing() {
        let mut builder = ProjectionArenaBuilder::new(counts(3, 1, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..3, 0, 100))
            .unwrap();
        let list = builder
            .add_node(
                ProjectionNodeKind::MdocList {
                    owner: ExecutionNodeKey(10),
                    kind: ExecutionMdocListKind::Column,
                },
                Some(ExecutionNodeKey(10)),
                span(0..3, 5, 80),
            )
            .unwrap();
        let item = builder
            .add_node(
                ProjectionNodeKind::NativeScope {
                    wrapper: 4,
                    kind: ProjectionScopeKind::MdocListItem(ExecutionMdocListKind::Column),
                },
                Some(ExecutionNodeKey(11)),
                span(0..3, 10, 70),
            )
            .unwrap();
        let head = builder
            .add_node(
                ProjectionNodeKind::DefinitionSegment {
                    kind: DefinitionSegmentKind::MdocHead,
                },
                Some(ExecutionNodeKey(12)),
                span(0..1, 11, 20),
            )
            .unwrap();
        let body0 = builder
            .add_node(
                ProjectionNodeKind::DefinitionSegment {
                    kind: DefinitionSegmentKind::MdocBody { ordinal: 0 },
                },
                Some(ExecutionNodeKey(13)),
                span(1..2, 21, 40),
            )
            .unwrap();
        let body1 = builder
            .add_node(
                ProjectionNodeKind::DefinitionSegment {
                    kind: DefinitionSegmentKind::MdocBody { ordinal: 1 },
                },
                Some(ExecutionNodeKey(14)),
                span(2..3, 41, 60),
            )
            .unwrap();
        let nested = builder
            .add_node(ProjectionNodeKind::Flow, None, span(2..3, 45, 55))
            .unwrap();
        builder.attach_structural(root, list).unwrap();
        builder.attach_structural(list, item).unwrap();
        builder.attach_structural(item, head).unwrap();
        builder.attach_structural(item, body0).unwrap();
        builder.attach_structural(item, body1).unwrap();
        builder.attach_structural(body1, nested).unwrap();
        builder.claim_flow(head, 0..1, order(12, 0)).unwrap();
        builder.claim_flow(body0, 1..2, order(22, 1)).unwrap();
        builder.claim_flow(nested, 2..3, order(46, 2)).unwrap();
        builder
            .claim_effect_plan(
                nested,
                &CanonicalEffectPlan {
                    key: EffectKey(0),
                    cause: EffectCauseKey::BoundaryRoot(0),
                    records: vec![RawRecordRef::Boundary(0)],
                    sequence: 50,
                    output_role: ExecutionOutputRole::Content,
                    outcome: CanonicalEffectOutcome::StateOnly,
                },
            )
            .unwrap();
        resolve_to_root(&mut builder);
        let arena = builder.finish().unwrap();
        assert_eq!(arena.node(head).content_parent, Some(item));
        assert_eq!(arena.node(body0).content_parent, Some(item));
        assert_eq!(arena.node(body1).content_parent, Some(item));
        assert_eq!(arena.node(nested).content_parent, Some(body1));
        assert!(arena.node(nested).pieces.iter().any(|piece| matches!(
            piece,
            ProjectionPiece::Stream { events, .. }
                if events.iter().any(|event| matches!(event,
                    StreamEvent::CanonicalEffect { key: EffectKey(0), .. }))
        )));
    }

    #[test]
    fn man_head_and_body_are_explicit_segment_owners() {
        let mut builder = ProjectionArenaBuilder::new(counts(2, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..2, 0, 50))
            .unwrap();
        let block = builder
            .add_node(
                ProjectionNodeKind::NativeScope {
                    wrapper: 3,
                    kind: ProjectionScopeKind::ManBlock(ExecutionManBlockKind::TaggedParagraph),
                },
                Some(ExecutionNodeKey(5)),
                span(0..2, 5, 40),
            )
            .unwrap();
        let head = builder
            .add_node(
                ProjectionNodeKind::DefinitionSegment {
                    kind: DefinitionSegmentKind::ManHead,
                },
                Some(ExecutionNodeKey(6)),
                span(0..1, 6, 15),
            )
            .unwrap();
        let body = builder
            .add_node(
                ProjectionNodeKind::DefinitionSegment {
                    kind: DefinitionSegmentKind::ManBody,
                },
                Some(ExecutionNodeKey(7)),
                span(1..2, 16, 35),
            )
            .unwrap();
        builder.attach_structural(root, block).unwrap();
        builder.attach_structural(block, head).unwrap();
        builder.attach_structural(block, body).unwrap();
        builder.claim_flow(head, 0..1, order(7, 0)).unwrap();
        builder.claim_flow(body, 1..2, order(17, 1)).unwrap();
        resolve_to_root(&mut builder);
        let arena = builder.finish().unwrap();
        assert_eq!(arena.node(head).content_parent, Some(block));
        assert_eq!(arena.node(body).content_parent, Some(block));
    }

    #[test]
    fn section_index_rejects_duplicate_headings_and_invalid_intervals() {
        let duplicate = SectionIndex::from_locations(vec![
            SectionLocation {
                key: SectionKey(1),
                heading_wrapper: 7,
                enter_sequence: 2,
                leave_sequence: 10,
            },
            SectionLocation {
                key: SectionKey(2),
                heading_wrapper: 7,
                enter_sequence: 11,
                leave_sequence: 20,
            },
        ]);
        assert_eq!(
            duplicate,
            Err(ProjectionPlanError::DuplicateHeadingDestination { wrapper: 7 })
        );

        let invalid = SectionIndex::from_locations(vec![SectionLocation {
            key: SectionKey(3),
            heading_wrapper: 8,
            enter_sequence: 9,
            leave_sequence: 4,
        }]);
        assert_eq!(
            invalid,
            Err(ProjectionPlanError::InvalidSectionInterval {
                key: SectionKey(3),
                enter_sequence: 9,
                leave_sequence: 4,
            })
        );
    }

    #[test]
    fn unresolved_destination_is_not_a_finished_arena_state() {
        let mut builder = ProjectionArenaBuilder::new(counts(0, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 10))
            .unwrap();
        let flow = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..0, 1, 2))
            .unwrap();
        builder.attach_structural(root, flow).unwrap();
        assert_eq!(
            builder.finish(),
            Err(ProjectionPlanError::UnresolvedDestination(flow))
        );
    }

    #[test]
    fn records_require_one_explicit_owner_or_omission_reason() {
        let mut builder = ProjectionArenaBuilder::new(counts(3, 2, 1));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..3, 0, 20))
            .unwrap();
        let flow = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..3, 1, 10))
            .unwrap();
        builder.attach_structural(root, flow).unwrap();
        builder.claim_flow(flow, 0..2, order(1, 0)).unwrap();
        builder
            .omit_atom_range(2..3, OmissionReason::DeviceDecoration)
            .unwrap();
        builder
            .claim_effect_plan(flow, &effect(0, 4, vec![RawRecordRef::Boundary(0)]))
            .unwrap();
        builder
            .omit_boundary(1, OmissionReason::SoftDeviceWrap)
            .unwrap();
        builder.claim_anchor(flow, 0, order(3, 0)).unwrap();
        resolve_to_root(&mut builder);
        let arena = builder.finish().unwrap();
        assert_eq!(
            arena.ownership.atoms,
            [
                RecordDisposition::Owned(flow),
                RecordDisposition::Owned(flow),
                RecordDisposition::Omitted(OmissionReason::DeviceDecoration),
            ]
        );
        let receipt = arena
            .final_consumer(FinalConsumer::Stream {
                owner: flow,
                piece: 0,
            })
            .expect("coalesced stream has one final-consumer receipt");
        assert_eq!(
            receipt.records,
            [
                RawRecordRef::Atom(0),
                RawRecordRef::Atom(1),
                RawRecordRef::Boundary(0),
                RawRecordRef::Anchor(0),
            ]
        );
    }

    #[test]
    fn duplicate_claims_fail_without_mutating_the_second_owner() {
        let mut builder = ProjectionArenaBuilder::new(counts(1, 0, 0));
        let first = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..1, 0, 2))
            .unwrap();
        let second = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..1, 3, 5))
            .unwrap();
        builder.claim_flow(first, 0..1, order(1, 0)).unwrap();
        assert_eq!(
            builder.claim_flow(second, 0..1, order(4, 0)),
            Err(ProjectionPlanError::RecordAlreadyAssigned {
                kind: RecordKind::Atom,
                index: 0,
                disposition: RecordDisposition::Owned(first),
            })
        );
        assert!(builder.nodes[second.0 as usize].pieces.is_empty());
    }

    #[test]
    fn zero_width_events_sort_with_children_without_claiming_parent_content() {
        // Fixed CVS `term_tbl()` calls the table renderer exactly once inside
        // the surrounding node traversal.  BEFORE/child/AFTER must therefore
        // remain three ordered pieces, not a flattened parent envelope plus a
        // second table projection.
        let mut builder = ProjectionArenaBuilder::new(counts(2, 1, 1));
        let display = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..2, 0, 20))
            .unwrap();
        let table = builder
            .add_node(
                ProjectionNodeKind::Table(ExecutionTableKey(0)),
                None,
                span(1..2, 8, 12),
            )
            .unwrap();
        builder.attach_structural(display, table).unwrap();
        builder.claim_flow(display, 0..1, order(2, 0)).unwrap();
        builder.claim_flow(table, 1..2, order(9, 0)).unwrap();
        builder
            .claim_anchor(
                display,
                0,
                ProjectionOrder {
                    sequence: 1,
                    phase: ProjectionPhase::Before,
                    stable: 0,
                },
            )
            .unwrap();
        builder
            .claim_effect_plan(
                display,
                &CanonicalEffectPlan {
                    key: EffectKey(0),
                    cause: EffectCauseKey::BoundaryRoot(0),
                    records: vec![RawRecordRef::Boundary(0)],
                    sequence: 15,
                    output_role: ExecutionOutputRole::Content,
                    outcome: CanonicalEffectOutcome::StateOnly,
                },
            )
            .unwrap();
        builder.collapse_specialized_pieces();
        builder.absorb_specialized_descendants().unwrap();
        resolve_to_root(&mut builder);
        let arena = builder.finish().unwrap();
        assert!(matches!(arena.node(display).pieces.as_slice(), [
            ProjectionPiece::Stream { events, .. },
            ProjectionPiece::Child { node, .. },
            ProjectionPiece::Stream { .. },
        ] if events.len() == 2 && *node == table));
    }

    #[test]
    fn specialized_receipt_absorbs_full_record_envelope_once() {
        let mut builder = ProjectionArenaBuilder::new(counts(1, 1, 1));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..1, 0, 20))
            .unwrap();
        let table = builder
            .add_node(
                ProjectionNodeKind::Table(ExecutionTableKey(0)),
                None,
                span(0..1, 2, 18),
            )
            .unwrap();
        builder.attach_structural(root, table).unwrap();
        builder
            .specialized_anchor_consumers
            .insert(table, BTreeSet::from([0]));
        builder.claim_flow(table, 0..1, order(3, 0)).unwrap();
        builder.claim_anchor(table, 0, order(4, 0)).unwrap();
        builder
            .claim_effect_plan(
                table,
                &CanonicalEffectPlan {
                    key: EffectKey(0),
                    cause: EffectCauseKey::BoundaryRoot(0),
                    records: vec![RawRecordRef::Boundary(0)],
                    sequence: 5,
                    output_role: ExecutionOutputRole::Content,
                    outcome: CanonicalEffectOutcome::StateOnly,
                },
            )
            .unwrap();
        builder.collapse_specialized_pieces();
        builder.absorb_specialized_descendants().unwrap();
        resolve_to_root(&mut builder);
        let arena = builder.finish().unwrap();
        assert_eq!(arena.specialized_absorptions.len(), 1);
        assert_eq!(
            arena.specialized_absorptions[0].records,
            [
                RawRecordRef::Atom(0),
                RawRecordRef::Boundary(0),
                RawRecordRef::Anchor(0),
            ]
        );
        let [ProjectionPiece::Specialized { records, .. }] = arena.node(table).pieces.as_slice()
        else {
            panic!("typed table root must own one collapsed recipe")
        };
        assert_eq!(records, &arena.specialized_absorptions[0].records);
        assert_eq!(
            arena
                .final_consumer(FinalConsumer::Specialized(table))
                .map(|receipt| receipt.records.as_slice()),
            Some(arena.specialized_absorptions[0].records.as_slice())
        );
    }

    #[test]
    fn specialized_recipe_rejects_anchor_absent_from_typed_model() {
        let mut builder = ProjectionArenaBuilder::new(counts(0, 0, 1));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 20))
            .unwrap();
        let table = builder
            .add_node(
                ProjectionNodeKind::Table(ExecutionTableKey(0)),
                None,
                span(0..0, 2, 18),
            )
            .unwrap();
        let row = builder
            .add_node(
                ProjectionNodeKind::TableRow(ExecutionTableRowKey(0)),
                None,
                span(0..0, 3, 17),
            )
            .unwrap();
        builder.attach_structural(root, table).unwrap();
        builder.attach_structural(table, row).unwrap();
        builder.claim_anchor(row, 0, order(4, 0)).unwrap();
        builder.collapse_specialized_pieces();
        assert_eq!(
            builder.absorb_specialized_descendants(),
            Err(ProjectionPlanError::InvalidSpecializedRecipe(row))
        );
    }

    #[test]
    fn specialized_root_does_not_swallow_untyped_anchor() {
        let mut builder = ProjectionArenaBuilder::new(counts(0, 0, 1));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 20))
            .unwrap();
        let table = builder
            .add_node(
                ProjectionNodeKind::Table(ExecutionTableKey(0)),
                None,
                span(0..0, 2, 18),
            )
            .unwrap();
        builder.attach_structural(root, table).unwrap();
        builder.claim_anchor(table, 0, order(4, 0)).unwrap();
        builder.collapse_specialized_pieces();
        builder.absorb_specialized_descendants().unwrap();
        resolve_to_root(&mut builder);
        let arena = builder.finish().unwrap();
        assert!(matches!(arena.node(table).pieces.as_slice(), [
            ProjectionPiece::Stream { events, .. },
            ProjectionPiece::Specialized { records, .. },
        ] if records.is_empty()
            && matches!(events.as_slice(), [StreamEvent::Anchor { key: 0 }])));
        assert!(
            arena
                .final_consumers
                .iter()
                .any(|receipt| receipt.records == [RawRecordRef::Anchor(0)])
        );
    }

    #[test]
    fn duplicate_zero_record_absorption_receipt_fails_closed() {
        let mut builder = ProjectionArenaBuilder::new(counts(0, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 20))
            .unwrap();
        let table = builder
            .add_node(
                ProjectionNodeKind::Table(ExecutionTableKey(0)),
                None,
                span(0..0, 2, 18),
            )
            .unwrap();
        builder.attach_structural(root, table).unwrap();
        builder.collapse_specialized_pieces();
        let receipt = SpecializedAbsorption {
            root: table,
            descendants: vec![table],
            records: Vec::new(),
        };
        assert_eq!(
            validate_absorption_receipts(
                &builder.nodes,
                &builder.ownership,
                &builder.specialized_anchor_consumers,
                &[receipt.clone(), receipt],
                &[],
            ),
            Err(ProjectionPlanError::InvalidSpecializedRecipe(table))
        );
    }

    #[test]
    fn total_ledger_rejects_silent_fallback() {
        let mut builder = ProjectionArenaBuilder::new(counts(1, 1, 1));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..1, 0, 10))
            .unwrap();
        let flow = builder
            .add_node(ProjectionNodeKind::Flow, None, span(0..1, 1, 2))
            .unwrap();
        builder.attach_structural(root, flow).unwrap();
        builder.claim_flow(flow, 0..1, order(1, 0)).unwrap();
        resolve_to_root(&mut builder);
        assert_eq!(
            builder.finish(),
            Err(ProjectionPlanError::UnassignedRecord {
                kind: RecordKind::Boundary,
                index: 0,
            })
        );
    }

    #[test]
    fn zero_output_controls_keep_total_ownership_and_a_final_consumer() {
        let mut builder = ProjectionArenaBuilder::new(ExecutionRecordCounts {
            controls: 1,
            ..ExecutionRecordCounts::default()
        });
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 3))
            .unwrap();
        builder.claim_control(root, 0, order(1, 0)).unwrap();
        resolve_to_root(&mut builder);

        let arena = builder.finish().unwrap();
        assert_eq!(arena.ownership.controls, [RecordDisposition::Owned(root)]);
        assert!(arena.final_consumers.iter().any(|receipt| {
            receipt.records == [RawRecordRef::Control(0)]
                && matches!(receipt.consumer, FinalConsumer::Stream { owner, .. } if owner == root)
        }));
    }

    #[test]
    fn all_raw_records_are_owned_once_but_materialize_as_one_canonical_effect() {
        // Fixed CVS can execute `print_bvspace()` and `term_flushln()` while
        // settling one structural boundary.  The observer records remain
        // auditable individually, but the projector must emit that formatter
        // effect once rather than replaying every callback as whitespace.
        let mut builder = ProjectionArenaBuilder::new(ExecutionRecordCounts {
            atoms: 1,
            fragments: 1,
            flushes: 1,
            boundaries: 1,
            controls: 0,
            geometry: 1,
            placements: 0,
            anchors: 1,
        });
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..1, 0, 20))
            .unwrap();
        builder.claim_flow(root, 0..1, order(2, 0)).unwrap();
        builder
            .claim_effect_plan(
                root,
                &CanonicalEffectPlan {
                    key: EffectKey(7),
                    cause: EffectCauseKey::BoundaryRoot(0),
                    records: vec![
                        RawRecordRef::Fragment(0),
                        RawRecordRef::Flush(0),
                        RawRecordRef::Boundary(0),
                        RawRecordRef::Geometry(0),
                    ],
                    sequence: 4,
                    output_role: ExecutionOutputRole::Content,
                    outcome: CanonicalEffectOutcome::StateOnly,
                },
            )
            .unwrap();
        builder.claim_anchor(root, 0, order(1, 0)).unwrap();

        resolve_to_root(&mut builder);
        let arena = builder.finish().unwrap();
        assert!(matches!(
            arena.node(root).pieces.as_slice(),
            [
                ProjectionPiece::Stream { events, .. },
            ] if events.len() == 3 && matches!(
                &events[2],
                StreamEvent::CanonicalEffect { key: EffectKey(7), records, .. }
                    if records.len() == 4
            )
        ));
        assert_eq!(arena.ownership.fragments, [RecordDisposition::Owned(root)]);
        assert_eq!(arena.ownership.flushes, [RecordDisposition::Owned(root)]);
        assert_eq!(arena.ownership.boundaries, [RecordDisposition::Owned(root)]);
        assert_eq!(arena.ownership.geometry, [RecordDisposition::Owned(root)]);
    }

    #[test]
    fn raw_claim_without_canonical_effect_fails_closed() {
        let mut builder = ProjectionArenaBuilder::new(ExecutionRecordCounts {
            fragments: 1,
            ..ExecutionRecordCounts::default()
        });
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 2))
            .unwrap();
        builder.claim_fragment(root, 0, order(1, 0)).unwrap();
        resolve_to_root(&mut builder);
        assert_eq!(
            builder.finish(),
            Err(ProjectionPlanError::OwnedRecordUnexplained {
                kind: RecordKind::Fragment,
                index: 0,
                owner: root,
            })
        );
    }

    #[test]
    fn canonical_effect_claim_is_atomic_and_rejects_duplicate_raw_records() {
        let mut builder = ProjectionArenaBuilder::new(ExecutionRecordCounts {
            flushes: 1,
            ..ExecutionRecordCounts::default()
        });
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 0, 2))
            .unwrap();
        assert_eq!(
            builder.claim_effect_plan(
                root,
                &effect(0, 1, vec![RawRecordRef::Flush(0), RawRecordRef::Flush(0)],),
            ),
            Err(ProjectionPlanError::RecordAlreadyAssigned {
                kind: RecordKind::Flush,
                index: 0,
                disposition: RecordDisposition::Owned(root),
            })
        );
        assert_eq!(builder.ownership.flushes, [RecordDisposition::Unassigned]);
        assert!(builder.nodes[root.0 as usize].pieces.is_empty());
    }

    #[test]
    fn typed_omissions_and_piece_ranges_are_validated() {
        let mut builder = ProjectionArenaBuilder::new(ExecutionRecordCounts {
            fragments: 1,
            anchors: 1,
            ..ExecutionRecordCounts::default()
        });
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..0, 5, 10))
            .unwrap();
        assert_eq!(
            builder.omit_fragment(0, OmissionReason::StateOnly),
            Err(ProjectionPlanError::InvalidOmission {
                kind: RecordKind::Fragment,
                reason: OmissionReason::StateOnly,
            })
        );
        assert_eq!(
            builder.claim_anchor(root, 0, order(11, 0)),
            Err(ProjectionPlanError::PieceOutsideOwner {
                owner: root,
                sequence: 11,
            })
        );
        assert_eq!(builder.ownership.anchors, [RecordDisposition::Unassigned]);
    }

    #[test]
    fn malformed_spans_and_wrong_parents_fail_closed() {
        let mut builder = ProjectionArenaBuilder::new(counts(4, 0, 0));
        assert_eq!(
            builder.add_node(
                ProjectionNodeKind::Flow,
                None,
                span(Range { start: 3, end: 2 }, 1, 2),
            ),
            Err(ProjectionPlanError::InvalidSpan {
                atoms: Range { start: 3, end: 2 },
                enter_sequence: 1,
                leave_sequence: 2,
                atom_count: 4,
            })
        );
        assert_eq!(
            builder.add_node(ProjectionNodeKind::Flow, None, span(0..5, 1, 2)),
            Err(ProjectionPlanError::InvalidSpan {
                atoms: 0..5,
                enter_sequence: 1,
                leave_sequence: 2,
                atom_count: 4,
            })
        );
        let parent = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..2, 0, 5))
            .unwrap();
        let child = builder
            .add_node(ProjectionNodeKind::Flow, None, span(1..3, 1, 4))
            .unwrap();
        assert_eq!(
            builder.attach_structural(parent, child),
            Err(ProjectionPlanError::ParentDoesNotContainChild { parent, child })
        );
    }

    #[test]
    fn native_boundary_tree_forms_one_causal_effect_closure() {
        // Fixed CVS `term_vspace()` calls `term_newln()`, which can in turn
        // enter `term_flushln()`.  The observer records a parent boundary and
        // nested boundary/flush facts, but projection must settle one effect.
        let ledger = EffectLedger::from_facts(
            &[
                BoundaryFact {
                    key: 0,
                    parent: None,
                    sequence: 10,
                    request: BoundaryRequest::VerticalSpace,
                    effect: BoundaryEffect::AddedVerticalSpace,
                    direct_device_lines: 1,
                    line_commit_cause: None,
                    visual_before: 0,
                    inline_flow: false,
                    output_role: ExecutionOutputRole::Content,
                },
                BoundaryFact {
                    key: 1,
                    parent: Some(0),
                    sequence: 11,
                    request: BoundaryRequest::DeviceEndline,
                    effect: BoundaryEffect::EndedLine,
                    direct_device_lines: 1,
                    line_commit_cause: Some(libmandoc_rs::LineCommitCause::VerticalBlank),
                    visual_before: 0,
                    inline_flow: false,
                    output_role: ExecutionOutputRole::Content,
                },
            ],
            &[FlushFact {
                key: 0,
                boundary: Some(1),
                sequence: 12,
                outcome: libmandoc_rs::FlushOutcome::Exhausted,
                logical_forced_break: false,
                output_role: ExecutionOutputRole::Content,
            }],
            &[
                GeometryFact {
                    key: 0,
                    cause: GeometryCause::Boundary(1),
                    sequence: 13,
                    output_role: ExecutionOutputRole::Content,
                },
                GeometryFact {
                    key: 1,
                    cause: GeometryCause::Atom,
                    sequence: 14,
                    output_role: ExecutionOutputRole::Content,
                },
            ],
        )
        .unwrap();
        assert_eq!(ledger.effects.len(), 1);
        assert_eq!(ledger.effects[0].cause, EffectCauseKey::BoundaryRoot(0));
        assert_eq!(
            ledger.effects[0].records,
            [
                RawRecordRef::Flush(0),
                RawRecordRef::Boundary(0),
                RawRecordRef::Boundary(1),
                RawRecordRef::Geometry(0),
            ]
        );
        assert_eq!(ledger.flow_geometry, [1]);
    }

    #[test]
    fn malformed_native_causal_graph_fails_closed() {
        assert_eq!(
            EffectLedger::from_facts(
                &[
                    BoundaryFact {
                        key: 0,
                        parent: Some(1),
                        sequence: 1,
                        request: BoundaryRequest::DeviceEndline,
                        effect: BoundaryEffect::NoOutput,
                        direct_device_lines: 0,
                        line_commit_cause: None,
                        visual_before: 0,
                        inline_flow: false,
                        output_role: ExecutionOutputRole::Content,
                    },
                    BoundaryFact {
                        key: 1,
                        parent: Some(0),
                        sequence: 2,
                        request: BoundaryRequest::DeviceEndline,
                        effect: BoundaryEffect::NoOutput,
                        direct_device_lines: 0,
                        line_commit_cause: None,
                        visual_before: 0,
                        inline_flow: false,
                        output_role: ExecutionOutputRole::Content,
                    },
                ],
                &[],
                &[],
            ),
            Err(ProjectionPlanError::CausalCycle {
                kind: RecordKind::Boundary,
                key: 0,
            })
        );
        assert_eq!(
            EffectLedger::from_facts(
                &[],
                &[FlushFact {
                    key: 0,
                    boundary: Some(9),
                    sequence: 1,
                    outcome: libmandoc_rs::FlushOutcome::NoContent,
                    logical_forced_break: false,
                    output_role: ExecutionOutputRole::Content,
                }],
                &[],
            ),
            Err(ProjectionPlanError::InvalidCausalReference {
                kind: RecordKind::Boundary,
                key: 9,
                parent: 9,
            })
        );
    }

    #[test]
    fn native_omission_predicates_are_closed_and_typed() {
        assert_eq!(atom_omission(AtomDisposition::Emitted, 0), Ok(None));
        assert_eq!(
            atom_omission(AtomDisposition::Replaced, 0),
            Ok(Some(OmissionReason::ReplacedAtom))
        );
        assert_eq!(
            atom_omission(AtomDisposition::Buffered, 7),
            Err(ProjectionPlanError::BufferedAtomAtSeal(7))
        );
        assert_eq!(fragment_omission(FragmentRole::Content), None);
        assert_eq!(
            fragment_omission(FragmentRole::MarginDecoration),
            Some(OmissionReason::MarginDecoration)
        );
    }

    #[test]
    fn region_policy_separates_containers_from_state_transitions() {
        // Fixed CVS handles .EX/.EE as ROFFT_ELEM state transitions in
        // `print_man_node()`.  Whole synopsis sections are likewise semantic
        // state over the section topology, not competing content containers.
        // Displays, individual synopsis declarations, and ce/rj captures
        // execute bounded child intervals and therefore remain containers.
        assert_eq!(
            region_policy(ExecutionRegionKind::ManLiteralBegin),
            RegionPolicy::StateTransition
        );
        assert_eq!(
            region_policy(ExecutionRegionKind::ManLiteralEnd),
            RegionPolicy::StateTransition
        );
        assert_eq!(
            region_policy(ExecutionRegionKind::ManSynopsisSection),
            RegionPolicy::StateTransition
        );
        assert_eq!(
            region_policy(ExecutionRegionKind::MdocSynopsisSection),
            RegionPolicy::StateTransition
        );
        for kind in [
            ExecutionRegionKind::ManSynopsisCommand,
            ExecutionRegionKind::MdocSynopsisItem,
            ExecutionRegionKind::MdocDisplayFilled,
            ExecutionRegionKind::MdocDisplayUnfilled,
            ExecutionRegionKind::MdocDisplayLiteral,
            ExecutionRegionKind::MdocDisplayRagged,
            ExecutionRegionKind::MdocDisplayCentered,
            ExecutionRegionKind::MdocDisplayOneLine,
            ExecutionRegionKind::MdocDisplayOneLineLiteral,
            ExecutionRegionKind::CenteredLines,
            ExecutionRegionKind::RightJustifiedLines,
        ] {
            assert_eq!(region_policy(kind), RegionPolicy::Container);
        }
    }

    #[test]
    fn cycles_and_non_root_orphans_fail_closed() {
        let mut cyclic = ProjectionArenaBuilder::new(counts(1, 0, 0));
        let outer = cyclic
            .add_node(ProjectionNodeKind::Root, None, span(0..1, 0, 10))
            .unwrap();
        let inner = cyclic
            .add_node(ProjectionNodeKind::Flow, None, span(0..1, 1, 9))
            .unwrap();
        cyclic.attach_structural(outer, inner).unwrap();
        assert_eq!(
            cyclic.attach_structural(inner, outer),
            Err(ProjectionPlanError::ParentCycle {
                parent: inner,
                child: outer,
            })
        );

        let mut orphaned = ProjectionArenaBuilder::new(counts(0, 0, 0));
        let orphan = orphaned
            .add_node(ProjectionNodeKind::Flow, None, span(0..0, 1, 2))
            .unwrap();
        assert_eq!(
            orphaned.finish(),
            Err(ProjectionPlanError::OrphanedNode(orphan))
        );
    }

    #[test]
    fn section_index_rejects_non_unique_and_crossing_locations() {
        assert_eq!(
            SectionIndex::from_locations(vec![
                SectionLocation {
                    key: SectionKey(0),
                    heading_wrapper: 1,
                    enter_sequence: 1,
                    leave_sequence: 4,
                },
                SectionLocation {
                    key: SectionKey(0),
                    heading_wrapper: 2,
                    enter_sequence: 5,
                    leave_sequence: 8,
                },
            ]),
            Err(ProjectionPlanError::DuplicateSectionKey(SectionKey(0)))
        );
        assert_eq!(
            SectionIndex::from_locations(vec![
                SectionLocation {
                    key: SectionKey(0),
                    heading_wrapper: 1,
                    enter_sequence: 1,
                    leave_sequence: 6,
                },
                SectionLocation {
                    key: SectionKey(1),
                    heading_wrapper: 2,
                    enter_sequence: 4,
                    leave_sequence: 8,
                },
            ]),
            Err(ProjectionPlanError::AmbiguousSectionIntervals {
                first: SectionKey(0),
                second: SectionKey(1),
            })
        );
    }

    #[test]
    fn direct_root_pieces_retain_independent_section_destinations() {
        let mut builder = ProjectionArenaBuilder::new(counts(2, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..2, 0, 20))
            .unwrap();
        builder.claim_flow(root, 0..1, order(2, 0)).unwrap();
        builder.claim_flow(root, 1..2, order(12, 1)).unwrap();
        let sections = SectionIndex::from_locations(vec![
            SectionLocation {
                key: SectionKey(0),
                heading_wrapper: 1,
                enter_sequence: 1,
                leave_sequence: 10,
            },
            SectionLocation {
                key: SectionKey(1),
                heading_wrapper: 2,
                enter_sequence: 10,
                leave_sequence: 20,
            },
        ])
        .unwrap();
        bind_node_destinations(&mut builder.nodes, &sections).unwrap();
        let arena = builder.finish().unwrap();
        let destinations = arena
            .node(root)
            .pieces
            .iter()
            .map(ProjectionPiece::destination)
            .collect::<Vec<_>>();
        assert_eq!(
            destinations,
            [
                ProjectionDestination::Section(SectionKey(0)),
                ProjectionDestination::Section(SectionKey(1)),
            ]
        );
    }

    #[test]
    fn zero_width_children_split_direct_parent_flow() {
        let owner = ProjectionNode {
            key: ProjectionNodeKey(0),
            execution_parent: None,
            content_parent: None,
            destination: ProjectionDestination::Root,
            source_owner: None,
            span: span(0..2, 0, 20),
            kind: ProjectionNodeKind::Root,
            pieces: vec![ProjectionPiece::Child {
                node: ProjectionNodeKey(1),
                role: ChildRole::Structural,
                destination: ProjectionDestination::Root,
                order: order(5, 1),
            }],
        };
        assert_eq!(
            direct_execution_runs_with(
                &[0, 1],
                &owner,
                |atom| if atom == 0 { 2 } else { 8 },
                |_| ProjectionDestination::Root,
            ),
            [0..1, 1..2]
        );
    }

    #[test]
    fn specialized_owner_requires_one_typed_recipe() {
        let mut builder = ProjectionArenaBuilder::new(counts(1, 0, 0));
        let root = builder
            .add_node(ProjectionNodeKind::Root, None, span(0..1, 0, 10))
            .unwrap();
        let table = builder
            .add_node(
                ProjectionNodeKind::Table(ExecutionTableKey(0)),
                None,
                span(0..1, 1, 9),
            )
            .unwrap();
        builder.attach_structural(root, table).unwrap();
        builder.claim_flow(table, 0..1, order(2, 0)).unwrap();
        resolve_to_root(&mut builder);
        assert_eq!(
            builder.finish(),
            Err(ProjectionPlanError::InvalidSpecializedRecipe(table))
        );
    }
}
