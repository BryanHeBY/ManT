//! Rebuild an operation-local semantic index from finalized identities.
use super::{
    model::{
        EntryKind, EntrySummary, ParameterKind, SemanticDocumentReference, SemanticDocumentTarget,
        SemanticEntry, ValueDomain,
    },
    walk::visit_child_definitions,
};
use crate::{Block, DefinitionItem, DefinitionRole, Document, Inline, NodeId};
use std::collections::BTreeMap;

/// Rebuildable semantic index for the document root and every section.
///
/// The index is a derived navigation sidecar. Definitions and their identities
/// remain in the [`Document`] content tree, so callers may rebuild this value
/// after a trusted document transformation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticIndex {
    root: Vec<SemanticEntry>,
    sections: BTreeMap<NodeId, Vec<SemanticEntry>>,
}

impl SemanticIndex {
    /// Build the semantic index from finalized definition identities.
    #[must_use]
    pub fn build(document: &Document) -> Self {
        let root = entries_in_blocks(&document.blocks);
        let mut sections = BTreeMap::new();
        collect_section_entries(&document.sections, &mut sections);
        Self { root, sections }
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
    visit_child_definitions(blocks, &mut |item| {
        if let Some(entry) = entry_from_definition(item) {
            entries.push(entry);
        }
    });
    entries
}

pub(super) fn entry_from_definition(item: &DefinitionItem) -> Option<SemanticEntry> {
    let identity = item.identity.as_ref()?;
    let children = entries_in_blocks(&item.description);
    let value_domain = identity.value_domain.clone().or_else(|| {
        (!children.is_empty() && children.iter().all(|child| child.kind == EntryKind::Value))
            .then_some(ValueDomain::Choices { exhaustive: false })
    });
    Some(SemanticEntry {
        id: identity.id.clone(),
        kind: entry_kind(identity.role),
        aliases: identity.names.clone(),
        case: identity.case,
        forms: item.terms.iter().map(|term| inline_text(term)).collect(),
        document_targets: document_targets(&item.terms),
        children,
        value_domain,
    })
}

fn document_targets(terms: &[Vec<Inline>]) -> Vec<SemanticDocumentTarget> {
    let mut targets = Vec::new();
    for term in terms {
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

const fn entry_kind(role: DefinitionRole) -> EntryKind {
    match role {
        DefinitionRole::Option => EntryKind::Parameter {
            parameter_kind: ParameterKind::Option,
        },
        DefinitionRole::Marker => EntryKind::Parameter {
            parameter_kind: ParameterKind::Marker,
        },
        DefinitionRole::Operand => EntryKind::Parameter {
            parameter_kind: ParameterKind::Operand,
        },
        DefinitionRole::Command => EntryKind::Command,
        DefinitionRole::ConfigurationKey => EntryKind::ConfigurationKey,
        DefinitionRole::EnvironmentVariable => EntryKind::EnvironmentVariable,
        DefinitionRole::Variable => EntryKind::Variable,
        DefinitionRole::Value => EntryKind::Value,
        DefinitionRole::Term => EntryKind::Term,
    }
}

fn inline_text(inlines: &[Inline]) -> String {
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
