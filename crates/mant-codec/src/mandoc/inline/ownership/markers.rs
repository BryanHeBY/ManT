//! Private carriers shared by annotation execution and publication.
use std::fmt::Display;

use mant_ir::LinkTarget;

use super::annotations::AnnotationKey;

const TARGET: &str = "\0mant:annotation:";
const ENTRY: &str = "\0mant:annotation-entry:";
const END: &str = "\0mant:annotation-end:";
const LINK_SCOPE: &str = "\0mant:link-scope:";
const CANDIDATE: &str = "\0mant:sphinx-candidate:";

pub(in crate::mandoc) fn target_key(key: impl Display) -> String {
    format!("{TARGET}{key}")
}

pub(in crate::mandoc::inline) fn is_target(uri: &str) -> bool {
    uri.starts_with(TARGET)
}

pub(in crate::mandoc::inline) fn target_owner(target: &LinkTarget) -> Option<AnnotationKey> {
    let LinkTarget::External { uri } = target else {
        return None;
    };
    uri.strip_prefix(TARGET)?.parse().ok()
}

/// Mark a provisional link whose accepted glyphs include cells retired by
/// a native overstrike. The annotation publisher consumes this private fact.
pub(in crate::mandoc::inline) const COVERED_ANNOTATION: &str = "\0mant:annotation-covered";

pub(super) fn is_annotation_marker(id: &str) -> bool {
    [ENTRY, LINK_SCOPE, END]
        .iter()
        .any(|prefix| id.starts_with(prefix))
}

pub(in crate::mandoc) fn entry_marker(target: &LinkTarget) -> Option<String> {
    match target {
        LinkTarget::External { uri } if is_target(uri) => Some(format!("{ENTRY}{uri}")),
        _ => None,
    }
}

pub(in crate::mandoc) fn end_marker(epoch: u64) -> String {
    format!("{END}{epoch}")
}

pub(in crate::mandoc) fn link_scope(owner: super::annotations::AnnotationKey) -> String {
    format!("{LINK_SCOPE}{owner}")
}

pub(super) fn candidate_target(index: usize) -> LinkTarget {
    LinkTarget::External {
        uri: format!("{CANDIDATE}{index}"),
    }
}

pub(super) fn is_candidate(uri: &str) -> bool {
    uri.starts_with(CANDIDATE)
}
