//! Fenced payload retains explicit row origins and author cells.
use super::super::{
    Block, Inline, LayoutHint, MarkdownFragmentOptions, TableCell, TableRow, parse_content,
    render_blocks_fragment,
};
use super::fixtures::layout;

#[test]
fn ordinary_heading_omits_hints_while_fenced_table_projection_retains_them() {
    let rows = layout(&[(0, 1), (1, 2)]);
    let heading = mant_ir::Heading {
        content: vec![Inline::Text {
            value: "A\nB".into(),
        }],
        inline_layout: rows.clone(),
        source: None,
    };
    assert_eq!(
        super::super::render_heading_fragment(1, &heading, MarkdownFragmentOptions::default()),
        "A  \nB\n==="
    );
    let table = Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![TableCell {
                break_after: false,
                kind: mant_ir::TableCellKind::Text,
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "A\nB".into(),
                    }],
                    inline_layout: rows,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(
        render_blocks_fragment(&[table], MarkdownFragmentOptions::default()),
        ["```\n A\n  B\n```"]
    );
}

#[test]
fn fenced_payload_distinguishes_generated_spaces_author_nbsp_and_open_tails() {
    for (value, expected) in [
        ("中\u{a0}\n  x\n", "  中\u{a0}\n  x\n"),
        ("中\u{a0}\n  x\n\n", "  中\u{a0}\n  x\n\n"),
    ] {
        let block = Block::Preformatted {
            children: vec![Inline::Text {
                value: value.into(),
            }],
            inline_layout: layout(&[(0, 2), (1, -3), (2, 9)]),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        };
        let original = block.clone();
        let markdown =
            render_blocks_fragment(&[block], MarkdownFragmentOptions::default()).join("\n\n");
        assert_eq!(markdown, format!("```\n{expected}\n```"));
        let parsed = parse_content(&markdown, None).unwrap();
        let Block::Preformatted { children, .. } = &parsed.document.as_ref().unwrap().blocks[0]
        else {
            panic!("literal readback")
        };
        assert_eq!(mant_ir::inline_plain_text(children), expected);
        let Block::Preformatted {
            children,
            inline_layout,
            ..
        } = original
        else {
            unreachable!()
        };
        assert_eq!(mant_ir::inline_plain_text(&children), value);
        assert_eq!(inline_layout.row_indent(2), 9);
        assert_eq!(expected.chars().filter(|c| *c == '\u{a0}').count(), 1);
    }
}

#[test]
fn flattened_fenced_table_preserves_author_row_edges_and_explicit_hint_spaces() {
    let value = " A  \n B ";
    let table = Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![TableCell {
                break_after: false,
                kind: mant_ir::TableCellKind::Text,
                column_span: 1,
                row_span: 1,
                alignment: None,
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: value.into(),
                    }],
                    inline_layout: layout(&[(0, 2), (1, -4)]),
                    layout: LayoutHint::default(),
                    source: None,
                }],
            }],
        }],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    };
    let markdown =
        render_blocks_fragment(&[table], MarkdownFragmentOptions::default()).join("\n\n");
    assert_eq!(markdown, "```\n   A  \n B \n```");
    let parsed = parse_content(&markdown, None).unwrap();
    let Block::Preformatted { children, .. } = &parsed.document.as_ref().unwrap().blocks[0] else {
        panic!("literal table projection")
    };
    assert_eq!(mant_ir::inline_plain_text(children), "   A  \n B ");
}
