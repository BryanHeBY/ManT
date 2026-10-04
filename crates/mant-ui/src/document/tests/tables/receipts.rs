//! Table receipts regressions with unchanged source and geometry expectations.
use super::super::*;

#[test]
fn nested_table_receipts_keep_parent_origins_and_completed_rows() {
    let cell = |blocks| TableCell {
        break_after: false,
        blocks,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    for (parent, offset, expected) in [(4090, 0, 4090), (5, -3, 2), (-2, 3, 1)] {
        let inner = Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    cell(vec![paragraph("LEFT")]),
                    cell(vec![paragraph("RIGHT")]),
                ],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![500, 1],
                ..Default::default()
            },
            layout: LayoutHint {
                indent_columns: offset,
                ..Default::default()
            },
            source: None,
        };
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell(vec![inner])],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![1],
                ..Default::default()
            },
            layout: LayoutHint {
                indent_columns: parent,
                ..Default::default()
            },
            source: None,
        }];
        let rendered = DocumentView::new(&query).render(u16::MAX);
        assert_eq!(rendered.search("LEFT")[0].start_column, expected);
        if parent == 4090 {
            assert_eq!(rendered.search("RIGHT")[0].start_column, expected);
        }
    }
    for widths in [vec![], vec![8]] {
        let inner = Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell(vec![
                    paragraph("FIRST"),
                    Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    },
                ])],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths,
                ..Default::default()
            },
            layout: LayoutHint::default(),
            source: None,
        };
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell(vec![inner]), cell(vec![paragraph("SECOND")])],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![1, 1],
                ..Default::default()
            },
            layout: LayoutHint::default(),
            source: None,
        }];
        let rendered = DocumentView::new(&query).render(80);
        assert_eq!(
            rendered.search("SECOND")[0].row - rendered.search("FIRST")[0].row,
            2
        );
    }
}

#[test]
fn nested_table_open_tail_receipts_propagate_across_every_cached_layer() {
    // Exact nested literal source ran all five pristine profiles before this
    // assertion: termp_bd_post/term_flushln expose an open field continuation;
    // term_vspace separately completes a blank row. This model fixes the
    // reading contract without copying native macro spacing into layout.
    let cell = |blocks| TableCell {
        break_after: false,
        blocks,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    for depth in [1, 2, 4] {
        for (value, distance) in [("FIRST\n", 1), ("FIRST\n\n", 2), ("FIRST\n \n", 2)] {
            let mut inner = Block::Preformatted {
                children: vec![
                    Inline::Text {
                        value: value.into(),
                    },
                    Inline::anchor("nested-tail"),
                ],
                inline_layout: mant_ir::InlineLayout::default(),
                language: None,
                layout: LayoutHint::default(),
                source: None,
            };
            for _ in 0..depth {
                inner = Block::Table {
                    rows: vec![TableRow {
                        kind: mant_ir::TableRowKind::Data,
                        cells: vec![cell(vec![inner])],
                    }],
                    column_preferences: mant_ir::ColumnPreferences {
                        widths: vec![8],
                        ..Default::default()
                    },
                    layout: LayoutHint::default(),
                    source: None,
                };
            }
            let mut query = bundle();
            let document = query.document.as_mut().unwrap();
            document.sections.clear();
            document.blocks = vec![Block::Table {
                rows: vec![TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: vec![cell(vec![inner]), cell(vec![paragraph("SECOND")])],
                }],
                column_preferences: mant_ir::ColumnPreferences {
                    widths: vec![8, 8],
                    ..Default::default()
                },
                layout: LayoutHint::default(),
                source: None,
            }];
            let rendered = DocumentView::new(&query).render(80);
            let first = rendered.search("FIRST")[0].clone();
            let second = rendered.search("SECOND")[0].clone();
            assert_eq!(second.row - first.row, distance, "depth={depth}, {value:?}");
            assert_eq!(rendered.anchor_row("nested-tail"), Some(second.row));
        }
    }
}
