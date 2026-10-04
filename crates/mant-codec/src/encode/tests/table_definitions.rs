//! Definition row/word facts survive portable tables and real Markdown reads.
use super::*;
use mant_ir::{HeadBodyRelation, InlineLayout, RowLayoutHint};

fn definition(head: Vec<Inline>, relation: HeadBodyRelation, body: Vec<Block>) -> Block {
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![head.into()],
            description: body,
            head_body_relation: relation,
            layout: mant_ir::DefinitionLayout::default(),
            source: None,
            entry: None,
        }],
        compact: true,
        declaration_groups: vec![],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn wrap(block: Block, context: &str) -> Block {
    let list = |block| Block::List {
        kind: ListKind::Plain,
        compact: true,
        items: vec![ListItem {
            blocks: vec![block],
            layout: mant_ir::ListItemLayout::default(),
            source: None,
            entry: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let table = |block| Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::default(),
            cells: vec![TableCell {
                blocks: vec![block],
                kind: mant_ir::TableCellKind::default(),
                break_after: false,
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    };
    match context {
        "root" => block,
        "list" => list(block),
        "table" => table(block),
        "nested-table" => table(table(list(block))),
        _ => unreachable!(),
    }
}

fn readback(block: &Block) -> String {
    struct Content(Vec<String>);
    impl<'a> Visit<'a> for Content {
        fn visit_block(&mut self, block: &'a Block) {
            match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    self.0.push(mant_ir::inline_plain_text(children));
                }
                _ => mant_ir::visit::walk_block(self, block),
            }
        }
    }
    let markdown = render_blocks_fragment(
        std::slice::from_ref(block),
        MarkdownFragmentOptions::default(),
    )
    .join("\n\n");
    let parsed = parse_content(&markdown, None).unwrap();
    let mut content = Content(Vec::new());
    for block in &parsed.document.unwrap().blocks {
        content.visit_block(block);
    }
    content.0.join("\n")
}

#[test]
fn every_definition_relation_survives_each_output_container() {
    for context in ["root", "list", "table", "nested-table"] {
        for relation in [
            HeadBodyRelation::joined(),
            HeadBodyRelation::separated(),
            HeadBodyRelation::Separate,
        ] {
            for gap in [0, 1, 2] {
                let mut body = vec![paragraph(vec![Inline::Text { value: "Y".into() }])];
                if gap > 0 {
                    body.insert(
                        0,
                        Block::VerticalSpace {
                            lines: gap,
                            source: None,
                        },
                    );
                }
                let block = wrap(
                    definition(vec![Inline::Text { value: "X".into() }], relation, body),
                    context,
                );
                let expected = if gap > 0 {
                    format!(
                        "X{}Y",
                        "\n".repeat(if context.contains("table") {
                            usize::from(gap) + 1
                        } else {
                            1
                        })
                    )
                } else if relation == HeadBodyRelation::Separate {
                    "X\nY".into()
                } else if relation.joins_without_separator() {
                    "XY".into()
                } else {
                    "X Y".into()
                };
                let wire = serde_json::to_string(&block).unwrap();
                let restored: Block = serde_json::from_str(&wire).unwrap();
                for value in [&block, &restored] {
                    assert_eq!(
                        readback(value),
                        expected,
                        "{context}/{relation:?}/gap={gap}"
                    );
                }
            }
        }
    }
}

#[test]
fn table_shared_rows_ignore_body_origin_but_keep_authored_spaces_and_continuations() {
    for literal in [false, true] {
        for relation in [HeadBodyRelation::joined(), HeadBodyRelation::separated()] {
            for head in ["X", "X\n", "X\n\n", "\n"] {
                for author_prefix in ["", " ", "\u{a0}"] {
                    let children = vec![Inline::Link {
                        target: mant_ir::LinkTarget::External {
                            uri: "https://example.test".into(),
                        },
                        title: None,
                        children: vec![Inline::Strong {
                            children: vec![Inline::Text {
                                value: format!("{author_prefix}Y\nZ"),
                            }],
                        }],
                    }];
                    let body_hints = InlineLayout {
                        row_hints: vec![
                            RowLayoutHint {
                                row: 0,
                                indent_columns: 7,
                            },
                            RowLayoutHint {
                                row: 1,
                                indent_columns: 3,
                            },
                        ],
                    };
                    let body = if literal {
                        Block::Preformatted {
                            children,
                            inline_layout: body_hints,
                            language: None,
                            layout: LayoutHint::default(),
                            source: None,
                        }
                    } else {
                        Block::Paragraph {
                            children,
                            inline_layout: body_hints,
                            layout: LayoutHint::default(),
                            source: None,
                        }
                    };
                    let mut block = definition(
                        vec![Inline::Code { value: head.into() }],
                        relation,
                        vec![body],
                    );
                    let Block::DefinitionList { items, .. } = &mut block else {
                        unreachable!()
                    };
                    let tail = head.bytes().filter(|&byte| byte == b'\n').count();
                    items[0].terms[0].inline_layout = InlineLayout {
                        row_hints: vec![RowLayoutHint {
                            row: u32::try_from(tail).unwrap(),
                            indent_columns: 4,
                        }],
                    };
                    let mut expected = head.to_owned();
                    if tail == 0 {
                        expected.insert_str(0, "    ");
                    } else {
                        expected.push_str("    ");
                    }
                    if !relation.joins_without_separator() {
                        expected.push(' ');
                    }
                    expected.push_str(author_prefix);
                    expected.push_str("Y\n   Z");
                    let block = wrap(block, "nested-table");
                    assert_eq!(
                        readback(&block),
                        expected,
                        "{head:?}/{relation:?}/{literal}/{author_prefix:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn empty_definition_owners_do_not_invent_colons_or_label_rows() {
    for head in [vec![], vec![Inline::anchor("head")]] {
        for body in [
            vec![],
            vec![paragraph(vec![])],
            vec![paragraph(vec![Inline::Text { value: "Y".into() }])],
        ] {
            let expected = if body.iter().any(
                |block| matches!(block, Block::Paragraph { children, .. } if !children.is_empty()),
            ) {
                "Y"
            } else {
                ""
            };
            let block = wrap(
                definition(head.clone(), HeadBodyRelation::joined(), body),
                "table",
            );
            assert_eq!(readback(&block), expected);
        }
    }
}

#[test]
fn unoccupied_heads_do_not_consume_the_body_first_row_hint() {
    for head in [
        vec![],
        vec![Inline::anchor("head")],
        vec![Inline::line_break()],
    ] {
        let occupied = head
            .iter()
            .any(|inline| matches!(inline, Inline::LineBreak {}));
        let mut body = paragraph(vec![Inline::Text { value: "Y".into() }]);
        let Block::Paragraph { inline_layout, .. } = &mut body else {
            unreachable!()
        };
        inline_layout.row_hints.push(RowLayoutHint {
            row: 0,
            indent_columns: 7,
        });
        let block = wrap(
            definition(head, HeadBodyRelation::joined(), vec![body]),
            "table",
        );
        assert_eq!(readback(&block), if occupied { "\nY" } else { "       Y" });
    }
}

#[test]
fn an_authored_empty_body_can_keep_the_shared_open_row_for_the_next_cell() {
    for empty_literal in [false, true] {
        let body = if empty_literal {
            Block::Preformatted {
                children: vec![Inline::Text {
                    value: String::new(),
                }],
                inline_layout: InlineLayout::default(),
                layout: LayoutHint::default(),
                language: None,
                source: None,
            }
        } else {
            paragraph(vec![])
        };
        let mut definition = definition(
            vec![Inline::Text {
                value: "X\n".into(),
            }],
            HeadBodyRelation::joined(),
            vec![body],
        );
        let Block::DefinitionList { items, .. } = &mut definition else {
            unreachable!()
        };
        items[0].terms[0]
            .inline_layout
            .row_hints
            .push(RowLayoutHint {
                row: 1,
                indent_columns: 4,
            });
        let mut block = wrap(definition, "table");
        let Block::Table { rows, .. } = &mut block else {
            unreachable!()
        };
        rows[0].cells.push(TableCell {
            blocks: vec![paragraph(vec![Inline::Text { value: "B".into() }])],
            break_after: false,
            kind: mant_ir::TableCellKind::Text,
            column_span: 1,
            row_span: 1,
            alignment: None,
        });
        assert_eq!(readback(&block), "X\n     | B");
    }
}
