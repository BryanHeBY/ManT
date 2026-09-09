//! Bounded, borrowed occurrences from authoritative inline content.
//!
//! This scan does not build a semantic index, project forms, infer entries or
//! resolve destinations. A target appears once per real link node, regardless
//! of duplicates, display filters or later navigation grouping.

use std::ops::ControlFlow;

mod association;
pub use association::*;

use crate::{
    Block, ContentBlockStep as Step, ContentInlineRoot, ContentLocationRef, Document, EntryOwner,
    EntryOwnerLocationRef, Inline, LinkTarget, MAX_CONTENT_DEPTH, MAX_CONTENT_LOCATION_BYTES,
    Section, SourceSpan,
};

/// Default work units across traversal and optional label/form inspection.
pub const DEFAULT_REFERENCE_SCAN_STEPS: usize = 250_000;
/// Hard work ceiling, independent of the source-file size limit.
pub const MAX_REFERENCE_SCAN_STEPS: usize = 1_000_000;
/// Default bytes inspected in link targets and labels.
pub const DEFAULT_REFERENCE_SCAN_BYTES: usize = 8 * 1024 * 1024;
/// Hard ceiling for target/label inspection.
pub const MAX_REFERENCE_SCAN_BYTES: usize = 32 * 1024 * 1024;

/// Limits apply before inspecting or retaining another node's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceScanLimits {
    /// Traversal steps, including containers, skipped links and optional label/form inspection.
    pub steps: usize,
    /// Combined section/block/inline structural depth, clamped to 256.
    pub depth: usize,
    /// Inspected target/label/form bytes; no text is copied by the base scan.
    pub bytes: usize,
}

impl Default for ReferenceScanLimits {
    fn default() -> Self {
        Self {
            steps: DEFAULT_REFERENCE_SCAN_STEPS,
            depth: MAX_CONTENT_DEPTH,
            bytes: DEFAULT_REFERENCE_SCAN_BYTES,
        }
    }
}

/// Why scanning stopped before visiting all requested content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceScanStop {
    /// The consumer explicitly stopped; no subsequent node was inspected.
    Visitor,
    /// Work-unit budget exhausted.
    Steps,
    /// Structural or inline nesting exceeded its limit.
    Depth,
    /// Target/label byte budget exhausted.
    Bytes,
    /// An occurrence position exceeds its encoded-size ceiling.
    Position,
    /// A requested subtree coordinate is invalid for this snapshot.
    InvalidRoot,
}

/// Operation coverage, not a cached whole-document inventory.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceScanReport {
    /// Charged traversal and optional label/form-inspection work units.
    pub steps: usize,
    /// Charged target/label/form bytes; repeated inspections also consume budget.
    pub bytes: usize,
    /// Real occurrences delivered to the consumer; no target deduplication.
    pub occurrences: usize,
    /// Absent only after a complete scan of the selected root.
    pub stopped: Option<ReferenceScanStop>,
}

impl ReferenceScanReport {
    /// Whether the selected source range was fully scanned.
    #[must_use]
    pub const fn complete(self) -> bool {
        self.stopped.is_none()
    }
}

/// Shared operation work account for traversal and optional form association.
/// A callback receives this same account, so repeated form checks cannot reset
/// or bypass the scan's step/byte limits.
#[derive(Debug)]
pub struct ReferenceWorkBudget {
    limits: ReferenceScanLimits,
    steps: usize,
    bytes: usize,
    stopped: Option<ReferenceScanStop>,
}

impl ReferenceWorkBudget {
    fn new(mut limits: ReferenceScanLimits) -> Self {
        limits.steps = limits.steps.min(MAX_REFERENCE_SCAN_STEPS);
        limits.bytes = limits.bytes.min(MAX_REFERENCE_SCAN_BYTES);
        limits.depth = limits.depth.min(MAX_CONTENT_DEPTH);
        Self {
            limits,
            steps: 0,
            bytes: 0,
            stopped: None,
        }
    }

    /// Remaining work units, shared by all phases in this scan.
    #[must_use]
    pub fn remaining_steps(&self) -> usize {
        self.limits.steps.saturating_sub(self.steps)
    }

    /// Remaining target/label/form inspection bytes.
    #[must_use]
    pub fn remaining_bytes(&self) -> usize {
        self.limits.bytes.saturating_sub(self.bytes)
    }

    /// The first failed charge, retained even if the consumer requests a stop.
    #[must_use]
    pub const fn stopped(&self) -> Option<ReferenceScanStop> {
        self.stopped
    }

    /// Charge bounded traversal/inspection before doing work or materializing
    /// data. Failed charges leave counters unchanged and latch the stop reason.
    ///
    /// # Errors
    /// Returns the first depth, step or byte limit that was exceeded, including
    /// a limit previously reached by another phase of this operation.
    pub fn consume(
        &mut self,
        depth: usize,
        steps: usize,
        bytes: usize,
    ) -> Result<(), ReferenceScanStop> {
        let stopped = self.stopped.or_else(|| {
            if depth > self.limits.depth {
                Some(ReferenceScanStop::Depth)
            } else if steps > self.remaining_steps() {
                Some(ReferenceScanStop::Steps)
            } else if bytes > self.remaining_bytes() {
                Some(ReferenceScanStop::Bytes)
            } else {
                None
            }
        });
        if let Some(reason) = stopped {
            self.stopped = Some(reason);
            return Err(reason);
        }
        self.steps += steps;
        self.bytes += bytes;
        Ok(())
    }
}

/// An actual content item and its source-neutral address.
#[derive(Debug, Clone, Copy)]
pub struct ReferenceOwnerRef<'ir, 'path> {
    /// Borrowed original item; attached semantic facts are not revalidated here.
    pub owner: EntryOwner<'ir>,
    /// Exact structural item position, not a semantic selector.
    pub location: EntryOwnerLocationRef<'path>,
}

/// One link and ephemeral structural context, borrowing only final IR.
///
/// Form association is intentionally uninspected at this layer. Consumers must
/// validate and budget original form slices separately rather than calling
/// `EntryOwner::forms` or treating the attached facts as validated relationships.
#[derive(Debug, Clone, Copy)]
pub struct LinkOccurrenceRef<'ir, 'path> {
    /// The actual `Inline::Link` node; never a reconstructed form copy.
    pub link: &'ir Inline,
    /// Original typed destination, including any original fragment.
    pub target: &'ir LinkTarget,
    /// Original visible label children, including empty labels and styling.
    pub label: &'ir [Inline],
    /// Precise snapshot-local node position.
    pub location: ContentLocationRef<'path>,
    /// Nearest actual list/definition item, including unannotated containers.
    pub content_owner: Option<ReferenceOwnerRef<'ir, 'path>>,
    /// Nearest attached semantic owner. Invalid fields never become inferred names.
    pub semantic_owner: Option<ReferenceOwnerRef<'ir, 'path>>,
    /// Nearest available content provenance, not a fabricated exact link span.
    pub source: Option<SourceSpan>,
}

/// Scan all authoritative roots in document order without retaining an index.
/// A callback can stop before the next node or path is inspected/materialized.
pub fn scan_references<'ir>(
    document: &'ir Document,
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_reference_scope(
        document,
        ReferenceScope::Document,
        limits,
        |occurrence, _| visit(occurrence),
    )
}

/// Select an existing source range without looking up semantic names.
#[derive(Debug, Clone, Copy)]
pub enum ReferenceScope<'a> {
    /// Heading, root content and every section.
    Document,
    /// Document heading and root blocks, excluding all sections.
    Overview,
    /// One section and its descendants.
    Section(&'a [u32]),
    /// One block and its descendants.
    Block {
        /// Section path; empty selects document-root content.
        sections: &'a [u32],
        /// Typed block path.
        blocks: &'a [Step],
    },
    /// One list or definition item, with its original terms and body.
    Owner(EntryOwnerLocationRef<'a>),
}

/// Scan an explicit source range with one shared traversal/association budget.
/// The callback may run bounded form association using the supplied account.
pub fn scan_reference_scope<'ir>(
    document: &'ir Document,
    scope: ReferenceScope<'_>,
    limits: ReferenceScanLimits,
    visit: impl for<'path> FnMut(
        LinkOccurrenceRef<'ir, 'path>,
        &mut ReferenceWorkBudget,
    ) -> ControlFlow<()>,
) -> ReferenceScanReport {
    let mut scan = Scan::new(limits, visit);
    let result = (|| match scope {
        ReferenceScope::Document => scan.document(document),
        ReferenceScope::Overview => scan.overview(document),
        ReferenceScope::Section(sections) => {
            scan.charge(
                sections.len(),
                sections.len(),
                sections.len().saturating_mul(std::mem::size_of::<u32>()),
            )?;
            let section = crate::resolve_content_section(document, sections)
                .ok_or(ReferenceScanStop::InvalidRoot)?;
            scan.sections.extend_from_slice(sections);
            scan.section(section)
        }
        ReferenceScope::Block { sections, blocks } => {
            let block = scan.select_block(document, sections, blocks)?;
            scan.block(block)
        }
        ReferenceScope::Owner(owner) => {
            let block = scan.select_block(document, owner.sections, owner.blocks)?;
            let item = match block {
                Block::List { items, .. } => EntryOwner::List(
                    items
                        .get(owner.item_index as usize)
                        .ok_or(ReferenceScanStop::InvalidRoot)?,
                ),
                Block::DefinitionList { items, .. } => EntryOwner::Definition(
                    items
                        .get(owner.item_index as usize)
                        .ok_or(ReferenceScanStop::InvalidRoot)?,
                ),
                _ => return Err(ReferenceScanStop::InvalidRoot),
            };
            scan.item(item, owner.item_index)
        }
    })();
    scan.finish(result)
}

/// Scan one explicit section subtree, independent of entry filters.
/// The initial section path is charged and checked before copying it.
pub fn scan_section_references<'ir>(
    document: &'ir Document,
    sections: &[u32],
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_reference_scope(
        document,
        ReferenceScope::Section(sections),
        limits,
        |occurrence, _| visit(occurrence),
    )
}

/// Scan one original block subtree, preserving its ancestor item ownership.
/// Root lookup consumes the same work/depth budget as ordinary traversal.
pub fn scan_block_references<'ir>(
    document: &'ir Document,
    sections: &[u32],
    blocks: &[Step],
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_reference_scope(
        document,
        ReferenceScope::Block { sections, blocks },
        limits,
        |occurrence, _| visit(occurrence),
    )
}

/// Scan a precisely addressed item's terms and body, without sibling items.
/// Semantic filters and aliases do not participate in locating this root.
pub fn scan_owner_references<'ir>(
    document: &'ir Document,
    owner: EntryOwnerLocationRef<'_>,
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_reference_scope(
        document,
        ReferenceScope::Owner(owner),
        limits,
        |occurrence, _| visit(occurrence),
    )
}

#[derive(Clone, Copy)]
struct OwnerFrame<'a> {
    owner: EntryOwner<'a>,
    blocks_len: usize,
    item: u32,
}

#[derive(Clone, Copy)]
enum Root {
    DocumentHeading,
    SectionHeading,
    Content(ContentInlineRoot),
}

struct Scan<'ir, F> {
    budget: ReferenceWorkBudget,
    report: ReferenceScanReport,
    visit: F,
    sections: Vec<u32>,
    blocks: Vec<Step>,
    path: Vec<u32>,
    owner: Option<OwnerFrame<'ir>>,
    semantic: Option<OwnerFrame<'ir>>,
}

impl<'ir, F> Scan<'ir, F>
where
    F: for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>, &mut ReferenceWorkBudget) -> ControlFlow<()>,
{
    fn new(limits: ReferenceScanLimits, visit: F) -> Self {
        Self {
            budget: ReferenceWorkBudget::new(limits),
            report: ReferenceScanReport::default(),
            visit,
            sections: Vec::new(),
            blocks: Vec::new(),
            path: Vec::new(),
            owner: None,
            semantic: None,
        }
    }

    fn finish(mut self, result: Result<(), ReferenceScanStop>) -> ReferenceScanReport {
        self.report.stopped = self.budget.stopped.or(result.err());
        self.report.steps = self.budget.steps;
        self.report.bytes = self.budget.bytes;
        self.report
    }

    fn depth(&self) -> usize {
        self.sections.len() + self.blocks.len() + self.path.len()
    }

    fn select_block(
        &mut self,
        document: &'ir Document,
        sections: &[u32],
        path: &[Step],
    ) -> Result<&'ir Block, ReferenceScanStop> {
        let depth = sections.len().saturating_add(path.len());
        let bytes = sections
            .len()
            .saturating_mul(std::mem::size_of::<u32>())
            .saturating_add(path.len().saturating_mul(std::mem::size_of::<Step>()));
        self.charge(depth, depth, bytes)?;
        let root = crate::content_location::content_blocks(document, sections)
            .ok_or(ReferenceScanStop::InvalidRoot)?;
        let (Step::Block { index }, rest) =
            path.split_first().ok_or(ReferenceScanStop::InvalidRoot)?
        else {
            return Err(ReferenceScanStop::InvalidRoot);
        };
        if !rest.len().is_multiple_of(2) {
            return Err(ReferenceScanStop::InvalidRoot);
        }
        let mut block = root
            .get(*index as usize)
            .ok_or(ReferenceScanStop::InvalidRoot)?;
        // All scratch growth is bounded before copying any path. Ancestor
        // frames retain prefix lengths, not a separate path allocation each.
        self.sections.extend_from_slice(sections);
        self.blocks.push(path[0]);
        for pair in rest.as_chunks::<2>().0 {
            let ancestor = match (block, pair[0]) {
                (Block::List { items, .. }, Step::ListItem { index }) => Some((
                    EntryOwner::List(
                        items
                            .get(index as usize)
                            .ok_or(ReferenceScanStop::InvalidRoot)?,
                    ),
                    index,
                )),
                (Block::DefinitionList { items, .. }, Step::DefinitionItem { index }) => Some((
                    EntryOwner::Definition(
                        items
                            .get(index as usize)
                            .ok_or(ReferenceScanStop::InvalidRoot)?,
                    ),
                    index,
                )),
                _ => None,
            };
            if let Some((owner, item)) = ancestor {
                let frame = OwnerFrame {
                    owner,
                    item,
                    blocks_len: self.blocks.len(),
                };
                self.owner = Some(frame);
                if owner.facts().is_some() {
                    self.semantic = Some(frame);
                }
            }
            let children = crate::content_location::block_children(block, pair[0])
                .ok_or(ReferenceScanStop::InvalidRoot)?;
            let Step::Block { index } = pair[1] else {
                return Err(ReferenceScanStop::InvalidRoot);
            };
            block = children
                .get(index as usize)
                .ok_or(ReferenceScanStop::InvalidRoot)?;
            self.blocks.extend_from_slice(pair);
        }
        Ok(block)
    }

    fn charge(
        &mut self,
        depth: usize,
        steps: usize,
        bytes: usize,
    ) -> Result<(), ReferenceScanStop> {
        self.budget.consume(depth, steps, bytes)
    }

    fn document(&mut self, document: &'ir Document) -> Result<(), ReferenceScanStop> {
        self.overview(document)?;
        self.section_array(&document.sections)
    }

    fn overview(&mut self, document: &'ir Document) -> Result<(), ReferenceScanStop> {
        self.charge(0, 1, 0)?;
        if let Some(heading) = &document.heading {
            self.charge(self.depth(), 1, 0)?;
            self.inlines(&heading.content, Root::DocumentHeading, heading.source)?;
        }
        self.block_array(&document.blocks)
    }

    fn section_array(&mut self, sections: &'ir [Section]) -> Result<(), ReferenceScanStop> {
        for (index, section) in sections.iter().enumerate() {
            self.charge(self.depth() + 1, 1, 0)?;
            self.sections.push(coordinate(index)?);
            self.section(section)?;
            self.sections.pop();
        }
        Ok(())
    }

    fn section(&mut self, section: &'ir Section) -> Result<(), ReferenceScanStop> {
        self.charge(self.depth(), 1, 0)?;
        self.inlines(
            &section.heading.content,
            Root::SectionHeading,
            section.heading.source,
        )?;
        self.block_array(&section.blocks)?;
        self.section_array(&section.children)
    }

    fn block_array(&mut self, blocks: &'ir [Block]) -> Result<(), ReferenceScanStop> {
        for (index, block) in blocks.iter().enumerate() {
            self.charge(self.depth() + 1, 1, 0)?;
            self.blocks.push(Step::Block {
                index: coordinate(index)?,
            });
            self.block(block)?;
            self.blocks.pop();
        }
        Ok(())
    }

    fn block(&mut self, block: &'ir Block) -> Result<(), ReferenceScanStop> {
        match block {
            Block::Paragraph {
                children, source, ..
            }
            | Block::Preformatted {
                children, source, ..
            } => {
                self.inlines(children, Root::Content(ContentInlineRoot::Inlines), *source)?;
            }
            Block::List { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    self.item(EntryOwner::List(item), coordinate(index)?)?;
                }
            }
            Block::DefinitionList { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    self.item(EntryOwner::Definition(item), coordinate(index)?)?;
                }
            }
            Block::Table { rows, .. } => {
                for (row, cells) in rows.iter().enumerate() {
                    self.charge(self.depth(), 1, 0)?;
                    for (column, cell) in cells.cells.iter().enumerate() {
                        self.charge(self.depth() + 1, 1, 0)?;
                        self.blocks.push(Step::TableCell {
                            row: coordinate(row)?,
                            column: coordinate(column)?,
                        });
                        self.block_array(&cell.blocks)?;
                        self.blocks.pop();
                    }
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
        Ok(())
    }

    fn item(&mut self, owner: EntryOwner<'ir>, index: u32) -> Result<(), ReferenceScanStop> {
        self.charge(self.depth() + 1, 1, 0)?;
        let previous = self.owner;
        let semantic = self.semantic;
        let frame = OwnerFrame {
            owner,
            blocks_len: self.blocks.len(),
            item: index,
        };
        self.owner = Some(frame);
        if owner.facts().is_some() {
            self.semantic = Some(frame);
        }
        if let EntryOwner::Definition(item) = owner {
            for (term, nodes) in item.terms.iter().enumerate() {
                self.charge(self.depth(), 1, 0)?;
                self.inlines(
                    nodes,
                    Root::Content(ContentInlineRoot::DefinitionTerm {
                        item_index: index,
                        term_index: coordinate(term)?,
                    }),
                    item.source,
                )?;
            }
        }
        self.blocks.push(match owner {
            EntryOwner::Definition(_) => Step::DefinitionItem { index },
            EntryOwner::List(_) => Step::ListItem { index },
        });
        self.block_array(owner.blocks())?;
        self.blocks.pop();
        self.owner = previous;
        self.semantic = semantic;
        Ok(())
    }

    fn inlines(
        &mut self,
        nodes: &'ir [Inline],
        root: Root,
        source: Option<SourceSpan>,
    ) -> Result<(), ReferenceScanStop> {
        for (index, node) in nodes.iter().enumerate() {
            self.charge(self.depth() + 1, 1, 0)?;
            self.path.push(coordinate(index)?);
            if let Inline::Link {
                target, children, ..
            } = node
            {
                self.charge(self.depth(), 0, target_bytes(target))?;
                // Summary does not inspect labels. A materializing callback
                // charges actual label work against this same budget.
                // Encoded-size validation walks the whole current position;
                // repeated links at a deep path must pay that work each time.
                self.charge(self.depth(), self.depth(), 0)?;
                let location = match root {
                    Root::DocumentHeading => {
                        ContentLocationRef::DocumentHeading { path: &self.path }
                    }
                    Root::SectionHeading => ContentLocationRef::SectionHeading {
                        sections: &self.sections,
                        path: &self.path,
                    },
                    Root::Content(root) => ContentLocationRef::Content {
                        sections: &self.sections,
                        blocks: &self.blocks,
                        root,
                        path: &self.path,
                    },
                };
                if location.encoded_len() > MAX_CONTENT_LOCATION_BYTES {
                    return Err(ReferenceScanStop::Position);
                }
                let owner = |frame: OwnerFrame<'ir>| ReferenceOwnerRef {
                    owner: frame.owner,
                    location: EntryOwnerLocationRef {
                        sections: &self.sections,
                        blocks: &self.blocks[..frame.blocks_len],
                        item_index: frame.item,
                    },
                };
                self.report.occurrences += 1;
                if (self.visit)(
                    LinkOccurrenceRef {
                        link: node,
                        target,
                        label: children,
                        location,
                        content_owner: self.owner.map(owner),
                        semantic_owner: self.semantic.map(owner),
                        source: source
                            .or_else(|| self.owner.and_then(|frame| frame.owner.source())),
                    },
                    &mut self.budget,
                )
                .is_break()
                {
                    return Err(ReferenceScanStop::Visitor);
                }
            }
            if let Some(children) = crate::content_location::inline_children(node) {
                self.inlines(children, root, source)?;
            }
            self.path.pop();
        }
        Ok(())
    }
}

fn coordinate(index: usize) -> Result<u32, ReferenceScanStop> {
    u32::try_from(index).map_err(|_| ReferenceScanStop::Position)
}

fn target_bytes(target: &LinkTarget) -> usize {
    match target {
        LinkTarget::External { uri } => uri.len(),
        LinkTarget::Email { address } => address.len(),
        LinkTarget::Document { name, fragment } => name
            .len()
            .saturating_add(fragment.as_ref().map_or(0, String::len)),
        LinkTarget::Manual {
            name,
            manual_section,
        } => name
            .len()
            .saturating_add(manual_section.as_ref().map_or(0, String::len)),
        LinkTarget::Section { id } => id.as_str().len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn link(name: &str) -> Value {
        json!({"type":"link","target":{"kind":"document","name":name},"children":[{"type":"text","value":name}]})
    }
    fn paragraph(children: Vec<Value>) -> Value {
        let mut value = json!({"type":"paragraph"});
        value["children"] = children.into();
        value
    }
    fn document(blocks: Vec<Value>, sections: Vec<Value>) -> Document {
        let mut value = json!({"parser":null,"source":{"format":"markdown"},"meta":{}});
        value["blocks"] = blocks.into();
        value["sections"] = sections.into();
        serde_json::from_value(value).unwrap()
    }
    fn facts(id: &str) -> Value {
        json!({"id":id,"kind":{"kind":"term"},"case":"sensitive","names":[],"forms":[],"valueDomain":null})
    }

    #[test]
    fn all_content_roots_and_repeated_links_rebuild_after_serde() {
        let mut document = document(
            vec![
                paragraph(vec![link("root"), link("same"), link("same")]),
                json!({"type":"preformatted","children":[link("pre")]}),
                json!({"type":"list","kind":{"kind":"bullet"},"items":[{"blocks":[paragraph(vec![link("item")])]}]}),
                json!({"type":"definition-list","items":[{"entry":null,"terms":[[link("term")]],"description":[paragraph(vec![link("description")])]}]}),
                json!({"type":"table","rows":[{"cells":[{"blocks":[paragraph(vec![link("cell")])]}]}]}),
            ],
            vec![
                json!({"id":"section","heading":{"content":[link("heading")]},"blocks":[paragraph(vec![link("section")])],"children":[
                    {"id":"child","heading":{"content":[link("child-heading")]},"blocks":[],"children":[]}
                ]}),
            ],
        );
        document.heading = Some(crate::Heading {
            content: vec![serde_json::from_value(link("document-heading")).unwrap()],
            source: None,
        });
        let collect = |document: &Document| {
            let mut locations = Vec::new();
            let report = scan_references(document, ReferenceScanLimits::default(), |occurrence| {
                let location = occurrence.location.to_owned().unwrap();
                assert!(std::ptr::eq(
                    location.resolve_link(document).unwrap(),
                    occurrence.link
                ));
                locations.push((location, occurrence.target.clone()));
                ControlFlow::Continue(())
            });
            assert!(report.complete(), "{report:?}");
            assert_eq!(report.occurrences, locations.len());
            locations
        };
        let expected = collect(&document);
        assert_eq!(expected.len(), 12);
        assert_ne!(expected[2].0, expected[3].0);
        assert_eq!(expected[2].1, expected[3].1);
        let reparsed = serde_json::from_slice(&serde_json::to_vec(&document).unwrap()).unwrap();
        assert_eq!(collect(&reparsed), expected);
        let mut section_names = Vec::new();
        let report = scan_section_references(
            &document,
            &[0],
            ReferenceScanLimits::default(),
            |occurrence| {
                section_names.push(occurrence.target.clone());
                ControlFlow::Continue(())
            },
        );
        assert!(report.complete());
        assert_eq!(section_names.len(), 3);
        assert_eq!(
            scan_section_references(
                &document,
                &[99],
                ReferenceScanLimits::default(),
                |_| unreachable!()
            )
            .stopped,
            Some(ReferenceScanStop::InvalidRoot)
        );
    }

    #[test]
    fn transparent_items_keep_nearest_semantic_owner_without_copying_forms() {
        let document = document(
            vec![json!({"type":"list","kind":{"kind":"bullet"},"items":[{
                "entry":facts("outer"),"blocks":[
                    paragraph(vec![link("outer")]),
                    {"type":"table","rows":[{"cells":[{"blocks":[
                        {"type":"list","kind":{"kind":"bullet"},"items":[{"blocks":[paragraph(vec![link("transparent")])]}]},
                        {"type":"definition-list","items":[{"entry":facts("child"),"terms":[[link("child")]],"description":[]}]}
                    ]}]}]}
                ]
            }]})],
            vec![],
        );
        let mut names = Vec::new();
        let report = scan_references(&document, ReferenceScanLimits::default(), |occurrence| {
            let semantic = occurrence.semantic_owner.unwrap();
            let resolved = semantic.location.resolve(&document).unwrap();
            assert_eq!(
                resolved.facts().unwrap().id,
                semantic.owner.facts().unwrap().id
            );
            names.push(semantic.owner.facts().unwrap().id.to_string());
            if names.len() == 2 {
                assert!(occurrence.content_owner.unwrap().owner.facts().is_none());
            }
            ControlFlow::Continue(())
        });
        assert!(report.complete());
        assert_eq!(names, ["outer", "outer", "child"]);
    }

    #[test]
    fn explicit_owner_and_block_roots_preserve_ancestor_context_and_exclude_siblings() {
        let document = document(
            vec![json!({"type":"list","kind":{"kind":"bullet"},"items":[
                {"entry":facts("outer"),"blocks":[{"type":"list","kind":{"kind":"bullet"},"items":[{"blocks":[paragraph(vec![link("nested")])]}]}]},
                {"blocks":[paragraph(vec![link("sibling")])]}
            ]})],
            vec![
                json!({"id":"section","heading":{"content":[]},"blocks":[paragraph(vec![link("excluded-section")])],"children":[]}),
            ],
        );
        let mut count = 0;
        let report = scan_reference_scope(
            &document,
            ReferenceScope::Overview,
            ReferenceScanLimits::default(),
            |_, _| {
                count += 1;
                ControlFlow::Continue(())
            },
        );
        assert!(report.complete());
        assert_eq!(count, 2);
        let path = [
            Step::Block { index: 0 },
            Step::ListItem { index: 0 },
            Step::Block { index: 0 },
        ];
        let owner = EntryOwnerLocationRef {
            sections: &[],
            blocks: &path,
            item_index: 0,
        };
        let report = scan_owner_references(
            &document,
            owner,
            ReferenceScanLimits::default(),
            |occurrence| {
                assert_eq!(
                    occurrence
                        .semantic_owner
                        .unwrap()
                        .owner
                        .facts()
                        .unwrap()
                        .id
                        .as_str(),
                    "outer"
                );
                assert!(occurrence.content_owner.unwrap().owner.facts().is_none());
                assert!(std::ptr::eq(
                    occurrence.location.resolve_link(&document).unwrap(),
                    occurrence.link
                ));
                ControlFlow::Continue(())
            },
        );
        assert!(report.complete());
        assert_eq!(report.occurrences, 1);
        let report = scan_block_references(
            &document,
            &[],
            &path,
            ReferenceScanLimits::default(),
            |occurrence| {
                assert_eq!(
                    occurrence
                        .semantic_owner
                        .unwrap()
                        .owner
                        .facts()
                        .unwrap()
                        .id
                        .as_str(),
                    "outer"
                );
                ControlFlow::Continue(())
            },
        );
        assert!(report.complete());
        assert_eq!(report.occurrences, 1);
    }

    #[test]
    fn callback_stop_and_zero_budget_do_not_inspect_later_nodes() {
        let document = document(
            vec![paragraph(vec![link("first"), link(&"x".repeat(1_000_000))])],
            vec![],
        );
        let report = scan_references(
            &document,
            ReferenceScanLimits {
                bytes: 10,
                ..Default::default()
            },
            |_| ControlFlow::Break(()),
        );
        assert_eq!(report.stopped, Some(ReferenceScanStop::Visitor));
        assert_eq!(report.occurrences, 1);
        assert_eq!(report.bytes, 5);
        let report = scan_references(
            &document,
            ReferenceScanLimits {
                steps: 0,
                ..Default::default()
            },
            |_| panic!("zero work budget called visitor"),
        );
        assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
        assert_eq!(report.steps, 0);
        let report = scan_references(
            &document,
            ReferenceScanLimits {
                bytes: 4,
                ..Default::default()
            },
            |_| panic!("target inspection exceeded its byte budget"),
        );
        assert_eq!(report.stopped, Some(ReferenceScanStop::Bytes));
        assert_eq!(report.occurrences, 0);
    }

    #[test]
    fn empty_labels_are_occurrences_and_depth_is_bounded_before_callback() {
        let empty =
            json!({"type":"link","target":{"kind":"document","name":"empty"},"children":[]});
        let document = document(vec![paragraph(vec![empty])], vec![]);
        let report = scan_references(
            &document,
            ReferenceScanLimits {
                bytes: 5,
                ..Default::default()
            },
            |occurrence| {
                assert!(occurrence.label.is_empty());
                assert!(occurrence.location.resolve_link(&document).is_some());
                ControlFlow::Continue(())
            },
        );
        assert!(report.complete());
        assert_eq!(report.occurrences, 1);
        let report = scan_references(
            &document,
            ReferenceScanLimits {
                depth: 1,
                ..Default::default()
            },
            |_| unreachable!(),
        );
        assert_eq!(report.stopped, Some(ReferenceScanStop::Depth));
        assert_eq!(report.occurrences, 0);
    }

    #[test]
    fn callback_budget_failure_remains_visible_even_at_end_or_explicit_stop() {
        let document = document(
            vec![paragraph(vec![
                json!({"type":"link","target":{"kind":"document","name":"target"},"children":[]}),
            ])],
            vec![],
        );
        for stop in [false, true] {
            let report = scan_reference_scope(
                &document,
                ReferenceScope::Document,
                ReferenceScanLimits::default(),
                |_, budget| {
                    assert_eq!(
                        budget.consume(0, budget.remaining_steps() + 1, 0),
                        Err(ReferenceScanStop::Steps)
                    );
                    assert_eq!(budget.consume(0, 0, 0), Err(ReferenceScanStop::Steps));
                    if stop {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                },
            );
            assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
            assert_eq!(report.occurrences, 1);
        }
    }

    #[test]
    fn nested_links_are_distinct_original_occurrences() {
        let nested = json!({"type":"link","target":{"kind":"document","name":"outer"},"children":[{"type":"strong","children":[link("inner")]}]});
        let document = document(vec![paragraph(vec![nested])], vec![]);
        let mut names = Vec::new();
        let report = scan_references(&document, ReferenceScanLimits::default(), |occurrence| {
            if let LinkTarget::Document { name, .. } = occurrence.target {
                names.push(name.clone());
            }
            assert!(std::ptr::eq(
                occurrence.location.resolve_link(&document).unwrap(),
                occurrence.link
            ));
            ControlFlow::Continue(())
        });
        assert!(report.complete());
        assert_eq!(names, ["outer", "inner"]);
    }

    #[test]
    fn thousands_of_repeated_targets_are_bounded_by_scan_not_unique_count() {
        let document = document(
            vec![paragraph((0..10_000).map(|_| link("same")).collect())],
            vec![],
        );
        let report = scan_references(
            &document,
            ReferenceScanLimits {
                steps: 100,
                ..Default::default()
            },
            |_| ControlFlow::Continue(()),
        );
        assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
        assert!(report.occurrences > 0 && report.occurrences < 100);
        assert!(report.steps <= 100);
    }

    #[test]
    fn repeated_deep_locations_charge_coordinate_inspection_before_callback() {
        let mut nested = paragraph((0..100).map(|_| link("same")).collect());
        for _ in 0..20 {
            nested = json!({"type":"list","kind":{"kind":"bullet"},"items":[{"blocks":[nested]}]});
        }
        let document = document(vec![nested], vec![]);
        let mut inspected_coordinates = 0;
        let report = scan_references(
            &document,
            ReferenceScanLimits {
                steps: 200,
                ..Default::default()
            },
            |occurrence| {
                inspected_coordinates += occurrence.location.depth();
                ControlFlow::Continue(())
            },
        );
        assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
        assert!(report.occurrences > 0 && report.occurrences < 4);
        assert!(inspected_coordinates <= report.steps);
        assert!(report.steps <= 200);
    }
}
