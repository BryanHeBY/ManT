//! Rebuild an operation-local semantic index from finalized identities.
use super::{
    model::{
        EntryKind, EntrySummary, SemanticDocumentReference, SemanticDocumentTarget, SemanticEntry,
        ValueDomain,
    },
    walk::visit_child_entries,
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
    sections: BTreeMap<NodeId, Vec<SemanticEntry>>,
}

impl SemanticIndex {
    /// Build the semantic index from finalized facts on either content shape.
    #[must_use]
    pub fn build(document: &Document) -> Self {
        let root = entries_in_blocks(&document.blocks);
        let mut sections = BTreeMap::new();
        collect_section_entries(&document.sections, &mut sections);
        let mut result = Self { root, sections };
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

    /// Entries directly owned by one section.
    #[must_use]
    pub fn section(&self, id: &str) -> &[SemanticEntry] {
        self.sections.get(id).map_or(&[], Vec::as_slice)
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
    output: &mut BTreeMap<NodeId, Vec<SemanticEntry>>,
) {
    for section in sections {
        output.insert(section.id.clone(), entries_in_blocks(&section.blocks));
        collect_section_entries(&section.children, output);
    }
}

fn entries_in_blocks(blocks: &[Block]) -> Vec<SemanticEntry> {
    let mut entries = Vec::new();
    visit_child_entries(blocks, &mut |item| {
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

fn entry_from_owner(item: EntryOwner<'_>) -> Option<SemanticEntry> {
    let identity = item.facts()?;
    let forms = item.forms().unwrap_or_default();
    let children = entries_in_blocks(item.blocks());
    let value_domain = identity.value_domain.clone().or_else(|| {
        (!children.is_empty() && children.iter().all(|child| child.kind == EntryKind::Value))
            .then_some(ValueDomain::Choices { exhaustive: false })
    });
    Some(SemanticEntry {
        id: identity.id.clone(),
        kind: identity.kind,
        names: item.validated_names().unwrap_or_default().to_vec(),
        alias_groups: item.validated_alias_groups().unwrap_or_default().to_vec(),
        alias_of: identity.alias_of.clone(),
        case: identity.case,
        forms: forms.iter().map(inline_text).collect(),
        document_targets: document_targets(&forms),
        children,
        value_domain,
    })
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
            } if SemanticDocumentReference::from_link_target(target).is_some() => {
                let candidate = SemanticDocumentTarget {
                    label: inline_text(children),
                    reference: SemanticDocumentReference::from_link_target(target)
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
