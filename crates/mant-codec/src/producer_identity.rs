//! Producer identity allocation policy over source-neutral IR locations.
//! No query selectors, DTOs or presentation are needed to validate an identity.
use mant_ir::{
    Block, DOCUMENT_ROOT_ID, Diagnostic, DiagnosticLevel, FragmentAlias, OutlinePath, Section,
    SourceSpan, content_entry_locations,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write as _,
};

/// The public content selector accepts at most this many UTF-8 bytes.
/// Source-authored fragment aliases and non-selectable anchors have their own
/// contracts.
pub(crate) const MAX_CONTENT_ID_BYTES: usize = 512;

/// Allocate an ID from a normalized base, using the final spelling to check
/// reservations and collisions. Keeping the ordinal lets a namespace resume
/// duplicate allocation without rescanning candidates.
pub(crate) fn allocate_content_id(
    base: &str,
    ordinal: &mut usize,
    mut claim: impl FnMut(&str) -> bool,
) -> String {
    let mut fingerprint = None;
    loop {
        *ordinal += 1;
        let suffix = duplicate_suffix(*ordinal);
        let limit = MAX_CONTENT_ID_BYTES - suffix.len();
        let candidate = if base.len() <= limit {
            format!("{base}{suffix}")
        } else {
            let fingerprint = fingerprint.get_or_insert_with(|| short_fingerprint(base.as_bytes()));
            let mut end = limit - fingerprint.len() - 1;
            while !base.is_char_boundary(end) {
                end -= 1;
            }
            let prefix = base[..end].trim_end_matches('-');
            format!("{prefix}-{fingerprint}{suffix}")
        };
        if claim(&candidate) {
            return candidate;
        }
    }
}

fn duplicate_suffix(ordinal: usize) -> String {
    if ordinal == 1 {
        String::new()
    } else {
        format!("-{ordinal}")
    }
}

#[derive(Default)]
pub(crate) struct HeadingIds {
    ordinals: HashMap<String, HeadingOrdinals>,
    assigned: HashSet<String>,
}

#[derive(Default)]
struct HeadingOrdinals {
    full: usize,
    bounded: usize,
}

pub(crate) struct FullHeadingId {
    spelling: String,
    ordinal: usize,
}

impl HeadingIds {
    #[cfg(feature = "roff")]
    pub(crate) fn reserve(&mut self, ids: &HashSet<String>) {
        self.assigned.extend(ids.iter().cloned());
    }

    pub(crate) fn claim_full(&mut self, base: &str) -> FullHeadingId {
        // The full slug can be a fragment even when the earlier selector
        // rejected it. Claim it independently so a collision on a shortened
        // ID cannot silently migrate that fragment to a different section.
        let ordinals = self.ordinals.entry(base.to_owned()).or_default();
        loop {
            ordinals.full += 1;
            let candidate = format!("{base}{}", duplicate_suffix(ordinals.full));
            if self.assigned.insert(candidate.clone()) {
                return FullHeadingId {
                    spelling: candidate,
                    ordinal: ordinals.full,
                };
            }
        }
    }

    pub(crate) fn complete(
        &mut self,
        base: &str,
        full: FullHeadingId,
    ) -> (String, Option<FragmentAlias>) {
        if full.spelling.len() <= MAX_CONTENT_ID_BYTES {
            return (full.spelling, None);
        }
        let ordinals = self.ordinals.get_mut(base).expect("reserved heading base");
        ordinals.bounded = ordinals.bounded.max(full.ordinal - 1);
        let bounded = allocate_content_id(base, &mut ordinals.bounded, |candidate| {
            self.assigned.insert(candidate.to_owned())
        });
        (bounded, Some(full.spelling.into()))
    }
}

pub(crate) fn short_fingerprint(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    digest[..6]
        .iter()
        .fold(String::with_capacity(12), |mut fingerprint, byte| {
            write!(fingerprint, "{byte:02x}").expect("writing to a String cannot fail");
            fingerprint
        })
}

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
