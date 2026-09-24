//! Rebuild an operation-local semantic index from finalized identities.
use super::{
    model::{EntrySummary, SemanticEntry},
    walk::{owner_child_step, visit_child_entry_locations},
};
use crate::{
    Block, ContentContext, ContentReadError, Document, DocumentBodyRef, EntryOwner, EntryOwnerView,
    FixedBody, NodeId,
};
use std::{collections::BTreeMap, num::NonZeroU32};

/// Rebuildable semantic index for the document root and every section.
///
/// The index is a derived navigation sidecar. Flow items or Fixed owner marks
/// and their facts remain in the [`Document`] body, so callers may rebuild it
/// after a trusted document transformation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticIndex {
    root: Vec<SemanticEntry>,
    sections: BTreeMap<Vec<usize>, Vec<SemanticEntry>>,
    section_paths: BTreeMap<NodeId, Option<Vec<usize>>>,
    owner_locations: BTreeMap<crate::OutlinePath, crate::ContentReveal>,
}

impl SemanticIndex {
    /// Build the semantic index from finalized facts on either content shape.
    #[must_use]
    pub fn build(document: &Document) -> Self {
        let flow = match document.body() {
            DocumentBodyRef::Flow(flow) => flow,
            DocumentBodyRef::Fixed(fixed) => return Self::build_fixed(fixed),
        };
        let content = document.content();
        let mut owner_locations = BTreeMap::new();
        let root =
            entries_with_locations(content, flow.blocks, &[], &[], &[], &mut owner_locations);
        let mut sections = BTreeMap::new();
        let mut section_paths = BTreeMap::new();
        collect_section_entries(
            content,
            flow.sections,
            &[],
            &mut sections,
            &mut section_paths,
            &mut owner_locations,
        );
        let mut result = Self {
            root,
            sections,
            section_paths,
            owner_locations,
        };
        if result
            .root
            .iter()
            .chain(result.sections.values().flatten())
            .any(has_alias_of)
        {
            let rejected = crate::entry_relation_issues(document)
                .into_iter()
                .filter(|issue| {
                    matches!(
                        issue.kind,
                        crate::EntryRelationIssueKind::AliasOf
                            | crate::EntryRelationIssueKind::Cycle
                    )
                })
                .map(|issue| issue.owner)
                .collect::<std::collections::BTreeSet<_>>();
            clear_rejected_relations(&mut result.root, &rejected);
            for entries in result.sections.values_mut() {
                clear_rejected_relations(entries, &rejected);
            }
        }
        result
    }

    fn build_fixed(fixed: &FixedBody) -> Self {
        // The final surface is authoritative. Only validated semantic facts
        // attached to its native owner become entries; owner keys and parent
        // edges never invent another Flow tree.
        let mut result = Self::default();
        let mut heading_paths = Vec::<Vec<usize>>::with_capacity(fixed.headings.len());
        let mut heading_child_counts = vec![0usize; fixed.headings.len() + 1];
        for heading in &fixed.headings {
            let parent = heading.parent.map_or(0, |key| key.get() as usize);
            let sibling = heading_child_counts[parent];
            heading_child_counts[parent] += 1;
            let mut path = heading.parent.map_or_else(Vec::new, |key| {
                heading_paths[(key.get() - 1) as usize].clone()
            });
            path.push(sibling);
            result
                .section_paths
                .entry(heading.id.clone())
                .and_modify(|existing| *existing = None)
                .or_insert_with(|| Some(path.clone()));
            result.sections.insert(path.clone(), Vec::new());
            heading_paths.push(path);
        }

        let mut candidates = Vec::<FixedCandidate>::new();
        let mut nearest_entry: Vec<Option<usize>> = vec![None; fixed.owners.len() + 1];
        let mut root_counts = BTreeMap::<Option<NonZeroU32>, usize>::new();
        for owner in &fixed.owners {
            let parent = owner
                .parent
                .and_then(|key| nearest_entry[key.get() as usize]);
            let Some(entry) = EntryOwnerView::fixed(fixed, owner)
                .and_then(|view| view.semantic_entry().ok().flatten())
            else {
                nearest_entry[owner.key.get() as usize] = parent;
                continue;
            };
            let parent = parent.filter(|&index| candidates[index].section == owner.section);
            let next = if let Some(index) = parent {
                let next = candidates[index].child_count;
                candidates[index].child_count += 1;
                next
            } else {
                let counter = root_counts.entry(owner.section).or_default();
                let next = *counter;
                *counter += 1;
                next
            };
            let mut entry_indices =
                parent.map_or_else(Vec::new, |index| candidates[index].entry_indices.clone());
            entry_indices.push(next + 1);
            let section_path = owner.section.map(|key| {
                heading_paths[(key.get() - 1) as usize]
                    .iter()
                    .map(|coordinate| coordinate + 1)
                    .collect::<Vec<_>>()
            });
            if let Some(path) =
                crate::OutlinePath::nested_entry(section_path.as_deref(), &entry_indices)
            {
                result
                    .owner_locations
                    .insert(path, crate::ContentReveal::FixedOwner { key: owner.key });
            }
            let index = candidates.len();
            candidates.push(FixedCandidate {
                section: owner.section,
                parent,
                child_count: 0,
                entry_indices,
                entry: Some(entry),
            });
            nearest_entry[owner.key.get() as usize] = Some(index);
        }
        let mut roots = BTreeMap::<Option<NonZeroU32>, Vec<SemanticEntry>>::new();
        for index in (0..candidates.len()).rev() {
            let mut entry = candidates[index].entry.take().expect("unassembled entry");
            entry.children.reverse();
            if let Some(parent) = candidates[index].parent {
                candidates[parent]
                    .entry
                    .as_mut()
                    .expect("parent precedes child")
                    .children
                    .push(entry);
            } else {
                roots
                    .entry(candidates[index].section)
                    .or_default()
                    .push(entry);
            }
        }
        for (scope, mut entries) in roots {
            entries.reverse();
            if let Some(section) = scope {
                let path = &heading_paths[(section.get() - 1) as usize];
                result.sections.insert(path.clone(), entries);
            } else {
                result.root = entries;
            }
        }
        result
    }

    /// Entries directly owned by content before the first section.
    #[must_use]
    pub fn root(&self) -> &[SemanticEntry] {
        &self.root
    }

    /// Entries directly owned by a uniquely identified section.
    ///
    /// Missing or duplicate IDs return no entries. Use [`Self::section_at`] to
    /// address a specific source owner even when public IR has duplicate IDs.
    #[must_use]
    pub fn section(&self, id: &str) -> &[SemanticEntry] {
        self.section_paths
            .get(id)
            .and_then(Option::as_deref)
            .map_or(&[], |path| self.section_at(path))
    }

    /// Entries owned by an exact section at zero-based source-tree coordinates.
    #[must_use]
    pub fn section_at(&self, path: &[usize]) -> &[SemanticEntry] {
        self.sections.get(path).map_or(&[], Vec::as_slice)
    }

    /// Exact original item for one indexed semantic path, independent of IDs.
    /// Built together with entries; querying it does not rescan document content.
    #[must_use]
    pub fn owner_at(&self, path: &crate::OutlinePath) -> Option<&crate::ContentReveal> {
        self.owner_locations.get(path)
    }

    /// Summary for content before the first section.
    #[must_use]
    pub fn root_summary(&self) -> EntrySummary {
        EntrySummary::for_entries(&self.root)
    }

    /// Summary for one section without including child sections.
    #[must_use]
    pub fn section_summary(&self, id: &str) -> EntrySummary {
        EntrySummary::for_entries(self.section(id))
    }
}

struct FixedCandidate {
    section: Option<NonZeroU32>,
    parent: Option<usize>,
    child_count: usize,
    entry_indices: Vec<usize>,
    entry: Option<SemanticEntry>,
}

fn has_alias_of(entry: &SemanticEntry) -> bool {
    entry.alias_of.is_some() || entry.children.iter().any(has_alias_of)
}

fn clear_rejected_relations(
    entries: &mut [SemanticEntry],
    rejected: &std::collections::BTreeSet<NodeId>,
) {
    for entry in entries {
        if rejected.contains(&entry.id) {
            entry.alias_of = None;
        }
        clear_rejected_relations(&mut entry.children, rejected);
    }
}

fn collect_section_entries(
    content: ContentContext<'_>,
    sections: &[crate::Section],
    parent: &[usize],
    output: &mut BTreeMap<Vec<usize>, Vec<SemanticEntry>>,
    paths: &mut BTreeMap<NodeId, Option<Vec<usize>>>,
    owners: &mut BTreeMap<crate::OutlinePath, crate::ContentReveal>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut path = parent.to_vec();
        path.push(index);
        paths
            .entry(section.id.clone())
            .and_modify(|path| *path = None)
            .or_insert_with(|| Some(path.clone()));
        output.insert(
            path.clone(),
            entries_with_locations(content, &section.blocks, &path, &[], &[], owners),
        );
        collect_section_entries(content, &section.children, &path, output, paths, owners);
    }
}

#[cfg(test)]
fn entries_in_blocks(content: ContentContext<'_>, blocks: &[Block]) -> Vec<SemanticEntry> {
    let mut entries = Vec::new();
    super::walk::visit_child_entries(blocks, &mut |item| {
        if let Some(entry) = entry_from_owner(content, item) {
            entries.push(entry);
        }
    });
    entries
}

#[cfg(test)]
pub(super) fn entry_from_definition(
    content: ContentContext<'_>,
    item: &crate::DefinitionItem,
) -> Option<SemanticEntry> {
    entry_from_owner(content, EntryOwner::Definition(item))
}

#[cfg(test)]
fn entry_from_owner(content: ContentContext<'_>, item: EntryOwner<'_>) -> Option<SemanticEntry> {
    let mut entry = SemanticEntry::from_owner_shallow_with_content(item, content)
        .expect("test entry content resolves")?;
    entry.children = entries_in_blocks(content, item.blocks());
    Some(entry)
}

fn entries_with_locations(
    content: ContentContext<'_>,
    blocks: &[Block],
    sections: &[usize],
    parent: &[usize],
    prefix: &[crate::ContentBlockStep],
    owners: &mut BTreeMap<crate::OutlinePath, crate::ContentReveal>,
) -> Vec<SemanticEntry> {
    let mut entries = Vec::new();
    visit_child_entry_locations(blocks, prefix, &mut |item, _, path, item_index| {
        let Some(mut entry) = SemanticEntry::from_owner_shallow_with_content(item, content)
            .expect("document entries resolve in their own content store")
        else {
            return;
        };
        let mut coordinates = parent.to_vec();
        coordinates.push(entries.len() + 1);
        let section_path: Vec<_> = sections.iter().map(|index| index + 1).collect();
        if let Some(key) = crate::OutlinePath::nested_entry(
            (!section_path.is_empty()).then_some(section_path.as_slice()),
            &coordinates,
        ) {
            owners.insert(
                key,
                crate::ContentReveal::Owner {
                    sections: sections
                        .iter()
                        .map(|index| u32::try_from(*index).unwrap_or(u32::MAX))
                        .collect(),
                    blocks: path.to_vec(),
                    item_index,
                },
            );
        }
        let mut child_path = path.to_vec();
        child_path.push(owner_child_step(item, item_index));
        entry.children = entries_with_locations(
            content,
            item.blocks(),
            sections,
            &coordinates,
            &child_path,
            owners,
        );
        entries.push(entry);
    });
    entries
}

impl SemanticEntry {
    /// Project one owner's metadata through its authoritative content store.
    ///
    /// Document-wide alias-of validity remains a separate operation.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained owner content does not resolve
    /// in this store.
    pub fn from_owner_shallow_with_content<'store>(
        item: EntryOwner<'store>,
        content: ContentContext<'store>,
    ) -> Result<Option<Self>, ContentReadError> {
        EntryOwnerView::flow(item, content).semantic_entry()
    }
}
