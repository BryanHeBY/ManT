//! Tests for the Fedora Linux 44 `gcc(1)` zstd fixture.

use crate::common::{self, count_outline_entries, find_outline_entry, query_for_document};
use crate::fixtures::fedora44_manual;
use mant_ast::{OutlineDetail, SourceFormat};
use mant_core::build_outline_with_detail;

/// 10 sections, `os = "gcc-16"`, 2,731 option-outline entries.
#[test]
fn keeps_complete_sections_and_semantic_option_outlines() {
    let document = fedora44_manual("gcc");
    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(document.sections.len(), 10);
    assert_eq!(document.meta.section.as_deref(), Some("1"));
    assert_eq!(document.meta.os.as_deref(), Some("gcc-16"));

    let query = query_for_document("gcc", document);
    let outline = build_outline_with_detail(&query, OutlineDetail::Options)
        .unwrap_or_else(|error| panic!("build gcc option outline: {error}"));
    assert_eq!(count_outline_entries(&outline.nodes), 2_731);
    assert!(find_outline_entry(&outline.nodes, "-Wsuggest-final-types").is_some());

    common::assert_no_duplicate_vertical_spacing(&document.sections, "fedora44/gcc");
}

/// No roff escapes leak into inline text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("fedora44/gcc", fedora44_manual("gcc"));
}
