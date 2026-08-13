//! Tests for Cargo's byte-identical Windows and Linux release manual.

use mant_engine::build_outline_with_detail;
use mant_protocol::OutlineDetail;

use crate::common::{self, count_outline_entries, find_outline_entry};
use crate::fixtures::{cross_platform_release_manual, cross_platform_release_query};

const CARGO_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "COMMANDS",
    "OPTIONS",
    "ENVIRONMENT",
    "EXIT STATUS",
    "FILES",
    "EXAMPLES",
    "BUGS",
    "SEE ALSO",
];

#[test]
fn keeps_commands_options_files_and_examples() {
    let document = cross_platform_release_manual("cargo");
    common::assert_section_topology("cross-platform-releases/cargo", document, CARGO_SECTIONS);
    assert_eq!(document.meta.manual_section.as_deref(), Some("1"));
    assert_eq!(
        document.meta.volume.as_deref(),
        Some("General Commands Manual")
    );

    let outline = build_outline_with_detail(
        &cross_platform_release_query("cargo"),
        OutlineDetail::Entries,
    )
    .expect("build Cargo option outline");
    assert_eq!(count_outline_entries(&outline.nodes), 13);
    assert!(find_outline_entry(&outline.nodes, "--version").is_some());
}

#[test]
fn does_not_leak_roff_markup_or_duplicate_spacing() {
    let document = cross_platform_release_manual("cargo");
    common::assert_document_has_no_source_markup("cross-platform-releases/cargo", document);
    common::assert_no_duplicate_vertical_spacing(
        &document.sections,
        "cross-platform-releases/cargo",
    );
}
