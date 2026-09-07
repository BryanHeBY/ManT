//! Producer diagnostics use the same index as query selection.
use super::{
    DocumentSelectorIndex, LocatedNode, collect_root_entries, collect_sections,
    semantic_name_shorthand,
};
use mant_ir::{Block, Diagnostic, DiagnosticLevel, Section};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Report selectors that cannot address exactly one semantic entry.
///
/// The lookup policy itself remains usable through stable paths and IDs, but
/// Markdown authors receive a source diagnostic before an agent discovers the
/// ambiguity at query time.
pub(crate) fn semantic_selector_diagnostics(
    blocks: &[Block],
    sections: &[Section],
    source_family: &str,
) -> Vec<Diagnostic> {
    let mut located = Vec::new();
    collect_root_entries(blocks, &mut located);
    collect_sections(sections, &[], &[], &mut located);
    let index = DocumentSelectorIndex::new(&located);
    let mut selectors = BTreeSet::new();
    for candidate in &located {
        let Some(identity) = candidate.facts() else {
            continue;
        };
        for name in &identity.names {
            selectors.insert(name.clone());
            if let Some(shorthand) = semantic_name_shorthand(identity.kind, name) {
                selectors.insert(shorthand.to_owned());
            }
        }
    }

    let mut diagnostics = selector_alias_diagnostics(&index, selectors, source_family);
    diagnostics.extend(duplicate_id_diagnostics(&index.ids, source_family));
    diagnostics
}

fn selector_alias_diagnostics(
    index: &DocumentSelectorIndex<'_>,
    selectors: BTreeSet<String>,
    source_family: &str,
) -> Vec<Diagnostic> {
    let mut reported = HashSet::new();
    let mut diagnostics = Vec::new();
    for selector in selectors {
        let (kind, matches) = index.matching_aliases(&selector);
        let exact_ids = index
            .ids
            .get(selector.as_str())
            .map_or(&[][..], Vec::as_slice);
        let shadowed_matches = matches
            .iter()
            .copied()
            .filter(|candidate| {
                !exact_ids
                    .iter()
                    .any(|owner| owner.path() == candidate.path())
            })
            .collect::<Vec<_>>();
        if !shadowed_matches.is_empty() && !exact_ids.is_empty() {
            let key = format!("shadowed\u{1f}{selector}");
            if reported.insert(key) {
                let owners = exact_ids
                    .iter()
                    .map(|candidate| format!("{} ({})", candidate.path(), candidate.id()))
                    .collect::<Vec<_>>()
                    .join(", ");
                let entries = shadowed_matches
                    .iter()
                    .map(|candidate| format!("{} ({})", candidate.path(), candidate.id()))
                    .collect::<Vec<_>>()
                    .join(", ");
                diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    code: Some(format!(
                        "{source_family}.semantic-entry.shadowed-selector"
                    )),
                    message: format!(
                        "semantic selector '{selector}' is owned by exact outline ID {owners}; matching {} entries {entries} require their path or ID",
                        kind.label()
                    ),
                    source: shadowed_matches
                        .first()
                        .and_then(|candidate| candidate.source()),
                });
            }
        }
        if matches.len() < 2 {
            continue;
        }
        let key = matches
            .iter()
            .map(|candidate| candidate.id())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        if !reported.insert(key) {
            continue;
        }
        let candidates = matches
            .iter()
            .map(|candidate| format!("{} ({})", candidate.path(), candidate.id()))
            .collect::<Vec<_>>()
            .join(", ");
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some(format!(
                "{source_family}.semantic-entry.ambiguous-selector"
            )),
            message: format!(
                "semantic selector '{selector}' has multiple {} matches: {candidates}; select by path or ID",
                kind.label()
            ),
            source: matches.first().and_then(|candidate| candidate.source()),
        });
    }
    diagnostics
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
