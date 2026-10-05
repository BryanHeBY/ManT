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
        if node.roles().len() > 1
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
    for (alias, targets) in index.ambiguous_fragments() {
        diagnostics.push(invariant(
            "ir.ambiguous-fragment-alias",
            format!(
                "fragment '{alias}' resolves to multiple document-local IDs: {}",
                targets
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }

    let mut collector = InvariantCollector::default();
    collector.visit_document(document);
    diagnostics.extend(collector.diagnostics);
    for id in collector.section_targets {
        if !index.contains(id.as_str()) {
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
