use super::*;

#[test]
fn signed_table_cells_compose_parent_origins_before_clipping() {
    let renderer = super::super::super::plain_renderer();
    for (table_indent, child_indent, expected_column) in
        [(-2, 3, 1), (3, -2, 1), (4096, 3, 4096), (4090, 10, 4096)]
    {
        let cell = |text| TableCell {
            break_after: false,
            kind: mant_ir::TableCellKind::Text,
            blocks: vec![paragraph(text, child_indent)],
            column_span: 2,
            row_span: 1,
            alignment: None,
        };
        let table = Block::Table {
            column_preferences: mant_ir::ColumnPreferences::default(),
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("FIRST"), cell("SECOND")],
            }],
            layout: LayoutHint {
                indent_columns: table_indent,
                ..Default::default()
            },
            source: None,
        };
        assert_eq!(
            renderer.render_blocks(&[table], 0),
            format!(
                "{}FIRST\n{}SECOND",
                " ".repeat(expected_column),
                " ".repeat(expected_column)
            )
        );
    }
    let nested = plain_list(
        vec![Block::Table {
            column_preferences: mant_ir::ColumnPreferences::default(),
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![plain_list(vec![paragraph("NESTED", 5)], 3)],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                }],
            }],
            layout: LayoutHint {
                indent_columns: 2,
                ..Default::default()
            },
            source: None,
        }],
        4090,
    );
    assert_eq!(
        renderer.render_blocks(&[nested], 0),
        format!("{}NESTED", " ".repeat(4096))
    );
    let table = Block::Table {
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: ["FIRST", "SECOND"]
                .map(|text| TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph(text, 0)],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                })
                .into(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let ordinary = renderer.render_blocks(&[table], 0);
    assert!(
        ordinary
            .lines()
            .any(|line| line.contains("FIRST") && line.contains("SECOND")),
        "{ordinary}"
    );
}

#[test]
fn stacked_tables_preserve_partial_whole_layout_rules_and_empty_rows() {
    let renderer = super::super::super::plain_renderer();
    let table = Block::Table {
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![
            mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    TableCell {
                        break_after: false,
                        kind: mant_ir::TableCellKind::HorizontalRule,
                        blocks: Vec::new(),
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                    TableCell {
                        break_after: false,
                        kind: mant_ir::TableCellKind::Text,
                        blocks: vec![paragraph("VISIBLE", -1)],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                ],
            },
            mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: Vec::new(),
            },
            mant_ir::TableRow {
                kind: mant_ir::TableRowKind::DoubleHorizontalRule,
                cells: Vec::new(),
            },
            mant_ir::TableRow {
                kind: mant_ir::TableRowKind::LayoutRule {
                    cells: vec![
                        mant_ir::TableRuleCellKind::Horizontal,
                        mant_ir::TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                cells: Vec::new(),
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(
        renderer.render_blocks(&[table], 0),
        "---\nVISIBLE\n\n===\n--- | ==="
    );
}

#[test]
fn declared_columns_pad_to_their_declared_offsets() {
    let renderer = super::super::super::plain_renderer();
    // mdoc_term.c::termp_it_pre (709-715): column 1 starts after the
    // first declared width plus the dcol gap of 4 (fewer than five
    // columns). Verified against the fixed -Tutf8 reference.
    let block = declared_column_table(&[3, 3], &["A", "B"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "A      B");
}

#[test]
fn declared_column_advances_truncate_at_256_columns() {
    let renderer = super::super::super::plain_renderer();
    // term_ascii.c::ascii_advance() truncates one advance at 256
    // columns; the fixed reference puts B 257 columns after A for a
    // 300-column declaration (300 + 4 - 1 = 303, clamped to 256).
    let block = declared_column_table(&[300, 10], &["A", "B"]);
    let expected = format!("A{}B", " ".repeat(256));
    assert_eq!(renderer.render_blocks(&[block], 0), expected);
    // A gap of exactly 255 remains unclamped.
    let block = declared_column_table(&[252, 10], &["A", "B"]);
    assert_eq!(
        renderer.render_blocks(&[block], 0),
        format!("A{}B", " ".repeat(255))
    );
}

#[test]
fn saturated_column_widths_keep_bounded_and_aligned_projection() {
    let renderer = super::super::super::plain_renderer();
    // External audit CW01: 65531/65532/65535/65536-column declarations
    // must neither overflow u16 arithmetic nor explode padding. Every
    // column boundary still advances one truncated 256-column step.
    for width in [65531_u16, 65532, 65535, u16::MAX] {
        let block = declared_column_table(&[width, width], &["A", "B"]);
        assert_eq!(
            renderer.render_blocks(&[block], 0),
            format!("A{}B", " ".repeat(256)),
            "width {width}"
        );
        let block = declared_column_table(&[width, width, width], &["A", "B", "C"]);
        assert_eq!(
            renderer.render_blocks(&[block], 0),
            format!("A{}B{}C", " ".repeat(256), " ".repeat(256)),
            "width {width}"
        );
    }
    // Zero and one column declarations keep the dcol-only separation.
    let block = declared_column_table(&[0, 0], &["A", "B"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "A   B");
    let block = declared_column_table(&[1, 1], &["A", "B"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "A    B");
}

#[test]
fn full_column_starts_wrap_to_the_next_physical_line() {
    let renderer = super::super::super::plain_renderer();
    // External audit CW03/CW04: content that already fills the next
    // column's start moves that column to a fresh physical line at its
    // declared start. The fixed reference renders `.Bl -column one two`
    // with `.It AAAA BB Ta C` as `AAAA BB` then `C` at the declared
    // column offset (term.c moves the field once viscol reaches the
    // field's rmargin).
    let block = declared_column_table(&[3, 3], &["AAAA BB", "C"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "AAAA BB\n       C");
    // Ending one column earlier keeps the same-line continuation.
    let block = declared_column_table(&[3, 3], &["AAAA B", "C"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "AAAA B C");
    // Overrunning content survives untruncated and later columns keep
    // their boundaries (reference: `.It AAAA BBX Ta C`).
    let block = declared_column_table(&[3, 3], &["AAAA BBX", "C"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "AAAA BBX\n       C");
}

#[test]
fn undeclared_cells_keep_their_source_order_without_an_invented_stride() {
    let renderer = super::super::super::plain_renderer();
    // Exact .Bl -column "xxx" "xxx" / .It A Ta B Ta C Ta D ran first:
    // the pristine tree has four It bodies, and ASCII/UTF-8 print CD together.
    // termp_it_pre caps prior offsets at ncols, keeps excess width 10 without
    // adding dcol, and term_flushln compares the previous configured end
    // (mdoc_term.c:684-736, term.c:233-253).
    let block = declared_column_table(&[3, 3], &["A", "B", "C", "D"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "A      B      CD");
    // CW02's many-cell boundary reaches the shared dense-placement
    // budget and uses existing source-order stack output. No derived
    // stride multiplication or large padding allocation is permitted.
    let cells: Vec<&str> = (0..4700).map(|_| "x").collect();
    let block = declared_column_table(&[3, 3], &cells);
    let text = renderer.render_blocks(&[block], 0);
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 4700);
    assert!(lines.iter().all(|line| *line == "x"));
    assert!(text.len() <= 4700 * 2);
}

#[test]
fn generic_preferences_preserve_default_typography_and_custom_gaps() {
    let renderer = super::super::super::plain_renderer();
    for (gap, expected) in [(0, "A|B"), (1, "A| B"), (2, "A | B"), (6, "A   |   B")] {
        let mut block = declared_column_table(&[], &["A", "B"]);
        if let Block::Table {
            column_preferences, ..
        } = &mut block
        {
            column_preferences.gap_columns = gap;
        }
        assert_eq!(renderer.render_blocks(&[block], 0), expected);
    }
    for (limit, origin) in [(None, 600), (Some(0), 1), (Some(256), 257)] {
        let mut block = declared_column_table(&[598, 1], &["A", "B"]);
        if let Block::Table {
            column_preferences, ..
        } = &mut block
        {
            *column_preferences = mant_ir::ColumnPreferences {
                widths: vec![598, 1],
                advance_limit_columns: limit,
                ..Default::default()
            };
        }
        assert_eq!(
            renderer.render_blocks(&[block], 0),
            format!("A{}B", " ".repeat(origin - 1))
        );
    }
}

#[test]
fn declared_excess_fields_close_at_their_own_capacity() {
    // Exact excess/excess-9/10/11 sources ran the pristine oracle before
    // these assertions. mdoc_term.c:654-745 gives excess fields width 10,
    // without dcol, and term.c:233-253 compares the preceding configured end.
    let renderer = super::super::super::plain_renderer();
    for length in [1, 9, 10, 11] {
        let content = "C".repeat(length);
        let block = declared_column_table(&[8], &["A", &content, "D"]);
        let expected = if length >= 10 {
            format!("A{}{content}\n{}D", " ".repeat(11), " ".repeat(12))
        } else {
            format!("A{}{content}D", " ".repeat(11))
        };
        assert_eq!(renderer.render_blocks(&[block], 0), expected);
    }
}

#[test]
fn generated_padding_fallback_preserves_every_serialized_cell_once() {
    let renderer = super::super::super::plain_renderer();
    for count in [256, 257] {
        let words = (0..count)
            .map(|index| format!("CELL_{index:03}"))
            .collect::<Vec<_>>();
        let borrowed = words.iter().map(String::as_str).collect::<Vec<_>>();
        let mut block = declared_column_table(&vec![u16::MAX; count], &borrowed);
        if let Block::Table {
            column_preferences, ..
        } = &mut block
        {
            column_preferences.advance_limit_columns = None;
        }
        let wire = serde_json::to_string(&block).unwrap();
        assert!(!wire.contains("columnWidths"));
        let block: Block = serde_json::from_str(&wire).unwrap();
        let output = renderer.render_blocks(&[block], 0);
        assert_eq!(output.lines().collect::<Vec<_>>(), borrowed);
    }
    // The final parent-plus-preferred origin is bounded, while long author
    // text remains intact in the source-order fallback.
    let author = "X".repeat(8000);
    let mut block = declared_column_table(&[5, 1], &[&author, "AFTER"]);
    if let Block::Table {
        layout,
        column_preferences,
        ..
    } = &mut block
    {
        layout.indent_columns = 4090;
        column_preferences.gap_columns = 2;
        column_preferences.advance_limit_columns = None;
    }
    let output = renderer.render_blocks(&[block], 0);
    assert_eq!(
        output,
        format!("{}{author}\n{}AFTER", " ".repeat(4090), " ".repeat(4090))
    );
}

#[test]
fn placed_fields_compose_parent_child_and_hint_before_leaf_clipping() {
    // Source-neutral counterpart of the offset/Bd exact oracle, already run.
    // mdoc_term.c::termp_bd_pre retains the actual parent until the visible
    // leaf. A preferred field must not translate a previously clipped child.
    let renderer = super::super::super::plain_renderer();
    for (indent, correction, nested, shared, column) in [
        (0, 0, false, true, 4096),
        (2, 0, false, false, 4092),
        (0, 2, false, false, 4092),
        (-2, 0, false, false, 4088),
        (-10, 0, true, false, 4090),
    ] {
        let mut body = paragraph("FIELD_B", indent);
        if let Block::Paragraph { inline_layout, .. } = &mut body
            && correction != 0
        {
            inline_layout.row_hints.push(mant_ir::RowLayoutHint {
                row: 0,
                indent_columns: correction,
            });
        }
        if nested {
            body = plain_list(vec![body], 10);
        }
        let cell = |block| TableCell {
            break_after: false,
            blocks: vec![block],
            kind: mant_ir::TableCellKind::Text,
            column_span: 1,
            row_span: 1,
            alignment: None,
        };
        let block = Block::Table {
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell(paragraph("A", 0)), cell(body)],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![4, 1],
                ..Default::default()
            },
            layout: LayoutHint {
                indent_columns: 4090,
                ..Default::default()
            },
            source: None,
        };
        let block: Block = serde_json::from_str(&serde_json::to_string(&block).unwrap()).unwrap();
        let output = renderer.render_blocks(&[block], 0);
        let rows = output.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), if shared { 1 } else { 2 });
        assert_eq!(
            rows.last().unwrap().find("FIELD_B"),
            Some(column),
            "{indent}/{correction}/{nested}"
        );
        assert_eq!(output.matches("FIELD_B").count(), 1);
    }
}

#[test]
fn derived_dense_sparse_and_rule_paths_bound_actual_origins_and_padding() {
    let renderer = super::super::super::plain_renderer();
    let mut dense = navigation_table(
        &[],
        4090,
        vec![vec![paragraph("A", 0)], vec![paragraph("B", 2)]],
    );
    if let Block::Table {
        column_preferences, ..
    } = &mut dense
    {
        column_preferences.gap_columns = 6;
    }
    assert_eq!(
        renderer.render_blocks(&[dense], 0),
        format!("{}A\n{}B", " ".repeat(4090), " ".repeat(4092))
    );
    let mut sparse = navigation_table(&[], 0, vec![vec![paragraph("B", 4094)]]);
    if let Block::Table { rows, .. } = &mut sparse {
        rows[0].cells[0].column_span = u16::MAX;
    }
    assert_eq!(
        renderer.render_blocks(&[sparse], 0),
        format!("{}B", " ".repeat(4094))
    );
    let cells = vec!["X"; 256];
    let mut extreme = declared_column_table(&[], &cells);
    if let Block::Table {
        column_preferences,
        rows,
        ..
    } = &mut extreme
    {
        column_preferences.gap_columns = u16::MAX;
        rows.push(mant_ir::TableRow {
            cells: vec![],
            kind: mant_ir::TableRowKind::LayoutRule {
                cells: vec![mant_ir::TableRuleCellKind::Horizontal; 256],
            },
        });
    }
    let wire = serde_json::to_string(&extreme).unwrap();
    let extreme = serde_json::from_str(&wire).unwrap();
    let output = renderer.render_blocks(&[extreme], 0);
    assert_eq!(output.lines().filter(|line| *line == "X").count(), 256);
    assert_eq!(output.lines().filter(|line| *line == "---").count(), 256);
    assert!(output.len() < 2048);
}

#[test]
fn nested_tables_keep_actual_parent_and_completed_tail_receipts() {
    // The nested-completed exact source ran all five pristine profiles first;
    // term_vspace completes a row, independently of leaving a nested Bl.
    // This pure IR counter isolates that receipt from native macro spacing.
    let renderer = super::super::super::plain_renderer();
    for (parent, inner_offset, expected) in [(4090, 0, 4090), (5, -3, 2), (-2, 3, 1)] {
        let mut inner = declared_column_table(&[500, 1], &["LEFT", "RIGHT"]);
        if let Block::Table {
            layout,
            column_preferences,
            ..
        } = &mut inner
        {
            layout.indent_columns = inner_offset;
            column_preferences.advance_limit_columns = None;
        }
        let outer = navigation_table(&[1], parent, vec![vec![inner]]);
        let output = renderer.render_blocks(&[outer], 0);
        assert_eq!(output.lines().next().unwrap().find("LEFT"), Some(expected));
        if parent == 4090 {
            assert_eq!(output.lines().last().unwrap().find("RIGHT"), Some(expected));
            assert_eq!(output.lines().count(), 2);
        }
    }
    for inner_widths in [&[][..], &[8_u16][..]] {
        let mut inner = navigation_table(
            inner_widths,
            0,
            vec![vec![
                paragraph("A", 0),
                Block::VerticalSpace {
                    lines: 1,
                    source: None,
                },
            ]],
        );
        if let Block::Table {
            column_preferences, ..
        } = &mut inner
        {
            column_preferences.advance_limit_columns = None;
        }
        let outer = navigation_table(&[1, 1], 0, vec![vec![inner], vec![paragraph("B", 0)]]);
        assert_eq!(renderer.render_blocks(&[outer], 0), "A\n\nB");
    }
}

#[test]
fn derived_literal_open_and_completed_rows_keep_distinct_receipts() {
    let renderer = super::super::super::plain_renderer();
    for (value, expected) in [("A\n", "A"), ("A\n\n", "A\n\n"), ("A\n \n", "A\n ")] {
        let literal = Block::Preformatted {
            children: vec![Inline::Text {
                value: value.into(),
            }],
            inline_layout: mant_ir::InlineLayout::default(),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        };
        let block = navigation_table(&[], 0, vec![vec![literal]]);
        assert_eq!(renderer.render_blocks(&[block], 0), expected);
    }
    let mut inner = declared_column_table(&[500, 1], &["   \u{a0}", "RIGHT"]);
    if let Block::Table {
        column_preferences, ..
    } = &mut inner
    {
        *column_preferences = mant_ir::ColumnPreferences {
            widths: vec![500, 1],
            ..Default::default()
        };
    }
    let outer = navigation_table(&[1], 5, vec![vec![inner]]);
    assert_eq!(
        renderer.render_blocks(&[outer], 0),
        format!("{}   \u{a0}{}RIGHT", " ".repeat(5), " ".repeat(498))
    );
}
