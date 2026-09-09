//! Source-order traversal with bounded paths and nearest-owner frames.
use super::{
    ContentRevealRef, EntrySetReferenceRef, LinkOccurrenceRef, NavigationEvent,
    NavigationScanOptions, NavigationTargetRef, ReferenceOwnerRef, ReferenceScanLimits,
    ReferenceScanReport, ReferenceScanStop, ReferenceScope, ReferenceWorkBudget,
};
use crate::{
    Block, ContentBlockStep as Step, ContentInlineRoot, ContentLocationRef, Document, EntryOwner,
    EntryOwnerLocationRef, Inline, LinkTarget, MAX_CONTENT_LOCATION_BYTES, Section, SourceSpan,
};
use std::ops::ControlFlow;

mod owners;
mod roots;
use owners::OwnerFrames;

pub(super) fn scan<'ir>(
    document: &'ir Document,
    scope: ReferenceScope<'_>,
    budget: &mut ReferenceWorkBudget,
    options: NavigationScanOptions,
    visit: impl for<'path> FnMut(
        NavigationEvent<'ir, 'path>,
        &mut ReferenceWorkBudget,
    ) -> ControlFlow<()>,
) -> ReferenceScanReport {
    let pending = std::mem::replace(
        budget,
        ReferenceWorkBudget::new(ReferenceScanLimits {
            steps: 0,
            depth: 0,
            bytes: 0,
        }),
    );
    let mut scan = Scan::new(pending, options, visit);
    let result = scan.scope(document, scope);
    let report = scan.finish(result);
    *budget = scan.budget;
    report
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

#[derive(Clone, Copy)]
enum TargetSite<'a> {
    Document,
    Section,
    Owner(OwnerFrame<'a>),
    Inline(Root),
}

struct Scan<'ir, F> {
    budget: ReferenceWorkBudget,
    report: ReferenceScanReport,
    visit: F,
    options: NavigationScanOptions,
    sections: Vec<u32>,
    blocks: Vec<Step>,
    path: Vec<u32>,
    owners: OwnerFrames<'ir>,
}

impl<'ir, F> Scan<'ir, F>
where
    F: for<'path> FnMut(NavigationEvent<'ir, 'path>, &mut ReferenceWorkBudget) -> ControlFlow<()>,
{
    fn new(budget: ReferenceWorkBudget, options: NavigationScanOptions, visit: F) -> Self {
        Self {
            budget,
            report: ReferenceScanReport::default(),
            visit,
            options,
            sections: Vec::new(),
            blocks: Vec::new(),
            path: Vec::new(),
            owners: OwnerFrames::default(),
        }
    }

    fn finish(&mut self, result: Result<(), ReferenceScanStop>) -> ReferenceScanReport {
        self.report.stopped = self.budget.stopped().or(result.err());
        self.report.steps = self.budget.used_steps();
        self.report.bytes = self.budget.used_bytes();
        self.report
    }

    fn depth(&self) -> usize {
        self.sections.len() + self.blocks.len() + self.path.len()
    }

    fn charge(
        &mut self,
        depth: usize,
        steps: usize,
        bytes: usize,
    ) -> Result<(), ReferenceScanStop> {
        self.budget.consume(depth, steps, bytes)
    }

    fn target(
        &mut self,
        id: &'ir crate::NodeId,
        aliases: &'ir [crate::FragmentAlias],
        site: TargetSite<'ir>,
    ) -> Result<(), ReferenceScanStop> {
        self.charge(
            self.depth(),
            self.depth().saturating_add(1),
            id.as_str().len(),
        )?;
        for alias in aliases {
            self.charge(self.depth(), 1, alias.as_str().len())?;
        }
        let reveal = match site {
            TargetSite::Document => ContentRevealRef::Document,
            TargetSite::Section => ContentRevealRef::Section(&self.sections),
            TargetSite::Owner(frame) => ContentRevealRef::Owner(EntryOwnerLocationRef {
                sections: &self.sections,
                blocks: &self.blocks[..frame.blocks_len],
                item_index: frame.item,
            }),
            TargetSite::Inline(root) => ContentRevealRef::Inline(match root {
                Root::DocumentHeading => ContentLocationRef::DocumentHeading { path: &self.path },
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
            }),
        };
        if (self.visit)(
            NavigationEvent::Target(NavigationTargetRef {
                id,
                aliases,
                reveal,
            }),
            &mut self.budget,
        )
        .is_break()
        {
            return Err(ReferenceScanStop::Visitor);
        }
        Ok(())
    }

    fn document(&mut self, document: &'ir Document) -> Result<(), ReferenceScanStop> {
        self.overview(document)?;
        self.section_array(&document.sections)
    }

    fn overview(&mut self, document: &'ir Document) -> Result<(), ReferenceScanStop> {
        self.charge(0, 1, 0)?;
        if self.options.targets
            && (document.heading.is_some()
                || !document.blocks.is_empty()
                || !document.fragment_aliases.is_empty())
        {
            static ROOT: std::sync::LazyLock<crate::NodeId> =
                std::sync::LazyLock::new(|| crate::NodeId::from(crate::DOCUMENT_ROOT_ID));
            self.target(&ROOT, &document.fragment_aliases, TargetSite::Document)?;
        }
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
        if self.options.targets {
            self.target(&section.id, &section.fragment_aliases, TargetSite::Section)?;
        }
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
        let frame = OwnerFrame {
            owner,
            blocks_len: self.blocks.len(),
            item: index,
        };
        let previous = self.owners.enter(frame);
        if self.options.targets
            && let Some(facts) = owner.facts()
        {
            self.target(&facts.id, &[], TargetSite::Owner(frame))?;
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
        if self.options.entry_sets
            && let Some(crate::ValueDomain::EntrySet {
                reference, source, ..
            }) = owner.facts().and_then(|facts| facts.value_domain.as_ref())
        {
            let bytes = match reference {
                crate::DocumentReference::Document { name, fragment } => name
                    .len()
                    .saturating_add(fragment.as_ref().map_or(0, String::len)),
                crate::DocumentReference::Manual {
                    name,
                    manual_section,
                } => name
                    .len()
                    .saturating_add(manual_section.as_ref().map_or(0, String::len)),
            };
            self.charge(self.depth(), self.depth().saturating_add(1), bytes)?;
            let relation = EntrySetReferenceRef {
                reference,
                owner: ReferenceOwnerRef {
                    owner,
                    location: EntryOwnerLocationRef {
                        sections: &self.sections,
                        blocks: &self.blocks,
                        item_index: index,
                    },
                },
                source: *source,
            };
            if (self.visit)(NavigationEvent::EntrySet(relation), &mut self.budget).is_break() {
                return Err(ReferenceScanStop::Visitor);
            }
        }
        self.owners.restore(previous);
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
            if self.options.targets
                && let Inline::Anchor {
                    id,
                    fragment_aliases,
                    ..
                } = node
            {
                let site = self
                    .owners
                    .semantic()
                    .filter(|frame| frame.owner.facts().is_some_and(|facts| facts.id == *id))
                    .map_or(TargetSite::Inline(root), TargetSite::Owner);
                self.target(id, fragment_aliases, site)?;
            }
            if let Inline::Link {
                target, children, ..
            } = node
                && self.options.links.contains(target)
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
                    NavigationEvent::Link(LinkOccurrenceRef {
                        link: node,
                        target,
                        label: children,
                        location,
                        content_owner: self.owners.content().map(owner),
                        semantic_owner: self.owners.semantic().map(owner),
                        source: source.or_else(|| {
                            self.owners.content().and_then(|frame| frame.owner.source())
                        }),
                    }),
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
