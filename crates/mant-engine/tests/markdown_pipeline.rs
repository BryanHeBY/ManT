//! End-to-end Markdown contracts live above the parser/encoder boundary.
use mant_codec::parse_markdown;
use mant_ir::ResolvedContent;
use mant_ir::{Block, DocumentAddress, EntryKind, Inline, MarkdownOrigin, NameCase};
use mant_loader::load_markdown_text;
use mant_protocol::{
    EntryProjection, ExcerptSelection, OutlineDetail, OutlineNode, OutlineNodeReference,
    SearchCase, SearchQuery, SearchScope, SearchSyntax,
};
use mant_query::{
    ProjectionError, build_outline_projection, build_outline_with_detail, search_query,
    select_excerpt,
};
use mant_render::{render_outline_text, render_query_text};

#[path = "markdown_pipeline/entries.rs"]
mod entries;
#[path = "markdown_pipeline/entry_forms.rs"]
mod entry_forms;
#[path = "markdown_pipeline/navigation.rs"]
mod navigation;
#[path = "../src/semantic_test_read.rs"]
mod semantic_test_read;
#[path = "markdown_pipeline/source_contracts.rs"]
mod source_contracts;

// These plain-document fixtures contain no top-level directives. Use the public
// parser, not an exported internal event/heading helper or a second parser.
fn parse_document(source: &str, path: Option<String>) -> mant_ir::Document {
    parse_markdown(source, path)
        .expect("Markdown fixture")
        .document
}

#[test]
fn a_heading_slug_colliding_with_a_disambiguated_duplicate_stays_unique() {
    // `# Foo 2` slugs to `foo-2`, the same id a second `# Foo` produces by
    // disambiguation. Every section must still own a distinct id, or search
    // ownership silently misattributes between the collision pair.
    let markdown = "\
# Foo

Alpha.

# Foo

Beta.

# Foo 2

Gamma.
";
    let document = parse_document(markdown, None);

    let ids: Vec<&str> = document
        .sections
        .iter()
        .map(|section| section.id.as_str())
        .collect();
    assert_eq!(ids, ["foo-2", "foo-2-2"]);

    let query = ResolvedContent {
        address: None,
        label: "collision".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let result = search_query(
        &query,
        &SearchQuery {
            pattern: "Gamma".to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .expect("search colliding section");
    assert_eq!(result.total, 1);
    assert!(matches!(
        &result.matches[0].outline.node,
        OutlineNodeReference::DocumentSection { path, id, .. }
            if path == "2" && id == "foo-2-2"
    ));
}

#[test]
fn exact_names_do_not_create_shorthands_or_selector_diagnostics() {
    let parsed = parse_markdown(
        "# Tool\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `-help`: Short help spelling.\n- `--help`: Long help spelling.\n",
        Some("shorthand-collision.md".to_owned()),
    )
    .expect("shorthand collision fixture");
    assert!(
        parsed.document.diagnostics.is_empty(),
        "{:?}",
        parsed.document.diagnostics
    );
    let query = ResolvedContent {
        address: None,
        label: "shorthand-collision.md".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    for selector in ["-help", "--help"] {
        assert!(crate::semantic_test_read::semantic_excerpt(&query, &[selector]).is_ok());
    }
    assert!(matches!(
        crate::semantic_test_read::semantic_excerpt(&query, &["help"]),
        Err(ProjectionError::UnknownSelector { .. })
    ));
}

#[test]
fn declared_fixed_attached_values_keep_their_official_identity() {
    let parsed = parse_markdown(
        "# Tool\n\n## Options\n\n<!-- mant:entries role=option case=insensitive attached=fixed -->\n- `/F`: Extended scan.\n- `/F:Y`: Extended scan and cleanup.\n- `/server:<NAME>`: Select a server.\n- `perf=default`: Select the default policy.\n",
        None,
    )
    .expect("fixed attached option values");
    assert!(parsed.document.diagnostics.is_empty());
    let query = ResolvedContent {
        address: None,
        label: "tool.md".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    let outline = build_outline_with_detail(&query, OutlineDetail::Entries)
        .expect("fixed attached value outline");
    let OutlineNode::DocumentSection { children, .. } = outline
        .nodes
        .iter()
        .find(|node| matches!(node, OutlineNode::DocumentSection { .. }))
        .expect("section node")
    else {
        panic!("options section");
    };
    assert!(matches!(
        children.as_slice(),
        [
            OutlineNode::DocumentEntry { id: first_id, title: first_title, names: first_names, .. },
            OutlineNode::DocumentEntry { id: fixed_id, title: fixed_title, names: fixed_names, .. },
            OutlineNode::DocumentEntry { names: placeholder_names, .. },
            OutlineNode::DocumentEntry { id: equals_id, title: equals_title, names: equals_names, .. },
        ] if first_id == "option-f"
            && first_title == "/F"
            && first_names == &["/F"]
            && fixed_id == "option-f-y"
            && fixed_title == "/F:Y"
            && fixed_names == &["/F:Y"]
            && placeholder_names == &["/server"]
            && equals_id == "option-perf-default"
            && equals_title == "perf=default"
            && equals_names == &["perf=default"]
    ));
    for selector in ["/F", "/F:Y", "/f:y", "perf=default"] {
        crate::semantic_test_read::semantic_excerpt(&query, &[selector])
            .expect("fixed attached value selector");
    }
}
