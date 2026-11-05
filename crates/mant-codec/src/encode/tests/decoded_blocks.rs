//! Non-inline entry points share safe scalar and code/fence projection rules.
use super::*;

fn text(document: &Document) -> String {
    #[derive(Default)]
    struct Text(String);
    impl<'ir> Visit<'ir> for Text {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match inline {
                Inline::Text { value } | Inline::Code { value } => self.0.push_str(value),
                Inline::LineBreak {} => self.0.push('\n'),
                _ => walk_inline(self, inline),
            }
        }
    }
    let mut text = Text::default();
    text.visit_document(document);
    text.0
}

fn table(block: Block) -> Block {
    Block::Table {
        borders: mant_ir::TableBorders::default(),
        alignment: None,
        column_count: None,
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::default(),
            column_borders: vec![],
            cells: vec![TableCell {
                blocks: vec![block],
                kind: mant_ir::TableCellKind::Text,
                break_after: false,
                column_span: 1,
                row_span: 1,
                alignment: None,
                preferred_width_columns: None,
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn projection_blocks(raw: &str) -> [Block; 6] {
    let children = vec![Inline::Text { value: raw.into() }];
    [
        paragraph(children.clone()),
        Block::Preformatted {
            children: children.clone(),
            inline_layout: mant_ir::InlineLayout::default(),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Unsupported {
            name: None,
            text: raw.into(),
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Equation {
            expression: None,
            value: raw.into(),
            display: false,
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Equation {
            expression: None,
            value: raw.into(),
            display: true,
            layout: LayoutHint::default(),
            source: None,
        },
        table(paragraph(children)),
    ]
}

#[test]
fn fallback_and_literal_blocks_do_not_bypass_safe_decoded_content_export() {
    for (raw, normalized) in [
        ("A\x0cB\x1bC\u{81}D", "A B C  D"),
        ("A\u{a0}B\u{2028}C\u{200b}D", "A\u{a0}B\u{2028}C\u{200b}D"),
    ] {
        for original in projection_blocks(raw) {
            let mut blocks = vec![original];
            let mut expected = None;
            for round in 1..=2 {
                let markdown = render_blocks_fragment(&blocks, MarkdownFragmentOptions::default())
                    .join("\n\n");
                assert!(
                    markdown.chars().all(
                        |character| !character.is_control() || matches!(character, '\n' | '\t')
                    ),
                    "round {round}: {markdown:?}"
                );
                let parsed = crate::parse_markdown(&markdown, None).unwrap().document;
                assert_eq!(parsed.diagnostics, [], "round {round}: {markdown:?}");
                let actual = text(&parsed);
                assert!(actual.contains(normalized), "round {round}: {actual:?}");
                if let Some(expected) = &expected {
                    assert_eq!(&actual, expected, "stable second read");
                } else {
                    expected = Some(actual);
                }
                blocks = parsed.blocks;
            }
        }
    }
}
