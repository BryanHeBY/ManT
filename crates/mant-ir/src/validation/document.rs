//! Coordinate indexed identities, content invariants and entry relations.
mod content;
mod diagnostics;
#[cfg(test)]
mod tests;

use super::source::validate_source_span;
use crate::{Diagnostic, Document, DocumentIndex, IndexedRole, visit::Visit};
use content::InvariantCollector;
use diagnostics::invariant;
pub(super) use diagnostics::invariant_at;

/// Validate invariants that parsers must satisfy before consumers receive IR.
///
/// Findings are ordinary document diagnostics so best-effort parsing remains
/// possible, while every parser and consumer sees the same contract failures.
#[must_use]
pub fn validate_document(document: &Document) -> Vec<Diagnostic> {
    crate::DocumentValidation::new(document).into_diagnostics()
}

impl DocumentIndex {
    /// Project the shared overview ownership finding without validating unrelated content.
    #[must_use]
    pub fn overview_conflict_diagnostic(&self) -> Option<Diagnostic> {
        self.has_overview_conflict().then(|| {
            invariant(
                crate::OVERVIEW_IDENTITY_COLLISION,
                format!(
                    "identity '{}' is shared by the document overview and another owner",
                    crate::DOCUMENT_ROOT_ID
                ),
            )
        })
    }

    /// Report ambiguous fragment aliases without validating unrelated content.
    ///
    /// A conflict over the canonical overview spelling with the same indexed
    /// ID is reported separately by [`Self::overview_conflict_diagnostic`].
    pub fn ambiguous_fragment_diagnostics(&self) -> impl Iterator<Item = Diagnostic> + '_ {
        self.ambiguous_fragments().filter_map(|(alias, targets)| {
            let detail = if targets.len() > 1 {
                format!(
                    "multiple document-local IDs: {}",
                    targets
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                if alias.as_str() == crate::DOCUMENT_ROOT_ID {
                    return None;
                }
                format!("multiple owners of '{}'", crate::DOCUMENT_ROOT_ID)
            };
            Some(invariant(
                crate::AMBIGUOUS_FRAGMENT_ALIAS,
                format!("fragment '{alias}' resolves to {detail}"),
            ))
        })
    }
}

pub(super) fn validate_with_index(
    document: &Document,
    index: &DocumentIndex,
    relations: &[crate::EntryRelationIssue],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for source in document
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.source)
    {
        validate_source_span(&mut diagnostics, source);
    }

    for (id, node) in index.iter() {
        if id.trim().is_empty() {
            for role in node.roles() {
                diagnostics.push(invariant(
                    "ir.empty-identity",
                    format!("{role:?} identity must not be empty"),
                ));
            }
        } else if !is_normalized_node_id(id) {
            diagnostics.push(invariant(
                "ir.invalid-identity",
                format!("identity '{id}' is not a normalized document-local ID"),
            ));
        }
        if id.as_str() == crate::DOCUMENT_ROOT_ID
            && let Some(collision) = index.overview_conflict_diagnostic()
        {
            diagnostics.push(collision);
        } else if node.roles().len() > 1
            && !(node.roles().len() == 2
                && node.has_role(IndexedRole::Entry)
                && node.has_role(IndexedRole::Anchor))
        {
            diagnostics.push(invariant(
                "ir.identity-role-collision",
                format!(
                    "identity '{id}' is shared by incompatible roles {:?}",
                    node.roles()
                ),
            ));
        }
    }

    for duplicate in index.duplicates() {
        diagnostics.push(invariant(
            "ir.duplicate-identity",
            format!("duplicate {:?} identity '{}'", duplicate.role, duplicate.id),
        ));
    }

    for alias in index.authored_fragments() {
        if alias.is_empty() {
            diagnostics.push(invariant(
                "ir.empty-fragment-alias",
                "source-authored fragment alias must not be empty".to_owned(),
            ));
        } else if alias
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        {
            diagnostics.push(invariant(
                "ir.invalid-fragment-alias",
                format!(
                    "source-authored fragment alias '{alias}' contains whitespace or control characters"
                ),
            ));
        }
    }
    diagnostics.extend(index.ambiguous_fragment_diagnostics());

    let mut collector = InvariantCollector::default();
    collector.visit_document(document);
    diagnostics.extend(collector.diagnostics);
    for id in collector.section_targets {
        // A producer keeps an exact spelling when canonicalizing it would
        // hide an ambiguity or lose the only unique route to the owner.
        if !index.contains(id.as_str())
            && !index.is_fragment_ambiguous(id.as_str())
            && !index
                .fragment_target(id.as_str())
                .is_some_and(|target| index.is_fragment_ambiguous(target.as_str()))
        {
            diagnostics.push(invariant(
                "ir.dangling-section-link",
                format!("section link target '{id}' does not exist"),
            ));
        }
    }

    diagnostics.extend(relations.iter().map(crate::EntryRelationIssue::diagnostic));
    diagnostics
}

/// Whether an exact authored ID satisfies the canonical identity grammar.
/// This does not check document-local uniqueness or reserved selector names.
#[must_use]
pub fn is_normalized_node_id(id: &str) -> bool {
    let mut characters = id.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    let Some(last) = id.chars().next_back() else {
        return false;
    };
    (first.is_alphanumeric() || first == '_')
        && (last.is_alphanumeric() || last == '_')
        && id
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
        && id.chars().flat_map(char::to_lowercase).eq(id.chars())
}
