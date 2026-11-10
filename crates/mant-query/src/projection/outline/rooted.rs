//! Resolve one outline root before materializing its DTOs or scanning references.

use super::{
    DOCUMENT_ROOT_ID, EntryProjection, EntrySummary, OutlineNode, OutlinePath, ProjectionError,
    ResolvedContent, SemanticEntry, TLDR_ID, borrowed_summary, entry_node, overview_node,
    record_projected_summary, tldr_node,
};
use crate::selectors::{
    DocumentSelectorIndex, LocatedNode, collect_selection_root_entries, collect_selection_sections,
};
use mant_ir::{Block, Document, DocumentAddress, EntryOwnerLocationRef, NodeId, ReferenceScope};
use mant_protocol::{ContentSelector, ReferenceInventory, ReferenceProjection};
use std::{cell::OnceCell, collections::BTreeSet};

enum Target {
    Tldr,
    Overview,
    Located(usize),
}

pub(super) struct RootedOutline<'ir> {
    target: Target,
    // Source-ordered borrowed locations. This full lightweight scan is needed
    // for global ID ambiguity, including collisions with the synthetic roots.
    locations: Vec<LocatedNode<'ir>>,
}

impl<'ir> RootedOutline<'ir> {
    pub(super) fn resolve(
        query: &'ir ResolvedContent,
        selector: &ContentSelector,
    ) -> Result<Self, ProjectionError> {
        let mut locations = Vec::new();
        if let Some(document) = &query.document {
            collect_selection_root_entries(&document.blocks, &mut locations);
            collect_selection_sections(&document.sections, &mut locations);
        }
        let index = DocumentSelectorIndex::new(&locations);
        let matches = super::super::excerpt::selector_matches;
        let target = if query.tldr.is_some() && matches(selector, &OutlinePath::Tldr, TLDR_ID) {
            index.validate_synthetic_identity(&query.label, selector, TLDR_ID, "0")?;
            Target::Tldr
        } else if query
            .document
            .as_ref()
            .is_some_and(|document| document.heading.is_some() || !document.blocks.is_empty())
            && matches(selector, &OutlinePath::DocumentRoot, DOCUMENT_ROOT_ID)
        {
            index.validate_synthetic_identity(&query.label, selector, DOCUMENT_ROOT_ID, "root")?;
            Target::Overview
        } else {
            Target::Located(index.resolve(&query.label, selector)?.order())
        };
        Ok(Self { target, locations })
    }

    pub(super) fn project(
        &self,
        query: &ResolvedContent,
        entries: &EntryProjection,
    ) -> OutlineNode {
        if matches!(self.target, Target::Tldr) {
            return tldr_node();
        }
        let document = query.document.as_ref().expect("resolved document content");
        let projection = Projection {
            document,
            address: query.address.as_ref(),
            entries,
            rejected_aliases: OnceCell::new(),
        };
        match self.target {
            Target::Tldr => unreachable!("tldr was already projected"),
            Target::Overview => {
                let count = self
                    .locations
                    .partition_point(|node| node.coordinates().is_empty());
                let children = projection.entry_list(&self.locations[..count]);
                overview_node(projection.summary(&document.blocks, &children), children)
            }
            Target::Located(index) => {
                let mut subtree = &self.locations[index..];
                if subtree[0].is_section() {
                    projection.section(&mut subtree)
                } else {
                    projection
                        .entry(&mut subtree, true)
                        .expect("explicit entry root")
                }
            }
        }
    }

    pub(super) fn references(
        &self,
        query: &ResolvedContent,
        policy: &ReferenceProjection,
    ) -> ReferenceInventory {
        let scan = |scope| {
            let document = query.document.as_ref().expect("resolved document content");
            super::super::project_references(document, query.address.as_ref(), scope, policy)
        };
        match self.target {
            Target::Tldr => ReferenceInventory::not_scanned(policy.clone()),
            Target::Overview => scan(ReferenceScope::Overview),
            Target::Located(index) => {
                let selected = &self.locations[index];
                let sections = content_sections(selected);
                match selected {
                    LocatedNode::Section { .. } => scan(ReferenceScope::Section(&sections)),
                    LocatedNode::Entry { entry, .. } => {
                        scan(ReferenceScope::Owner(EntryOwnerLocationRef {
                            sections: &sections,
                            blocks: entry.block_path(),
                            item_index: u32::try_from(entry.item_index())
                                .expect("addressable item"),
                        }))
                    }
                }
            }
        }
    }
}

fn content_sections(node: &LocatedNode<'_>) -> Vec<u32> {
    node.coordinates()
        .iter()
        .map(|coordinate| u32::try_from(coordinate - 1).expect("addressable section"))
        .collect()
}

struct Projection<'a> {
    document: &'a Document,
    address: Option<&'a DocumentAddress>,
    entries: &'a EntryProjection,
    rejected_aliases: OnceCell<BTreeSet<NodeId>>,
}

impl Projection<'_> {
    fn section(&self, remaining: &mut &[LocatedNode<'_>]) -> OutlineNode {
        let (node, rest) = remaining.split_first().expect("section root");
        let LocatedNode::Section { section, .. } = node else {
            unreachable!("section traversal ends after its entries")
        };
        let coordinates = node.coordinates();
        let entry_count = rest.partition_point(|item| item.coordinates() == coordinates);
        let (entries, sections) = rest.split_at(entry_count);
        *remaining = sections;
        let mut children = self.entry_list(entries);
        let entry_summary = self.summary(&section.blocks, &children);
        while remaining
            .first()
            .is_some_and(|child| child.coordinates().starts_with(coordinates))
        {
            children.push(self.section(remaining));
        }
        OutlineNode::DocumentSection {
            path: node.path().to_string().into(),
            id: section.id.clone(),
            title: section.heading.single_line_text(),
            entry_summary,
            children,
        }
    }

    fn entry_list(&self, mut remaining: &[LocatedNode<'_>]) -> Vec<OutlineNode> {
        if matches!(
            self.entries,
            EntryProjection::None | EntryProjection::Summary
        ) {
            return Vec::new();
        }
        let mut output = Vec::new();
        while !remaining.is_empty() {
            output.extend(self.entry(&mut remaining, false));
        }
        output
    }

    fn entry(&self, remaining: &mut &[LocatedNode<'_>], explicit: bool) -> Option<OutlineNode> {
        let (node, rest) = remaining.split_first()?;
        let LocatedNode::Entry { entry, .. } = node else {
            unreachable!("entry traversal contains only entries from one section")
        };
        let count = rest.partition_point(|candidate| {
            matches!(candidate, LocatedNode::Entry { entry: child, .. }
                if candidate.coordinates() == node.coordinates()
                    && child.indices().starts_with(entry.indices()))
        });
        let (descendants, rest) = rest.split_at(count);
        *remaining = rest;
        let children = self.entry_list(descendants);
        let item = entry.owner();
        let kind = item.facts().expect("located semantic owner").kind;
        if !explicit
            && children.is_empty()
            && matches!(self.entries, EntryProjection::Kinds { kinds } if !kinds.contains(&kind))
        {
            return None;
        }
        let mut metadata = SemanticEntry::from_owner_shallow(item).expect("located semantic owner");
        if metadata.alias_of.is_some() && self.rejected_aliases().contains(&metadata.id) {
            metadata.alias_of = None;
        }
        Some(entry_node(
            &metadata,
            node.path(),
            mant_ir::ContentReveal::Owner {
                sections: content_sections(node),
                blocks: entry.block_path().to_vec(),
                item_index: u32::try_from(entry.item_index()).expect("addressable item"),
            },
            self.address,
            self.summary(item.blocks(), &children),
            children,
        ))
    }

    fn summary(&self, blocks: &[Block], children: &[OutlineNode]) -> Option<EntrySummary> {
        fn collect(
            nodes: &[OutlineNode],
            projection: &EntryProjection,
            direct: bool,
            summary: &mut EntrySummary,
        ) {
            for node in nodes {
                if let OutlineNode::DocumentEntry {
                    entry_kind,
                    forms,
                    children,
                    ..
                } = node
                {
                    if !matches!(projection, EntryProjection::Kinds { kinds } if !kinds.contains(entry_kind))
                    {
                        record_projected_summary(summary, *entry_kind, forms.len(), direct);
                    }
                    collect(children, projection, false, summary);
                }
            }
        }
        if matches!(
            self.entries,
            EntryProjection::None | EntryProjection::Summary
        ) {
            return borrowed_summary(blocks, self.entries);
        }
        let mut summary = EntrySummary::default();
        collect(children, self.entries, true, &mut summary);
        (!summary.is_empty()).then_some(summary)
    }

    fn rejected_aliases(&self) -> &BTreeSet<NodeId> {
        self.rejected_aliases.get_or_init(|| {
            mant_ir::entry_relation_issues(self.document)
                .into_iter()
                .filter(|issue| {
                    matches!(
                        issue.kind,
                        mant_ir::EntryRelationIssueKind::AliasOf
                            | mant_ir::EntryRelationIssueKind::Cycle
                    )
                })
                .map(|issue| issue.owner)
                .collect()
        })
    }
}
