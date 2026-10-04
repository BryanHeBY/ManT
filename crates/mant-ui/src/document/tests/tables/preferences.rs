//! Table preferences regressions with unchanged source and geometry expectations.
use super::super::*;

#[test]
fn content_derived_tables_consume_custom_gap_preferences() {
    for gap in [0, 1, 2, 6] {
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        let cell = |text: &str| TableCell {
            break_after: false,
            blocks: vec![paragraph(text)],
            kind: mant_ir::TableCellKind::Text,
            column_span: 1,
            row_span: 1,
            alignment: None,
        };
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("LEFT_TOKEN"), cell("RIGHT_TOKEN")],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                gap_columns: gap,
                ..Default::default()
            },
            layout: LayoutHint::default(),
            source: None,
        }];
        let wire = mant_render::render_query_json(&query, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored = restored.into();
        let rendered = DocumentView::new(&restored).render(80);
        let left = rendered.search("LEFT_TOKEN");
        let right = rendered.search("RIGHT_TOKEN");
        assert_eq!(left.len(), 1);
        assert_eq!(right.len(), 1);
        assert_eq!(left[0].row, right[0].row);
        assert_eq!(
            right[0].start_column - left[0].start_column,
            10 + usize::from(gap)
        );
        let selected = rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: left[0].row,
                column: left[0].start_column,
            },
            focus: TextPosition {
                row: right[0].row,
                column: right[0].end_column,
            },
        });
        assert!(selected.contains("LEFT_TOKEN"));
        assert!(selected.contains("RIGHT_TOKEN"));
    }
}

#[test]
fn full_query_padding_fallback_preserves_all_cells_at_maximum_viewport() {
    for count in [256, 257] {
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        let words = (0..count)
            .map(|index| format!("CELL_{index:03}"))
            .collect::<Vec<_>>();
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: words
                    .iter()
                    .map(|word| TableCell {
                        break_after: false,
                        blocks: vec![paragraph(word)],
                        kind: mant_ir::TableCellKind::Text,
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    })
                    .collect(),
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![u16::MAX; count],
                advance_limit_columns: Some(256),
                extra_width_columns: Some(10),
                ..Default::default()
            },
            layout: LayoutHint::default(),
            source: None,
        }];
        let wire = mant_render::render_query_json(&query, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored = restored.into();
        let rendered = DocumentView::new(&restored).render(u16::MAX);
        let mut previous = None;
        for word in words {
            let matches = rendered.search(&word);
            assert_eq!(matches.len(), 1, "count={count}, {word}");
            let hit = &matches[0];
            assert_eq!(hit.start_column, 0, "count={count}, {word}");
            if let Some(row) = previous {
                assert_eq!(hit.row, row + 1);
            }
            previous = Some(hit.row);
        }
    }
}
