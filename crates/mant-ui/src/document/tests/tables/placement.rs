//! Whole-table placement is independent from cell and owner-row alignment.
use super::super::*;
use mant_ir::{ColumnPreferences, TableAlignment, TableRowKind, TableRuleCellKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

fn cell(value: &str, alignment: Option<TableAlignment>) -> TableCell {
    TableCell {
        preferred_width_columns: None,
        blocks: vec![Block::Paragraph {
            children: vec![
                Inline::anchor(format!("at-{value}")),
                Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://example.test".into(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: value.into(),
                    }],
                },
            ],
            inline_layout: mant_ir::InlineLayout::default(),
            layout: LayoutHint::default(),
            source: None,
        }],
        kind: mant_ir::TableCellKind::Text,
        break_after: false,
        column_span: 1,
        row_span: 1,
        alignment,
    }
}

fn table(alignment: Option<TableAlignment>, count: Option<u32>) -> Block {
    Block::Table {
        borders: mant_ir::TableBorders::default(),
        alignment,
        column_count: count,
        column_preferences: ColumnPreferences::default(),
        rows: vec![
            TableRow {
                column_borders: vec![],
                kind: TableRowKind::HorizontalRule,
                cells: vec![],
            },
            TableRow {
                column_borders: vec![],
                kind: TableRowKind::Data,
                cells: vec![
                    cell("Q", Some(TableAlignment::Center)),
                    cell("R", Some(TableAlignment::Right)),
                ],
            },
            TableRow {
                column_borders: vec![],
                kind: TableRowKind::LayoutRule {
                    cells: vec![
                        TableRuleCellKind::Horizontal,
                        TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                cells: vec![],
            },
            TableRow {
                column_borders: vec![],
                kind: TableRowKind::Data,
                cells: vec![cell("LLLL", None), cell("MMMMM", None)],
            },
            TableRow {
                column_borders: vec![],
                kind: TableRowKind::DoubleHorizontalRule,
                cells: vec![],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn query(block: Block) -> ResolvedContent {
    let mut value = bundle();
    let document = value.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![block];
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&value)).unwrap();
    serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into()
}

#[test]
fn table_frame_and_all_rule_endpoints_move_together_after_json() {
    for width in [24_u16, 60] {
        for (alignment, offset) in [
            (None, 0),
            (Some(TableAlignment::Left), 0),
            (Some(TableAlignment::Center), (usize::from(width) - 11) / 2),
            (Some(TableAlignment::Right), usize::from(width) - 11),
        ] {
            let query = query(table(alignment, None));
            let rendered = DocumentView::new(&query).render(width);
            let q = &rendered.search("Q")[0];
            let r = &rendered.search("R")[0];
            assert_eq!((q.start_column, r.start_column), (offset + 1, offset + 10));
            let rows = &rendered.text.lines;
            assert_eq!(
                rows[q.row - 1].to_string(),
                format!("{}{}", " ".repeat(offset), "─".repeat(11))
            );
            assert_eq!(
                rows[q.row + 1].to_string(),
                format!("{}────  ═════", " ".repeat(offset))
            );
            assert_eq!(
                rows[q.row + 3].to_string(),
                format!("{}{}", " ".repeat(offset), "═".repeat(11))
            );
            let area = Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
            let mut buffer = Buffer::empty(area);
            Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
            assert_eq!(
                buffer[(
                    u16::try_from(q.start_column).unwrap(),
                    u16::try_from(q.row).unwrap()
                )]
                    .symbol(),
                "Q"
            );
            assert!(rendered.link_target_at(q.row, q.start_column).is_some());
            assert_eq!(rendered.anchor_row("at-Q"), Some(q.row));
            assert_eq!(
                rendered
                    .selected_text(RenderedSelection {
                        anchor: TextPosition {
                            row: q.row,
                            column: 0
                        },
                        focus: TextPosition {
                            row: q.row,
                            column: q.end_column - 1
                        },
                    })
                    .trim(),
                "Q"
            );
        }
    }
}

#[test]
fn explicit_frames_bound_owner_alignment_but_automatic_tables_keep_viewport_slots() {
    for alignment in [
        None,
        Some(TableAlignment::Left),
        Some(TableAlignment::Center),
        Some(TableAlignment::Right),
    ] {
        for declared in [false, true] {
            let mut block = table(alignment, None);
            let Block::Table {
                rows,
                column_preferences,
                ..
            } = &mut block
            else {
                unreachable!()
            };
            if declared {
                column_preferences.widths = vec![4, 5];
            }
            let Block::Paragraph { inline_layout, .. } = &mut rows[1].cells[1].blocks[0] else {
                unreachable!()
            };
            inline_layout.row_hints.push(mant_ir::RowLayoutHint {
                alignment: mant_ir::TextAlignment::Right,
                ..Default::default()
            });
            let rendered = DocumentView::new(&query(block)).render(40);
            let r = &rendered.search("R")[0];
            let expected = match alignment {
                None | Some(TableAlignment::Right) => 39,
                Some(TableAlignment::Left) => 10,
                Some(TableAlignment::Center) => 24,
            };
            assert_eq!(
                r.start_column, expected,
                "{alignment:?}, declared={declared}"
            );
        }
    }
}

#[test]
fn unoccupied_declared_columns_extend_the_frame_without_creating_rows_or_links() {
    for count in [2_u32, 3, 5] {
        let value = query(table(Some(TableAlignment::Center), Some(count)));
        let rendered = DocumentView::new(&value).render(60);
        let extent = 11 + (usize::try_from(count).unwrap() - 2) * 3;
        let offset = (60 - extent) / 2;
        let q = &rendered.search("Q")[0];
        assert_eq!(q.start_column, offset + 1);
        assert_eq!(
            rendered.text.lines[q.row - 1].to_string(),
            format!("{}{}", " ".repeat(offset), "─".repeat(extent))
        );
        assert_eq!(rendered.links.len(), 4);
    }
    for count in [Some(3), Some(u32::MAX)] {
        let value = query(Block::Table {
            borders: mant_ir::TableBorders::default(),
            rows: vec![TableRow {
                column_borders: vec![],
                kind: TableRowKind::HorizontalRule,
                cells: vec![],
            }],
            alignment: Some(TableAlignment::Center),
            column_count: count,
            column_preferences: ColumnPreferences::default(),
            layout: LayoutHint::default(),
            source: None,
        });
        let rendered = DocumentView::new(&value).render(20);
        let rule = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .find(|row| row.contains('─'))
            .unwrap();
        assert_eq!(
            rule,
            if count == Some(3) {
                "      ───────"
            } else {
                "─"
            }
        );
    }
}

#[test]
fn all_rule_kinds_share_the_data_fallback_after_origin_and_budget_checks() {
    for kind in 0..4 {
        let mut block = table(Some(TableAlignment::Center), None);
        let Block::Table {
            rows,
            layout,
            column_preferences,
            column_count,
            ..
        } = &mut block
        else {
            unreachable!()
        };
        match kind {
            0 => {
                layout.indent_columns = 4;
                for row in rows.iter_mut() {
                    for cell in &mut row.cells {
                        let Block::Paragraph { layout, .. } = &mut cell.blocks[0] else {
                            unreachable!()
                        };
                        layout.indent_columns = -6;
                    }
                }
            }
            1 => *column_count = Some(257),
            2 => column_preferences.gap_columns = u16::MAX,
            _ => column_preferences.widths = vec![4096, 4],
        }
        let value = query(block);
        let rendered = DocumentView::new(&value).render(40);
        let q = &rendered.search("Q")[0];
        let r = &rendered.search("R")[0];
        assert_eq!(r.row, q.row + 1, "kind={kind}");
        assert_eq!(q.start_column, 0, "kind={kind}");
        for row in &rendered.text.lines {
            let row = row.to_string();
            if row.contains('─') || row.contains('═') {
                assert_eq!(row.chars().count(), 5, "kind={kind}, {row:?}");
                assert!(!row.starts_with(' '));
            }
        }
    }
}

#[test]
fn mixed_layout_rules_preserve_strengths_and_leave_undeclared_rule_cells_empty() {
    let rendered = DocumentView::new(&query(table(Some(TableAlignment::Left), Some(3)))).render(40);
    let q = rendered.search("Q")[0].row;
    assert_eq!(rendered.text.lines[q + 1].to_string(), "────  ═════   ");
    assert_eq!(rendered.text.lines[q - 1].to_string(), "──────────────");
    let mut block = table(Some(TableAlignment::Center), Some(257));
    let Block::Table {
        column_preferences, ..
    } = &mut block
    else {
        unreachable!()
    };
    column_preferences.gap_columns = u16::MAX;
    let rendered = DocumentView::new(&query(block)).render(40);
    let q = rendered.search("Q")[0].row;
    assert_eq!(rendered.text.lines[q + 2].to_string(), "───══");
}

#[test]
fn viewport_translation_is_bounded_and_does_not_move_document_rules() {
    for alignment in [TableAlignment::Center, TableAlignment::Right] {
        let mut value = query(table(Some(alignment), None));
        value
            .document
            .as_mut()
            .unwrap()
            .blocks
            .push(Block::ThematicBreak { source: None });
        let view = DocumentView::new(&value);
        let wide = view.render(u16::MAX);
        assert_eq!(wide.search("Q")[0].start_column, 1);
        let regular = view.render(40);
        assert!(
            regular
                .text
                .lines
                .iter()
                .any(|row| row.to_string() == "─".repeat(40))
        );
        // Repeated resize retains the original preference and owner coordinates.
        assert_eq!(
            view.render(40).search("Q")[0].start_column,
            match alignment {
                TableAlignment::Center => 15,
                TableAlignment::Right => 30,
                TableAlignment::Left => unreachable!(),
            }
        );
    }
}

#[test]
fn portable_markdown_readback_keeps_content_and_hard_rows_without_table_geometry() {
    let mut expected = None;
    for alignment in [
        None,
        Some(TableAlignment::Left),
        Some(TableAlignment::Center),
        Some(TableAlignment::Right),
    ] {
        for count in [None, Some(3), Some(257)] {
            let original = query(table(alignment, count));
            let markdown = mant_codec::encode::render_markdown(&original);
            let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let document = readback.document.as_ref().unwrap();
            let literal = document
                .blocks
                .iter()
                .find_map(|block| match block {
                    Block::Preformatted { children, .. } => {
                        Some(mant_ir::inline_plain_text(children))
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(literal.lines().count(), 5, "{markdown}");
            for word in ["Q", "R", "LLLL", "MMMMM"] {
                assert_eq!(literal.matches(word).count(), 1);
            }
            if let Some(expected) = &expected {
                assert_eq!(&literal, expected);
            } else {
                expected = Some(literal);
            }
            // JSON retains the source-neutral facts even though portable
            // Markdown deliberately uses its existing fenced projection.
            assert_eq!(original.document.as_ref().unwrap().blocks.len(), 1);
        }
    }
}

#[test]
fn declared_cell_alignment_does_not_use_the_final_owner_viewport_slot() {
    for alignment in [
        None,
        Some(TableAlignment::Left),
        Some(TableAlignment::Center),
        Some(TableAlignment::Right),
    ] {
        let mut block = table(alignment, None);
        let Block::Table {
            column_preferences, ..
        } = &mut block
        else {
            unreachable!()
        };
        column_preferences.widths = vec![4, 5];
        let rendered = DocumentView::new(&query(block)).render(40);
        let origin = match alignment {
            None | Some(TableAlignment::Left) => 0,
            Some(TableAlignment::Center) => 14,
            Some(TableAlignment::Right) => 29,
        };
        assert_eq!(rendered.search("Q")[0].start_column, origin + 1);
        assert_eq!(rendered.search("R")[0].start_column, origin + 10);
    }
}

#[test]
fn final_cell_alignment_respects_composed_origin_bounds_in_both_column_plans() {
    for declared in [false, true] {
        let mut short = cell("A", Some(TableAlignment::Right));
        let Block::Paragraph { layout, .. } = &mut short.blocks[0] else {
            unreachable!()
        };
        layout.indent_columns = 3;
        let long = cell("BBBBBBBBBB", Some(TableAlignment::Right));
        let value = query(Block::Table {
            borders: mant_ir::TableBorders::default(),
            alignment: Some(TableAlignment::Left),
            column_count: Some(1),
            column_preferences: ColumnPreferences {
                widths: if declared { vec![10] } else { vec![] },
                ..Default::default()
            },
            rows: vec![
                TableRow {
                    column_borders: vec![],
                    kind: TableRowKind::Data,
                    cells: vec![short],
                },
                TableRow {
                    column_borders: vec![],
                    kind: TableRowKind::Data,
                    cells: vec![long],
                },
            ],
            layout: LayoutHint {
                indent_columns: 4090,
                ..Default::default()
            },
            source: None,
        });
        let rendered = DocumentView::new(&value).render(5000);
        assert_eq!(
            rendered.search("A")[0].start_column,
            4093,
            "declared={declared}"
        );
        assert_eq!(rendered.search("BBBBBBBBBB")[0].start_column, 4090);
        assert_eq!(rendered.links.len(), 2);
    }
}

#[test]
fn table_region_composes_with_list_definition_and_nested_table_parents() {
    for (container, expected_q) in [(0, 15), (1, 18), (2, 17), (3, 15)] {
        let inner = table(Some(TableAlignment::Center), None);
        let block = match container {
            0 => inner,
            1 => Block::List {
                kind: mant_ir::ListKind::Bullet,
                compact: true,
                items: vec![mant_ir::ListItem {
                    blocks: vec![inner],
                    entry: None,
                    source: None,
                    layout: mant_ir::ListItemLayout::default(),
                }],
                layout: LayoutHint {
                    indent_columns: 3,
                    ..Default::default()
                },
                source: None,
            },
            2 => Block::DefinitionList {
                items: vec![mant_ir::DefinitionItem {
                    terms: vec![
                        vec![Inline::Text {
                            value: "Outside".into(),
                        }]
                        .into(),
                    ],
                    description: vec![inner],
                    head_body_relation: mant_ir::HeadBodyRelation::Separate,
                    layout: mant_ir::DefinitionLayout {
                        body_indent_columns: 4,
                        ..Default::default()
                    },
                    entry: None,
                    source: None,
                }],
                declaration_groups: vec![],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            },
            _ => Block::Table {
                borders: mant_ir::TableBorders::default(),
                alignment: None,
                column_count: None,
                rows: vec![TableRow {
                    column_borders: vec![],
                    kind: TableRowKind::Data,
                    cells: vec![TableCell {
                        preferred_width_columns: None,
                        blocks: vec![inner],
                        kind: mant_ir::TableCellKind::Text,
                        break_after: false,
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    }],
                }],
                column_preferences: ColumnPreferences::default(),
                layout: LayoutHint::default(),
                source: None,
            },
        };
        let value = query(block);
        let view = DocumentView::new(&value);
        let rendered = view.render(40);
        let q = &rendered.search("Q")[0];
        assert_eq!(q.start_column, expected_q, "container={container}");
        assert_eq!(rendered.anchor_row("at-Q"), Some(q.row));
        assert!(rendered.link_target_at(q.row, q.start_column).is_some());
        assert_eq!(rendered.search("MMMMM")[0].row, q.row + 2);
    }
}

#[test]
fn translating_the_whole_frame_cannot_bypass_internal_origin_bounds() {
    let mut entry = cell("A", None);
    let Block::Paragraph { layout, .. } = &mut entry.blocks[0] else {
        unreachable!()
    };
    layout.indent_columns = 3;
    let value = query(Block::Table {
        borders: mant_ir::TableBorders::default(),
        alignment: Some(TableAlignment::Center),
        column_count: Some(1),
        rows: vec![TableRow {
            column_borders: vec![],
            kind: TableRowKind::Data,
            cells: vec![entry],
        }],
        column_preferences: ColumnPreferences::default(),
        layout: LayoutHint {
            indent_columns: 4090,
            ..Default::default()
        },
        source: None,
    });
    let rendered = DocumentView::new(&value).render(4106);
    // The existing narrow-cell readability policy reduces the cell-local
    // three-cell origin to two in its four-cell slot. Centering adds none.
    assert_eq!(rendered.search("A")[0].start_column, 4092);
}

#[test]
fn absent_rows_in_a_wide_neighbor_cell_count_toward_the_padding_budget() {
    let mut first = cell("A", None);
    first.blocks = vec![Block::Preformatted {
        children: vec![Inline::Text {
            value: std::iter::repeat_n("A", 17).collect::<Vec<_>>().join("\n"),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }];
    let value = query(Block::Table {
        borders: mant_ir::TableBorders::default(),
        alignment: None,
        column_count: None,
        rows: vec![TableRow {
            column_borders: vec![],
            kind: TableRowKind::Data,
            cells: vec![first, cell(&"B".repeat(5000), None)],
        }],
        column_preferences: ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    });
    let rendered = DocumentView::new(&value).render(10000);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(rows.iter().filter(|row| *row == "A").count(), 17);
    assert_eq!(
        rows.iter()
            .map(|row| row.matches('B').count())
            .sum::<usize>(),
        5000
    );
    assert!(rows.iter().map(String::len).sum::<usize>() < 6000);
}
