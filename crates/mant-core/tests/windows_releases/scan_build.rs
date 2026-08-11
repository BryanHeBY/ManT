//! Tests for LLVM's scan-build manual from its official Windows MSVC archive.

use mant_ast::{OutlineDetail, SourceFormat};
use mant_core::build_outline_with_detail;

use crate::common::{self, count_outline_entries, find_outline_entry};
use crate::fixtures::{windows_release_manual, windows_release_query};

const SCAN_BUILD_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "EXIT STATUS",
    "CHECKERS",
    "EXAMPLE",
    "AUTHORS",
];

#[test]
fn keeps_the_analyzer_options_checkers_and_archive_metadata() {
    let document = windows_release_manual("scan-build");
    assert_eq!(document.source.format, SourceFormat::Mdoc);
    assert!(common::source_path_ends_with(
        document,
        "windows-releases/scan-build.1.zst"
    ));
    assert_eq!(
        document
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect::<Vec<_>>(),
        SCAN_BUILD_SECTIONS,
    );
    assert_eq!(document.meta.section.as_deref(), Some("1"));
    assert_eq!(document.meta.date.as_deref(), Some("August 18, 2024"));
    assert_eq!(document.meta.os.as_deref(), Some("clang 20"));

    let outline =
        build_outline_with_detail(&windows_release_query("scan-build"), OutlineDetail::Entries)
            .expect("build scan-build option outline");
    assert_eq!(count_outline_entries(&outline.nodes), 19);
    assert!(find_outline_entry(&outline.nodes, "--use-cc").is_some());
}

#[test]
fn does_not_leak_roff_markup_or_duplicate_spacing() {
    let document = windows_release_manual("scan-build");
    common::assert_document_has_no_source_markup("windows-releases/scan-build", document);
    common::assert_no_duplicate_vertical_spacing(&document.sections, "windows-releases/scan-build");
}
