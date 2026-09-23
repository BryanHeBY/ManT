//! Tests for the Fedora Linux 44 `git(1)` zstd fixture.

use crate::common::{self, count_outline_entries, find_outline_entry, query_for_document};
use crate::fixtures::fedora44_manual;
use mant_ir::{Block, ListKind, SourceFormat};
use mant_protocol::OutlineDetail;
use mant_query::build_outline_with_detail;

/// 24 sections, `os = "Git 2.53.0"`, and options plus environment entries.
#[test]
fn keeps_complete_sections_and_semantic_option_outlines() {
    let document = fedora44_manual("git");
    assert_eq!(document.root_format(), Some(SourceFormat::Man));
    assert_eq!(document.flow().unwrap().sections.len(), 24);
    assert_eq!(document.meta.manual_section.as_deref(), Some("1"));
    assert_eq!(document.meta.os.as_deref(), Some("Git 2.53.0"));

    let query = query_for_document("git", document);
    let outline = build_outline_with_detail(&query, OutlineDetail::Entries)
        .unwrap_or_else(|error| panic!("build git option outline: {error}"));
    assert_eq!(count_outline_entries(&outline.nodes), 231);
    assert!(find_outline_entry(&outline.nodes, "--help").is_some());
    assert!(find_outline_entry(&outline.nodes, "GIT_DIR").is_some());
    let deprecated =
        common::nested_definition_items(common::section(document, "ENVIRONMENT VARIABLES"))
            .into_iter()
            .find(|item| {
                item.entry
                    .as_ref()
                    .is_some_and(|entry| entry.names == ["GIT_PRINT_SHA1_ELLIPSIS"])
            })
            .expect("deprecated variable annotation must not erase its declaration");
    assert_eq!(
        deprecated.entry.as_ref().unwrap().kind,
        mant_ir::EntryKind::EnvironmentVariable
    );
    assert_eq!(deprecated.source.unwrap().line, 2165);
    assert!(deprecated.terms.iter().any(|term| {
        document
            .content()
            .plain_text(term)
            .is_ok_and(|text| text.contains("deprecated"))
    }));

    let commands: Vec<_> = common::semantic_definition_items(document)
        .into_iter()
        .filter(|item| item.entry.as_ref().unwrap().kind == mant_ir::EntryKind::Command)
        .collect();
    assert_eq!(commands.len(), 136);
    let add = commands
        .iter()
        .find(|item| item.entry.as_ref().unwrap().names == ["git-add"])
        .unwrap();
    assert_eq!(add.source.unwrap().line, 351);
    assert!(
        add.terms
            .iter()
            .any(|term| { common::inline_text(document.content(), term).contains("git-add(1)") })
    );
    assert!(
        common::block_slice_text(document.content(), &add.description)
            .contains("Add file contents to the index.")
    );
    assert!(
        commands
            .iter()
            .any(|item| item.entry.as_ref().unwrap().names == ["scalar"])
    );
    assert!(commands.iter().all(|item| !item.description.is_empty()));

    let version = common::nested_definition_items(common::section(document, "OPTIONS"))
        .into_iter()
        .find(|item| {
            item.entry
                .as_ref()
                .is_some_and(|identity| identity.names.iter().any(|name| name == "--version"))
        })
        .expect("semantic --version option");
    assert!(matches!(
        version.description.first(),
        Some(Block::Paragraph { layout, .. }) if layout.spacing_before_lines == 0
    ));

    let notes = common::section(document, "NOTES");
    assert!(matches!(
        notes.blocks.as_slice(),
        [Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            items,
            ..
        }] if items.len() == 7
    ));

    common::assert_bounded_vertical_spacing(document, "fedora44/git");
}

/// No roff escapes leak into text.
#[test]
fn does_not_leak_roff_markup() {
    let document = fedora44_manual("git");
    common::assert_document_has_no_source_markup("fedora44/git", document);
    common::assert_git_generated_highlight_is_lowered("fedora44/git", document);
}
