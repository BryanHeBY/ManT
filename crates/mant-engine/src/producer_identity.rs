//! Producer identity allocation policy over source-neutral IR locations.
//! No query selectors, DTOs or presentation are needed to validate an identity.
use mant_ir::{
    Block, DOCUMENT_ROOT_ID, Diagnostic, DiagnosticLevel, OutlinePath, Section, SourceSpan,
    content_entry_locations,
};
use std::collections::BTreeMap;

/// Reserve synthetic roots, structural coordinates and generated entry prefixes
/// before allocating authored section identities. This is producer policy, not
/// a restriction on all public IR IDs: generated entry IDs use these prefixes.
pub(crate) fn is_reserved_selector(value: &str) -> bool {
    matches!(value, "tldr" | DOCUMENT_ROOT_ID)
        || value.parse::<OutlinePath>().is_ok()
        || [
            "option-",
            "marker-",
            "operand-",
            "command-",
            "configuration-",
            "environment-",
            "variable-",
            "value-",
            "term-",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

struct IdentityLocation {
    path: OutlinePath,
    source: Option<SourceSpan>,
}

/// Report duplicated content identities; repeated semantic names are valid evidence.
pub(crate) fn outline_identity_diagnostics(
    blocks: &[Block],
    sections: &[Section],
    source_family: &str,
) -> Vec<Diagnostic> {
    let mut ids = BTreeMap::new();
    collect_entries(blocks, None, &mut ids);
    collect_sections(sections, &[], &mut ids);
    ids.into_iter()
        .filter_map(|(id, matches)| {
            if matches.len() < 2 {
                return None;
            }
            let candidates = matches
                .iter()
                .map(|candidate| format!("{} ({id})", candidate.path))
                .collect::<Vec<_>>()
                .join(", ");
            Some(Diagnostic {
                level: DiagnosticLevel::Warning,
                impact: mant_ir::DiagnosticImpact::None,
                code: Some(format!("{source_family}.outline.duplicate-id")),
                message: format!(
                    "outline ID '{id}' belongs to multiple nodes: {candidates}; select by path"
                ),
                source: matches.first().and_then(|candidate| candidate.source),
            })
        })
        .collect()
}

fn collect_entries<'a>(
    blocks: &'a [Block],
    section: Option<&[usize]>,
    ids: &mut BTreeMap<&'a str, Vec<IdentityLocation>>,
) {
    for entry in content_entry_locations(blocks) {
        let facts = entry
            .owner()
            .facts()
            .expect("located entries have identities");
        ids.entry(facts.id.as_str())
            .or_default()
            .push(IdentityLocation {
                path: OutlinePath::nested_entry(section, entry.indices())
                    .expect("enumerated entry paths are one-based"),
                source: entry.source(),
            });
    }
}

fn collect_sections<'a>(
    sections: &'a [Section],
    parent: &[usize],
    ids: &mut BTreeMap<&'a str, Vec<IdentityLocation>>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut coordinates = parent.to_vec();
        coordinates.push(index + 1);
        ids.entry(section.id.as_str())
            .or_default()
            .push(IdentityLocation {
                path: OutlinePath::section(&coordinates)
                    .expect("enumerated section paths are one-based"),
                source: section.source,
            });
        collect_entries(&section.blocks, Some(&coordinates), ids);
        collect_sections(&section.children, &coordinates, ids);
    }
}

#[cfg(test)]
mod tests;
