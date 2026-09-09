//! Whitespace is presentation, not proof that a definition's continuation ended.
#[path = "../src/semantic_test_read.rs"]
mod semantic_read;
use std::path::Path;

use mant_engine::{build_outline_projection, parse_manual_bytes, query_markdown_text};
use mant_ir::Block;
use mant_protocol::{EntryProjection, ExcerptSelection, OutlineNode};
use mant_render::render_excerpt_text;

#[test]
fn inline_definition_continuations_keep_the_structural_description_origin() {
    let query = mant_engine::query_roff_bytes(include_bytes!(
        "../../../tests/fixtures/roff/inline-definition-continuations.1"
    ))
    .unwrap();
    let text = mant_render::render_query_text(&query);
    for payload in [
        "INLINE_CONTINUATION.",
        "CODE_CONTINUATION",
        "SECOND_CONTINUATION.",
    ] {
        let line = text.lines().find(|line| line.trim() == payload).unwrap();
        assert_eq!(line, format!("    {payload}"));
    }
    assert!(
        // Independent .sp and .sp 2 consume three blank rows, as in
        // mandoc CVS HEAD term_vspace and groff (not max(1, 2)).
        text.contains("Initial.\n\n\n\n    INLINE_CONTINUATION."),
        "{text}"
    );
    let excerpt = semantic_read::semantic_excerpt(&query, &["-a"]).unwrap();
    let extracted = render_excerpt_text(&excerpt);
    assert!(extracted.contains("    SECOND_CONTINUATION."));
    assert!(!extracted.contains("--next-option"));
}

#[test]
fn leading_spacing_and_code_do_not_become_an_inline_description() {
    for label in ["-a", "--long-option"] {
        for body in [".sp 2\nCONTENT", ".nf\nCONTENT\n.fi"] {
            let source = format!(".TH PROBE 1\n.SH OPTIONS\n.IP \"{label}\" 4\n{body}\n");
            let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
            let text = mant_render::render_query_text(&query);
            assert!(text.lines().any(|line| line == label), "{text}");
            assert!(text.lines().any(|line| line == "    CONTENT"), "{text}");
            if body.starts_with(".sp") {
                assert!(
                    text.contains(&format!("{label}\n\n\n    CONTENT")),
                    "{text}"
                );
            }
        }
    }
}

#[test]
fn spaced_relative_scopes_stay_with_their_definition_across_query_surfaces() {
    let doc = parse_manual_bytes(
        Path::new("definition-spaced-continuations.1"),
        include_bytes!("../../../tests/fixtures/roff/definition-spaced-continuations.1"),
    )
    .unwrap();
    let mut query = query_markdown_text("# Placeholder\n\nBody.\n", None).unwrap();
    query.document = Some(doc);
    let excerpt = semantic_read::semantic_excerpt(&query, &["--help"]).unwrap();
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

    let outline = build_outline_projection(
        &query,
        EntryProjection::All,
        Some(mant_protocol::ContentSelector::path(
            excerpt.selections[0].outline().path(),
        )),
    )
    .unwrap();
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
            render_excerpt_text(
                &mant_engine::select_excerpt(
                    &query,
                    &[mant_protocol::ContentSelector::path(child.path())]
                )
                .unwrap()
            )
            .contains(expected)
        );
    }
    assert_eq!(
        mant_engine::select_excerpt(
            &query,
            &[mant_protocol::ContentSelector::path(path.as_ref())]
        )
        .unwrap(),
        excerpt
    );
    assert!(
        render_excerpt_text(&semantic_read::semantic_excerpt(&query, &["--version"]).unwrap())
            .contains("NEXT_OPTION_BODY")
    );
}
