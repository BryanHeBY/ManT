use super::*;

#[test]
fn signed_table_cells_compose_parent_origins_before_clipping() {
    let renderer = super::super::super::plain_renderer();
    for (table_indent, child_indent, expected_column) in
        [(-2, 3, 1), (3, -2, 1), (4096, 3, 4096), (4090, 10, 4096)]
    {
        let cell = |text| TableCell {
            kind: mant_ir::TableCellKind::Text,
            blocks: vec![paragraph(text, child_indent)],
            column_span: 2,
            row_span: 1,
            alignment: None,
        };
        let table = Block::Table {
            column_widths: Vec::new(),
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
            column_widths: Vec::new(),
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![TableCell {
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
        column_widths: Vec::new(),
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: ["FIRST", "SECOND"]
                .map(|text| TableCell {
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
        column_widths: Vec::new(),
        rows: vec![
            mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    TableCell {
                        kind: mant_ir::TableCellKind::HorizontalRule,
                        blocks: Vec::new(),
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                    TableCell {
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
    // termp_it_pre caps its preceding-offset loop at ncols. Exact roff
    // oracle input was run first; the parser joins trailing Ta operands,
    // while this source-neutral IR deliberately keeps four actual cells.
    let block = declared_column_table(&[3, 3], &["A", "B", "C", "D"]);
    assert_eq!(
        renderer.render_blocks(&[block], 0),
        "A      B      C\n              D"
    );
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
