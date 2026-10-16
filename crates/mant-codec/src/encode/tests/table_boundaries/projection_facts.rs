//! Table simplification retains authored rows, author spaces and destinations.
use super::super::{Block, Inline, LayoutHint, MarkdownOptions, paragraph};
use super::fixtures::{
    assert_mapped_readback, assert_readback, cell_blocks, literal, owned, table,
};

#[test]
fn authored_literal_rows_never_become_semicolon_separated_words() {
    for (value, expected) in [
        ("", "A\n\nB"),
        ("X", "A\nX\nB"),
        ("X\n", "A\nX\n\nB"),
        ("X\n\n", "A\nX\n\n\nB"),
        (" α\u{a0} ", "A\n α\u{a0} \nB"),
    ] {
        let block = table(vec![cell_blocks(
            vec![owned("A", "first"), literal(value), owned("B", "last")],
            false,
        )]);
        assert_mapped_readback(&block, expected, &[("first", "A"), ("last", "B")]);
    }
    let mut absent = literal("");
    if let Block::Preformatted { children, .. } = &mut absent {
        children.clear();
    }
    let block = table(vec![cell_blocks(
        vec![
            paragraph(vec![Inline::Text { value: "A".into() }]),
            absent,
            paragraph(vec![Inline::Text { value: "B".into() }]),
        ],
        false,
    )]);
    assert_readback(&block, "A; B");
}

#[test]
fn unsupported_author_spaces_are_body_not_projection_padding() {
    let block = table(vec![cell_blocks(
        vec![Block::Unsupported {
            name: Some("portable".into()),
            text: " α\u{a0} ".into(),
            layout: LayoutHint::default(),
            source: None,
        }],
        false,
    )]);
    assert_readback(&block, " α\u{a0} ");
}

#[test]
fn visible_and_navigation_cells_preserve_each_anchor_once_outside_the_fence() {
    for blocks in [
        vec![
            paragraph(vec![
                Inline::anchor("spot"),
                Inline::Text { value: "X".into() },
            ]),
            owned("BODY", "owner"),
        ],
        vec![
            paragraph(vec![Inline::Strong {
                children: vec![Inline::anchor("spot"), Inline::Text { value: "X".into() }],
            }]),
            owned("BODY", "owner"),
        ],
        vec![
            paragraph(vec![Inline::anchor("spot")]),
            owned("BODY", "owner"),
        ],
    ] {
        let block = table(vec![cell_blocks(blocks, false)]);
        let wire = serde_json::to_string(&block).unwrap();
        let restored: Block = serde_json::from_str(&wire).unwrap();
        for block in [&block, &restored] {
            let mapped = super::blocks::render_blocks_with_entries(
                std::slice::from_ref(block),
                MarkdownOptions {
                    preserve_anchors: true,
                    ..MarkdownOptions::default()
                },
                true,
            );
            assert_eq!(
                mapped.text.matches("id=\"spot\"").count(),
                1,
                "{}",
                mapped.text
            );
            assert!(mapped.text.find("id=\"spot\"").unwrap() < mapped.text.find("```").unwrap());
            assert_eq!(mapped.entries.len(), 1);
            let entry = &mapped.entries[0];
            assert_eq!(&mapped.text[entry.start..entry.end], "BODY");
        }
    }
}
