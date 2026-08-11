//! Tests for rustc's byte-identical Windows and Linux release manual.

use mant_ast::OutlineDetail;
use mant_core::build_outline_with_detail;

use crate::common::{self, count_outline_entries, find_outline_entry};
use crate::fixtures::{cross_platform_release_manual, cross_platform_release_query};

const RUSTC_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "OPTIONS",
    "CODEGEN OPTIONS",
    "ENVIRONMENT",
    "EXAMPLES",
    "SEE ALSO",
    "BUGS",
    "AUTHOR",
    "COPYRIGHT",
];

#[test]
fn keeps_compiler_options_environment_and_release_metadata() {
    let document = cross_platform_release_manual("rustc");
    common::assert_section_topology("cross-platform-releases/rustc", document, RUSTC_SECTIONS);
    assert_eq!(document.meta.section.as_deref(), Some("1"));
    assert_eq!(document.meta.date.as_deref(), Some("April 2019"));
    assert_eq!(
        document.meta.os.as_deref(),
        Some("rustc 1.97.1 (8bab26f4f 2026-07-14)")
    );

    let outline = build_outline_with_detail(
        &cross_platform_release_query("rustc"),
        OutlineDetail::Entries,
    )
    .expect("build rustc option outline");
    assert_eq!(count_outline_entries(&outline.nodes), 31);
    assert!(find_outline_entry(&outline.nodes, "--target").is_some());
}

#[test]
fn does_not_leak_roff_markup_or_duplicate_spacing() {
    let document = cross_platform_release_manual("rustc");
    common::assert_document_has_no_source_markup("cross-platform-releases/rustc", document);
    common::assert_no_duplicate_vertical_spacing(
        &document.sections,
        "cross-platform-releases/rustc",
    );
}
