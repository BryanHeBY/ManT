//! Tests for the Fedora Linux 44 `clang(1)` zstd fixture.

use crate::common::{
    self, count_outline_entries, find_outline_entry, query_for_document, source_path_ends_with,
};
use crate::fixtures::fedora44_manual;
use mant_engine::build_outline_with_detail;
use mant_ir::SourceFormat;
use mant_protocol::OutlineDetail;

/// 9 sections, `os = "22"`, 90 semantic entries, and no duplicate
/// vertical spacing.
#[test]
fn keeps_complete_sections_and_semantic_option_outlines() {
    let document = fedora44_manual("clang");
    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(document.sections.len(), 9);
    assert_eq!(document.meta.manual_section.as_deref(), Some("1"));
    assert_eq!(document.meta.os.as_deref(), Some("22"));
    assert!(source_path_ends_with(document, "fedora44/clang.1.zst"));

    let query = query_for_document("clang", document);
    let outline = build_outline_with_detail(&query, OutlineDetail::Entries)
        .unwrap_or_else(|error| panic!("build clang option outline: {error}"));
    assert_eq!(count_outline_entries(&outline.nodes), 90);
    assert!(find_outline_entry(&outline.nodes, "-std").is_some());
    assert!(find_outline_entry(&outline.nodes, "TMPDIR").is_some());
    assert!(find_outline_entry(&outline.nodes, "TEMP").is_some());

    common::assert_no_duplicate_vertical_spacing(&document.sections, "fedora44/clang");
}

/// No roff escapes or control characters leak into text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("fedora44/clang", fedora44_manual("clang"));
}
