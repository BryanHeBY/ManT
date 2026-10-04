//! Shared actual-parent gaps retain source styling, links and copy ranges.

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

fn table(cells: Vec<TableCell>, origin: i32, gap: u16) -> Block {
    Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells,
        }],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint {
            indent_columns: origin,
            spacing_before_lines: gap,
            ..Default::default()
        },
        source: None,
    }
}

fn body(gap: u16) -> Block {
    Block::Paragraph {
        children: vec![Inline::Strong {
            children: vec![Inline::Link {
                children: vec![Inline::Text {
                    value: "BODY LONGWORD CONTINUATION".into(),
                }],
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.test/gaps".into(),
                },
                title: None,
            }],
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint {
            indent_columns: 3,
            spacing_before_lines: gap,
            ..Default::default()
        },
        source: None,
    }
}

fn gap(rows: u16) -> Block {
    Block::VerticalSpace {
        lines: rows,
        source: None,
    }
}

fn assert_layout(blocks: Vec<Block>, first: &str, distance: usize, column: usize) {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = blocks;
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    for width in [20, 80] {
        let rendered = DocumentView::new(&restored).render(width);
        let before = &rendered.search(first)[0];
        let body = &rendered.search("BODY")[0];
        assert_eq!(body.row - before.row, distance, "width={width}");
        assert_eq!(body.start_column, column, "width={width}");
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: body.row,
                    column: body.start_column
                },
                focus: TextPosition {
                    row: body.row,
                    column: body.end_column - 1
                },
            }),
            "BODY"
        );
        assert_eq!(
            rendered.link_target_at(body.row, body.start_column),
            Some(&LinkTarget::External(
                ExternalUri::parse("https://example.test/gaps").unwrap()
            ))
        );
        assert!(rendered.text.lines[body.row].spans.iter().any(|span| {
            span.content.contains("BODY") && span.style.add_modifier.contains(Modifier::BOLD)
        }));
        if width == 20 {
            assert!(rendered.search("CONTINUATION")[0].row > body.row);
        }
    }
}

#[test]
fn leading_requests_share_parent_budget_through_nested_cells_and_wrapped_links() {
    for depth in [0, 1, 2, 4] {
        let mut inner = body(3000);
        for _ in 0..depth {
            inner = table(vec![cell(vec![inner], false)], 0, 0);
        }
        assert_layout(
            vec![
                paragraph("BEFORE"),
                table(vec![cell(vec![inner], false)], -2, 3000),
            ],
            "BEFORE",
            4097,
            1,
        );
    }
    for declared in [false, true] {
        let mut independent = table(vec![cell(vec![body(3000)], false)], 2, 3000);
        if declared
            && let Block::Table {
                column_preferences, ..
            } = &mut independent
        {
            column_preferences.widths = vec![8];
        }
        assert_layout(vec![paragraph("BEFORE"), independent], "BEFORE", 6001, 5);
    }
}

#[test]
fn trailing_and_leading_requests_share_a_cursor_and_gap_only_closes_once() {
    for break_after in [false, true] {
        assert_layout(
            vec![table(
                vec![
                    cell(vec![paragraph("FIRST"), gap(3000)], break_after),
                    cell(vec![body(3000)], false),
                ],
                -2,
                0,
            )],
            "FIRST",
            if break_after { 6001 } else { 4097 },
            1,
        );
        assert_layout(
            vec![
                paragraph("BEFORE"),
                table(
                    vec![
                        cell(vec![gap(3000)], break_after),
                        cell(vec![body(0)], false),
                    ],
                    -2,
                    3000,
                ),
            ],
            "BEFORE",
            4097,
            1,
        );
        assert_layout(
            vec![
                paragraph("BEFORE"),
                table(vec![cell(vec![gap(3000)], break_after)], -2, 0),
                gap(3000),
                body(0),
            ],
            "BEFORE",
            6001,
            3,
        );
    }
}

#[test]
fn positive_topology_fallback_keeps_the_original_independent_gap_policy() {
    for origin in [-2, 2] {
        let mut block = table(vec![cell(vec![body(3000)], false)], origin, 3000);
        if let Block::Table {
            rows,
            column_preferences,
            ..
        } = &mut block
        {
            column_preferences.widths = vec![8];
            rows[0].cells[0].column_span = 2;
        }
        assert_layout(
            vec![paragraph("BEFORE"), block],
            "BEFORE",
            if origin < 0 { 4097 } else { 6001 },
            if origin < 0 { 1 } else { 5 },
        );
    }
}

#[test]
fn saturated_gap_only_completion_survives_nested_json_without_a_placeholder() {
    for depth in [1, 2, 4] {
        for break_after in [false, true] {
            let mut inner = table(vec![cell(vec![gap(3000)], break_after)], 0, 0);
            for _ in 1..depth {
                inner = table(vec![cell(vec![inner], break_after)], 0, 0);
            }
            let outer = table(vec![cell(vec![inner], break_after)], -2, 4096);
            assert_layout(vec![paragraph("BEFORE"), outer, body(0)], "BEFORE", 4097, 3);
        }
    }
}
