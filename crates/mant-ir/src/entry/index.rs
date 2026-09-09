//! Rebuild an operation-local semantic index from finalized identities.
use super::{
    model::{DocumentReference, EntrySummary, SemanticDocumentTarget, SemanticEntry, ValueDomain},
    walk::{owner_child_step, visit_child_entry_locations},
};
use crate::{Block, Document, EntryOwner, Inline, NodeId};
use std::collections::BTreeMap;

/// Rebuildable semantic index for the document root and every section.
///
/// The index is a derived navigation sidecar. Both item shapes and their facts
/// remain in the [`Document`] content tree, so callers may rebuild this value
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
        let mut owner_locations = BTreeMap::new();
        let root = entries_with_locations(&document.blocks, &[], &[], &[], &mut owner_locations);
        let mut sections = BTreeMap::new();
        let mut section_paths = BTreeMap::new();
        collect_section_entries(
            &document.sections,
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
            entries_with_locations(&section.blocks, &path, &[], &[], owners),
        );
        collect_section_entries(&section.children, &path, output, paths, owners);
    }
}

#[cfg(test)]
fn entries_in_blocks(blocks: &[Block]) -> Vec<SemanticEntry> {
    let mut entries = Vec::new();
    super::walk::visit_child_entries(blocks, &mut |item| {
        if let Some(entry) = entry_from_owner(item) {
            entries.push(entry);
        }
    });
    entries
}

#[cfg(test)]
pub(super) fn entry_from_definition(item: &crate::DefinitionItem) -> Option<SemanticEntry> {
    entry_from_owner(EntryOwner::Definition(item))
}

#[cfg(test)]
fn entry_from_owner(item: EntryOwner<'_>) -> Option<SemanticEntry> {
    let mut entry = SemanticEntry::from_owner_shallow(item)?;
    entry.children = entries_in_blocks(item.blocks());
    Some(entry)
}

fn entries_with_locations(
    blocks: &[Block],
    sections: &[usize],
    parent: &[usize],
    prefix: &[crate::ContentBlockStep],
    owners: &mut BTreeMap<crate::OutlinePath, crate::ContentReveal>,
) -> Vec<SemanticEntry> {
    let mut entries = Vec::new();
    visit_child_entry_locations(blocks, prefix, &mut |item, path, item_index| {
        let Some(mut entry) = SemanticEntry::from_owner_shallow(item) else {
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
        entry.children =
            entries_with_locations(item.blocks(), sections, &coordinates, &child_path, owners);
        entries.push(entry);
    });
    entries
}

impl SemanticEntry {
    /// Project this owner's metadata only, without copying content or indexing descendants.
    ///
    /// Document-wide alias-of validity is a separate operation; consumers must
    /// consult [`crate::entry_relation_issues`] before exposing that relation.
    #[must_use]
    pub fn from_owner_shallow(item: EntryOwner<'_>) -> Option<Self> {
        let identity = item.facts()?;
        let forms = item.forms().unwrap_or_default();
        let value_domain = identity.value_domain.clone().or_else(|| {
            item.has_value_choices()
                .then_some(ValueDomain::Choices { exhaustive: false })
        });
        Some(SemanticEntry {
            id: identity.id.clone(),
            kind: identity.kind,
            names: item.validated_names().unwrap_or_default().to_vec(),
            // An absent relationship has nothing to project. Native manuals usually
            // have no authored groups: do not revalidate all names a second time.
            alias_groups: if identity.alias_groups.is_empty() {
                Vec::new()
            } else {
                item.validated_alias_groups().unwrap_or_default().to_vec()
            },
            alias_of: identity.alias_of.clone(),
            case: identity.case,
            forms: forms.iter().map(inline_text).collect(),
            document_targets: document_targets(&forms),
            children: Vec::new(),
            value_domain,
        })
    }
}

fn document_targets(terms: &crate::EntryForms<'_>) -> Vec<SemanticDocumentTarget> {
    let mut targets = Vec::new();
    for term in terms.iter() {
        collect_document_targets(term, &mut targets);
    }
    targets
}

fn collect_document_targets(inlines: &[Inline], output: &mut Vec<SemanticDocumentTarget>) {
    for inline in inlines {
        match inline {
            Inline::Link {
                target, children, ..
            } if DocumentReference::from_link_target(target).is_some() => {
                let candidate = SemanticDocumentTarget {
                    label: inline_text(children),
                    reference: DocumentReference::from_link_target(target)
                        .expect("the match guard accepts a document reference"),
                };
                if !output.contains(&candidate) {
                    output.push(candidate);
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect_document_targets(children, output),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
    }
}

pub(super) fn inline_text(inlines: &[Inline]) -> String {
    let mut output = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => output.push_str(&inline_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
}
