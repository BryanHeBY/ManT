//! Producer diagnostics use the same index as query selection.
use super::{
    DocumentSelectorIndex, LocatedNode, collect_selection_root_entries, collect_selection_sections,
};
use mant_ir::{Block, Diagnostic, DiagnosticLevel, Section};
use std::collections::BTreeMap;

/// Report duplicated content identities; repeated semantic names are valid evidence.
pub(crate) fn outline_identity_diagnostics(
    blocks: &[Block],
    sections: &[Section],
    source_family: &str,
) -> Vec<Diagnostic> {
    let mut located = Vec::new();
    collect_selection_root_entries(blocks, &mut located);
    collect_selection_sections(sections, &mut located);
    let index = DocumentSelectorIndex::new(&located);
    duplicate_id_diagnostics(&index.ids, source_family)
}

fn duplicate_id_diagnostics(
    ids: &BTreeMap<&str, Vec<&LocatedNode<'_>>>,
    source_family: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, matches) in ids {
        if matches.len() < 2 {
            continue;
        }
        let candidates = matches
            .iter()
            .map(|candidate| format!("{} ({})", candidate.path(), candidate.id()))
            .collect::<Vec<_>>()
            .join(", ");
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some(format!("{source_family}.outline.duplicate-id")),
            message: format!(
                "outline ID '{id}' belongs to multiple nodes: {candidates}; select by path"
            ),
            source: matches.first().and_then(|candidate| candidate.source()),
        });
    }
    diagnostics
}
