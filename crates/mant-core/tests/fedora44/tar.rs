//! Tests for the Fedora Linux 44 `tar(1)` zstd fixture.

use crate::common::{
    self, count_outline_entries, find_outline_entry, query_for_document, semantic_definition_items,
};
use crate::fixtures::fedora44_manual;
use mant_ast::{
    ExcerptSelection, OutlineDetail, SearchCase, SearchQuery, SearchScope, SearchSyntax,
    SourceFormat,
};
use mant_core::{build_outline_with_detail, search_query, select_excerpt};

/// 9 sections, `os = "TAR"`, 156 option-outline entries.
#[test]
fn keeps_complete_sections_and_semantic_option_outlines() {
    let document = fedora44_manual("tar");
    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(document.sections.len(), 9);
    assert_eq!(document.meta.section.as_deref(), Some("1"));
    assert_eq!(document.meta.os.as_deref(), Some("TAR"));

    let query = query_for_document("tar", document);
    let outline = build_outline_with_detail(&query, OutlineDetail::Options)
        .unwrap_or_else(|error| panic!("build tar option outline: {error}"));
    assert_eq!(count_outline_entries(&outline.nodes), 156);
    assert!(find_outline_entry(&outline.nodes, "--acls").is_some());

    common::assert_no_duplicate_vertical_spacing(&document.sections, "fedora44/tar");
}

/// `--acls` option is addressable through a V2 outline and
/// `select_excerpt` returns its identity.
#[test]
fn options_are_addressable_in_v3_outlines_and_excerpts() {
    let document = fedora44_manual("tar");
    let query = query_for_document("tar", document);
    let acls = semantic_definition_items(document)
        .into_iter()
        .find(|item| {
            item.identity
                .as_ref()
                .is_some_and(|identity| identity.names.iter().any(|name| name == "--acls"))
        })
        .expect("tar --acls semantic option");
    let identity = acls.identity.as_ref().expect("option identity");
    assert!(!identity.id.is_empty());

    let outline =
        build_outline_with_detail(&query, OutlineDetail::Options).expect("tar option outline");
    let outlined = find_outline_entry(&outline.nodes, "--acls").expect("outlined --acls");
    assert_eq!(outlined.id(), identity.id);

    let excerpt = select_excerpt(&query, &["acls".to_owned()]).expect("--acls excerpt by alias");
    assert!(matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|value| value.id == identity.id)
    ));
}

/// `search_query` for `--acls` returns markdown line/column coordinates
/// and the result node is directly selectable.
#[test]
fn search_maps_long_options_to_markdown_lines_and_selectable_nodes() {
    let query = query_for_document("tar", fedora44_manual("tar"));
    let result = search_query(
        &query,
        &SearchQuery {
            pattern: "--acls".to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 1,
            limit: 100,
            offset: 0,
        },
    )
    .expect("search tar --acls");

    assert!(result.total >= 1);
    let option = result
        .matches
        .iter()
        .find(|found| {
            matches!(&found.node,
                mant_ast::SearchNode::DocumentEntry { names, .. }
                if names.iter().any(|name| name == "--acls"))
        })
        .expect("--acls option match");
    assert!(option.node.path().contains("/o"));
    assert!(option.markdown.start_line > 1);
    assert!(option.markdown.start_column > 0);
    assert!(option.preview.contains("--acls"));

    let excerpt = select_excerpt(&query, &[option.node.path().to_owned()])
        .expect("search node can be passed directly to --node");
    assert!(matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| identity.names.iter().any(|name| name == "--acls"))
    ));
}

/// No roff escapes leak into text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("fedora44/tar", fedora44_manual("tar"));
}
