//! Closed table cells retain source positions and never invent blank rows.

use super::*;

fn cell(blocks: Vec<Block>, break_after: bool) -> TableCell {
    TableCell {
        blocks,
        break_after,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

fn table(cells: Vec<TableCell>, preferences: mant_ir::ColumnPreferences) -> Block {
    Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells,
        }],
        column_preferences: preferences,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn literal(value: &str) -> Block {
    Block::Preformatted {
        children: vec![Inline::Text {
            value: value.into(),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn preferences() -> Vec<mant_ir::ColumnPreferences> {
    vec![
        mant_ir::ColumnPreferences::default(),
        mant_ir::ColumnPreferences {
            widths: vec![8, 8],
            ..Default::default()
        },
        mant_ir::ColumnPreferences {
            widths: vec![u16::MAX, 1],
            ..Default::default()
        },
        mant_ir::ColumnPreferences {
            gap_columns: u16::MAX,
            ..Default::default()
        },
    ]
}

fn assert_distance(block: Block, distance: usize) {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![block];
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    for width in [20, 80, u16::MAX] {
        let rendered = DocumentView::new(&restored).render(width);
        let first = &rendered.search("FIRST")[0];
        let second = &rendered.search("SECOND")[0];
        assert_eq!(second.row - first.row, distance, "width={width}");
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: first.row,
                    column: first.start_column
                },
                focus: TextPosition {
                    row: first.row,
                    column: first.end_column - 1
                },
            }),
            "FIRST"
        );
    }
}

#[test]
fn occupied_boundaries_and_completed_empty_rows_survive_json_and_copy() {
    for preferences in preferences() {
        for (first, distance) in [
            (vec![paragraph("FIRST")], 1),
            (vec![literal("FIRST\n")], 2),
            (
                vec![
                    paragraph("FIRST"),
                    Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    },
                ],
                2,
            ),
        ] {
            assert_distance(
                table(
                    vec![cell(first, true), cell(vec![paragraph("SECOND")], false)],
                    preferences.clone(),
                ),
                distance,
            );
        }
    }
}

#[test]
fn nested_boundaries_keep_occupied_and_empty_rows_distinct() {
    for preferences in preferences() {
        for (first, distance) in [
            (vec![paragraph("FIRST")], 1),
            (vec![literal("FIRST\n")], 2),
            (
                vec![
                    paragraph("FIRST"),
                    Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    },
                ],
                2,
            ),
        ] {
            let inner = table(vec![cell(first, true)], preferences.clone());
            assert_distance(
                table(
                    vec![
                        cell(vec![inner], false),
                        cell(vec![paragraph("SECOND")], false),
                    ],
                    preferences.clone(),
                ),
                distance,
            );
        }
    }
}

#[test]
fn empty_data_boundaries_keep_first_middle_nested_and_eof_rows() {
    // Fresh NBRZW/\p controls establish the closed invisible data row;
    // term_fill's acceptance and term_flushln's close survive glyph retirement.
    for preferences in preferences() {
        for nested in [false, true] {
            let mut query = bundle();
            let document = query.document.as_mut().unwrap();
            document.sections.clear();
            let empty = if nested {
                vec![table(vec![cell(vec![], true)], preferences.clone())]
            } else {
                Vec::new()
            };
            document.blocks = vec![table(
                vec![cell(empty, !nested), cell(vec![paragraph("SECOND")], false)],
                preferences.clone(),
            )];
            let rendered = DocumentView::new(&query).render(80);
            let second = &rendered.search("SECOND")[0];
            let mut baseline = query.clone();
            baseline.document.as_mut().unwrap().blocks = vec![table(
                vec![cell(vec![paragraph("SECOND")], false)],
                preferences.clone(),
            )];
            let baseline = DocumentView::new(&baseline).render(80);
            assert_eq!(second.row, baseline.search("SECOND")[0].row + 1);
            assert_eq!(rendered.text.lines[second.row - 1].to_string().trim(), "");
        }
        assert_distance(
            table(
                vec![
                    cell(vec![paragraph("FIRST")], false),
                    cell(vec![], true),
                    cell(vec![paragraph("SECOND")], false),
                ],
                preferences.clone(),
            ),
            1,
        );
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![table(vec![cell(vec![], true)], preferences.clone())];
        let rendered = DocumentView::new(&query).render(80);
        let mut baseline = query.clone();
        baseline.document.as_mut().unwrap().blocks = vec![table(
            vec![cell(vec![paragraph("OCCUPIED")], false)],
            preferences,
        )];
        let baseline = DocumentView::new(&baseline).render(80);
        assert_eq!(rendered.text.lines.len(), baseline.text.lines.len());
        assert_eq!(rendered.text.lines.last().unwrap().to_string().trim(), "");
    }
}

#[test]
fn closed_anchor_hint_rows_keep_json_navigation_and_empty_source_copy() {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![table(
        vec![
            cell(
                vec![Block::Paragraph {
                    children: vec![Inline::anchor("empty-data")],
                    inline_layout: mant_ir::InlineLayout {
                        row_hints: vec![mant_ir::RowLayoutHint {
                            row: 0,
                            indent_columns: 12,
                        }],
                    },
                    layout: LayoutHint::default(),
                    source: None,
                }],
                true,
            ),
            cell(vec![paragraph("SECOND")], false),
        ],
        mant_ir::ColumnPreferences::default(),
    )];
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    let rendered = DocumentView::new(&restored).render(80);
    let empty = rendered.anchor_row("empty-data").unwrap();
    assert_eq!(rendered.search("SECOND")[0].row, empty + 1);
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: empty,
                column: 0
            },
            focus: TextPosition {
                row: empty,
                column: 12
            },
        }),
        ""
    );
}

fn linked_literal(value: &str) -> Block {
    Block::Preformatted {
        children: vec![Inline::Strong {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.test/tail".into(),
                },
                title: None,
                children: vec![
                    Inline::Text {
                        value: value.into(),
                    },
                    Inline::anchor("tail"),
                ],
            }],
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn generic_fallback_tail_receipts_keep_nested_json_styles_links_and_copy() {
    for depth in [1, 2, 4] {
        for (value, distance) in [("FIRST\n", 1), ("FIRST\n\n", 2), ("FIRST\n \n", 2)] {
            let mut inner = table(
                vec![
                    cell(vec![paragraph("PREFIX")], false),
                    cell(vec![linked_literal(value)], false),
                ],
                mant_ir::ColumnPreferences {
                    gap_columns: u16::MAX,
                    ..Default::default()
                },
            );
            for _ in 1..depth {
                inner = table(
                    vec![cell(vec![inner], false)],
                    mant_ir::ColumnPreferences::default(),
                );
            }
            let outer = table(
                vec![
                    cell(vec![inner], false),
                    cell(vec![paragraph("SECOND")], false),
                ],
                mant_ir::ColumnPreferences::default(),
            );
            assert_distance(outer.clone(), distance);
            assert_fallback_link_style(outer, distance);
        }
    }
}

fn assert_fallback_link_style(block: Block, distance: usize) {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![block];
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    for width in [20, 80, u16::MAX] {
        let rendered = DocumentView::new(&restored).render(width);
        let first = &rendered.search("FIRST")[0];
        let second = &rendered.search("SECOND")[0];
        assert_eq!(second.row - first.row, distance);
        assert_eq!(
            rendered.link_target_at(first.row, first.start_column),
            Some(&LinkTarget::External(
                ExternalUri::parse("https://example.test/tail").unwrap()
            ))
        );
        assert!(rendered.text.lines[first.row].spans.iter().any(|span| {
            span.content.contains("FIRST") && span.style.add_modifier.contains(Modifier::BOLD)
        }));
        assert!(rendered.anchor_row("tail").unwrap() < second.row);
    }
}

#[test]
fn structural_data_rows_separate_incoming_gap_budgets_and_keep_empty_copy() {
    for preferences in preferences() {
        for origin in [0, -2] {
            let mut empty = table(Vec::new(), preferences.clone());
            if let Block::Table { layout, .. } = &mut empty {
                layout.indent_columns = origin;
            }
            let mut query = bundle();
            let document = query.document.as_mut().unwrap();
            document.sections.clear();
            document.blocks = vec![
                paragraph("BEFORE"),
                Block::VerticalSpace {
                    lines: 3000,
                    source: None,
                },
                empty,
                Block::VerticalSpace {
                    lines: 3000,
                    source: None,
                },
                paragraph("AFTER"),
            ];
            let rendered = DocumentView::new(&query).render(80);
            let before = &rendered.search("BEFORE")[0];
            let after = &rendered.search("AFTER")[0];
            assert_eq!(after.row - before.row, 6002);
            let blank = before.row + 3001;
            assert_eq!(rendered.text.lines[blank].to_string(), "");
            assert_eq!(
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: blank,
                        column: 0
                    },
                    focus: TextPosition {
                        row: blank,
                        column: 12
                    },
                }),
                ""
            );
        }
    }
}
