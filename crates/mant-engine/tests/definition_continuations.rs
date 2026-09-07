//! Whitespace is presentation, not proof that a definition's continuation ended.
use std::path::Path;

use mant_engine::{
    build_outline_projection, parse_manual_bytes, query_markdown_text, render_excerpt_text,
    select_excerpt, select_explanation,
};
use mant_ir::Block;
use mant_protocol::{EntryProjection, ExcerptSelection, OutlineNode};

#[test]
fn spaced_relative_scopes_stay_with_their_definition_across_query_surfaces() {
    let doc = parse_manual_bytes(
        Path::new("definition-spaced-continuations.1"),
        include_bytes!("../../../tests/fixtures/roff/definition-spaced-continuations.1"),
    )
    .unwrap();
    let mut query = query_markdown_text("# Placeholder\n\nBody.\n", None).unwrap();
    query.document = Some(doc);
    let excerpt = select_explanation(&query, "--help").unwrap();
    let text = render_excerpt_text(&excerpt);
    for retained in [
        "HELP_INTRO",
        "CLASS_BODY",
        "NESTED_BODY",
        "QUALIFIER_BODY",
        "JOINED_BODY",
        "TAIL_RESTRICTION",
        "tool --help=classes,undocumented",
    ] {
        assert!(text.contains(retained), "missing {retained}: {text}");
    }
    assert!(!text.contains("NEXT_OPTION_BODY"), "{text}");
    assert!(!text.contains("NEXT_SECTION_BODY"), "{text}");

    let [ExcerptSelection::DocumentEntry { entry, .. }] = &excerpt.selections[..] else {
        panic!("expected the help entry")
    };
    let owner = entry.entry_owner().unwrap();
    assert!(owner.facts().unwrap().value_domain.is_none());
    assert!(
        owner
            .blocks()
            .iter()
            .any(|block| matches!(block, Block::VerticalSpace { .. }))
    );
    assert!(
        owner
            .blocks()
            .iter()
            .any(|block| matches!(block, Block::Preformatted { .. }))
    );

    let outline =
        build_outline_projection(&query, EntryProjection::All, Some("--help".into())).unwrap();
    let [OutlineNode::DocumentEntry { path, children, .. }] = &outline.nodes[..] else {
        panic!("expected a single help subtree")
    };
    assert_eq!(children.len(), 3, "{children:#?}");
    assert_eq!(children[0].children().len(), 1);
    for (child, expected) in children
        .iter()
        .zip(["CLASS_BODY", "QUALIFIER_BODY", "JOINED_BODY"])
    {
        assert!(
            render_excerpt_text(&select_excerpt(&query, &[child.path()]).unwrap())
                .contains(expected)
        );
    }
    assert_eq!(select_excerpt(&query, &[path.as_ref()]).unwrap(), excerpt);
    assert!(
        render_excerpt_text(&select_explanation(&query, "--version").unwrap())
            .contains("NEXT_OPTION_BODY")
    );
}
