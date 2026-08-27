//! Tests for the Fedora Linux 44 `git(1)` zstd fixture.

use crate::common::{self, count_outline_entries, find_outline_entry, query_for_document};
use crate::fixtures::fedora44_manual;
use mant_engine::build_outline_with_detail;
use mant_ir::{Block, SourceFormat};
use mant_protocol::OutlineDetail;

/// 24 sections, `os = "Git 2.53.0"`, and options plus environment entries.
#[test]
fn keeps_complete_sections_and_semantic_option_outlines() {
    let document = fedora44_manual("git");
    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(document.sections.len(), 24);
    assert_eq!(document.meta.manual_section.as_deref(), Some("1"));
    assert_eq!(document.meta.os.as_deref(), Some("Git 2.53.0"));

    let query = query_for_document("git", document);
    let outline = build_outline_with_detail(&query, OutlineDetail::Entries)
        .unwrap_or_else(|error| panic!("build git option outline: {error}"));
    assert_eq!(count_outline_entries(&outline.nodes), 102);
    assert!(find_outline_entry(&outline.nodes, "--help").is_some());
    assert!(find_outline_entry(&outline.nodes, "GIT_DIR").is_some());

    let version = common::nested_definition_items(common::section(document, "OPTIONS"))
        .into_iter()
        .find(|item| {
            item.identity
                .as_ref()
                .is_some_and(|identity| identity.names.iter().any(|name| name == "--version"))
        })
        .expect("semantic --version option");
    assert!(matches!(
        version.description.first(),
        Some(Block::Paragraph { layout, .. }) if layout.spacing_before_lines == 0
    ));

    common::assert_no_duplicate_vertical_spacing(&document.sections, "fedora44/git");
}

/// No roff escapes leak into text.
#[test]
fn does_not_leak_roff_markup() {
    let document = fedora44_manual("git");
    common::assert_document_has_no_source_markup("fedora44/git", document);
    common::assert_git_generated_highlight_is_lowered("fedora44/git", document);
}
