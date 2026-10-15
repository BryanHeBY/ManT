//! Whole-table placement is independent of cell/owner row alignment.
use super::*;
use mant_ir::{TableAlignment, TableRow, TableRowKind, TableRuleCellKind};

fn cell(text: &str, alignment: Option<TableAlignment>) -> TableCell {
    TableCell {
        blocks: vec![paragraph(text, 0)],
        alignment,
        break_after: false,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
    }
}

fn data(cells: Vec<TableCell>) -> TableRow {
    TableRow {
        kind: TableRowKind::Data,
        cells,
    }
}

fn block(rows: Vec<TableRow>, alignment: Option<TableAlignment>) -> Block {
    Block::Table {
        rows,
        alignment,
        column_count: None,
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn at_width(block: &Block, width: Option<usize>, decorated: bool) -> String {
    let paint = |_: crate::presentation::TextPresentation, value: &str| {
        if decorated {
            format!("\x1b[1m{value}\x1b[0m")
        } else {
            value.to_owned()
        }
    };
    let renderer = BlockRenderer {
        width,
        names: None,
        locations: None,
        decorate: &paint,
    };
    renderer.render_blocks(std::slice::from_ref(block), 0)
}

#[test]
fn table_frame_places_every_row_and_rule_at_one_measured_origin() {
    for alignment in [
        None,
        Some(TableAlignment::Left),
        Some(TableAlignment::Center),
        Some(TableAlignment::Right),
    ] {
        for (gap, parent) in [(0, 0), (2, 0), (2, 4), (6, 3)] {
            let mut table = block(
                vec![
                    data(vec![
                        cell("A", Some(TableAlignment::Center)),
                        cell("B", Some(TableAlignment::Right)),
                    ]),
                    data(vec![cell("LONGLEFT", None), cell("LONGRIGHT", None)]),
                    TableRow {
                        kind: TableRowKind::HorizontalRule,
                        cells: vec![],
                    },
                    TableRow {
                        kind: TableRowKind::DoubleHorizontalRule,
                        cells: vec![],
                    },
                    TableRow {
                        kind: TableRowKind::LayoutRule {
                            cells: vec![
                                TableRuleCellKind::Horizontal,
                                TableRuleCellKind::DoubleHorizontal,
                            ],
                        },
                        cells: vec![],
                    },
                ],
                alignment,
            );
            if let Block::Table {
                column_preferences,
                layout,
                ..
            } = &mut table
            {
                column_preferences.gap_columns = gap;
                layout.indent_columns = parent;
            }
            let table: Block =
                serde_json::from_str(&serde_json::to_string(&table).unwrap()).unwrap();
            let extent = 8 + usize::from(gap) + 1 + 9;
            let spare = 48 - usize::try_from(parent).unwrap() - extent;
            let start = usize::try_from(parent).unwrap()
                + match alignment {
                    Some(TableAlignment::Center) => spare / 2,
                    Some(TableAlignment::Right) => spare,
                    _ => 0,
                };
            let text = at_width(&table, Some(48), false);
            let rows = text.lines().collect::<Vec<_>>();
            assert_eq!(rows.len(), 5, "{text:?}");
            assert_eq!(rows[0].find('A'), Some(start + 3));
            assert_eq!(rows[0].find('B'), Some(start + extent - 1));
            assert_eq!(rows[1].find("LONGLEFT"), Some(start));
            assert_eq!(
                rows[1].find("LONGRIGHT"),
                Some(start + 8 + usize::from(gap) + 1)
            );
            assert_eq!(
                rows[2],
                format!("{}{}", " ".repeat(start), "-".repeat(extent))
            );
            assert_eq!(
                rows[3],
                format!("{}{}", " ".repeat(start), "=".repeat(extent))
            );
            assert_eq!(
                rows[4].find("========="),
                Some(start + 8 + usize::from(gap) + 1)
            );
            assert_eq!(rows[4].len(), start + extent);
            assert_eq!(
                at_width(&table, Some(48), true)
                    .replace("\x1b[1m", "")
                    .replace("\x1b[0m", ""),
                text
            );
        }
    }
}

#[test]
fn an_explicit_frame_bounds_the_last_owner_hint_without_changing_automatic_tables() {
    for (declared, cell_alignment) in [false, true].into_iter().flat_map(|declared| {
        [
            None,
            Some(TableAlignment::Left),
            Some(TableAlignment::Center),
            Some(TableAlignment::Right),
        ]
        .map(|alignment| (declared, alignment))
    }) {
        let mut right = cell("X", cell_alignment);
        let Block::Paragraph { inline_layout, .. } = &mut right.blocks[0] else {
            unreachable!()
        };
        inline_layout.row_hints.push(mant_ir::RowLayoutHint {
            row: 0,
            alignment: mant_ir::TextAlignment::Center,
            ..Default::default()
        });
        let mut table = block(vec![data(vec![cell("AAA", None), right])], None);
        if declared
            && let Block::Table {
                column_preferences, ..
            } = &mut table
        {
            column_preferences.widths = vec![3, 3];
        }
        let automatic = at_width(&table, Some(40), false);
        assert_eq!(
            automatic.find('X'),
            Some(22),
            "declared={declared}: {automatic:?}"
        );
        if let Block::Table { alignment, .. } = &mut table {
            *alignment = Some(TableAlignment::Left);
        }
        let finite = at_width(&table, Some(40), false);
        assert_eq!(finite.find('X'), Some(6), "declared={declared}: {finite:?}");
    }
}

#[test]
fn declared_cell_alignment_and_empty_declared_columns_share_the_frame() {
    let mut table = block(
        vec![data(vec![
            cell("A", Some(TableAlignment::Center)),
            cell("B", Some(TableAlignment::Right)),
        ])],
        Some(TableAlignment::Center),
    );
    if let Block::Table {
        column_preferences,
        column_count,
        ..
    } = &mut table
    {
        column_preferences.widths = vec![5, 5];
        *column_count = Some(3);
    }
    // Three fields (5,5,1) and two two-column gaps form a 15-column frame.
    let text = at_width(&table, Some(41), false);
    assert_eq!(text.find('A'), Some(15));
    assert_eq!(text.find('B'), Some(24));
    assert_eq!(at_width(&table, None, false).find('A'), Some(2));
}

#[test]
fn narrower_and_budget_fallbacks_remain_left_and_keep_absent_cells_absent() {
    for widths in [vec![], vec![u16::MAX, 1], vec![1; 257]] {
        let mut empty = cell("", None);
        empty.blocks.clear();
        let mut table = block(
            vec![
                data(vec![
                    cell("A", Some(TableAlignment::Center)),
                    empty,
                    cell("C", None),
                ]),
                TableRow {
                    kind: TableRowKind::HorizontalRule,
                    cells: vec![],
                },
            ],
            Some(TableAlignment::Center),
        );
        if let Block::Table {
            column_preferences, ..
        } = &mut table
        {
            column_preferences.widths = widths;
        }
        assert_eq!(at_width(&table, Some(2), false), "A\nC\n---");
    }
}

#[test]
fn unused_logical_columns_do_not_repeat_partial_layout_rule_strengths() {
    let mut table = block(
        vec![
            data(vec![cell("LONG", None), cell("END", None)]),
            TableRow {
                kind: TableRowKind::LayoutRule {
                    cells: vec![
                        TableRuleCellKind::Horizontal,
                        TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                cells: vec![],
            },
        ],
        Some(TableAlignment::Left),
    );
    if let Block::Table { column_count, .. } = &mut table {
        *column_count = Some(3);
    }
    let text = at_width(&table, Some(40), false);
    let rows = text.lines().collect::<Vec<_>>();
    assert_eq!(rows[0], "LONG | END |  ");
    assert_eq!(rows[1], "---- | === |  ");
    assert_eq!(rows[1].matches('=').count(), 3);
}

#[test]
fn long_author_text_does_not_turn_rule_measurements_into_unbounded_padding() {
    let table = block(
        vec![
            data(vec![cell(&"A".repeat(5000), None)]),
            TableRow {
                kind: TableRowKind::LayoutRule {
                    cells: vec![TableRuleCellKind::Horizontal],
                },
                cells: vec![],
            },
        ],
        Some(TableAlignment::Center),
    );
    let text = at_width(&table, None, false);
    let rows = text.lines().collect::<Vec<_>>();
    assert_eq!(rows[0].len(), 5000, "author content is never clipped");
    assert_eq!(rows[1].len(), mant_ir::geometry::MAX_COLUMN_PADDING);
    assert!(rows[1].chars().all(|character| character == '-'));
}

#[test]
fn cell_rule_lengths_follow_the_same_columns_as_styled_unicode_content() {
    let table = block(
        vec![
            data(vec![cell("界界", None), cell("e\u{301}", None)]),
            data(vec![
                TableCell {
                    kind: mant_ir::TableCellKind::HorizontalRule,
                    ..cell("", None)
                },
                TableCell {
                    kind: mant_ir::TableCellKind::DoubleHorizontalRule,
                    ..cell("", None)
                },
            ]),
        ],
        Some(TableAlignment::Center),
    );
    let text = at_width(&table, Some(20), false);
    let rows = text.lines().collect::<Vec<_>>();
    // Rules establish three columns of intrinsic minimum width; the first
    // Unicode field is four display cells, regardless of UTF-8 byte length.
    assert_eq!(rows[0], "     界界 | e\u{301}");
    assert_eq!(rows[1], "     ---- | ===");
    assert_eq!(
        at_width(&table, Some(20), true)
            .replace("\x1b[1m", "")
            .replace("\x1b[0m", ""),
        text
    );
}

#[test]
fn declared_last_cell_alignment_has_a_slot_while_its_owner_hint_has_a_viewport() {
    for frame in [None, Some(TableAlignment::Left)] {
        for (owner, expected) in [(false, 10), (true, if frame.is_some() { 10 } else { 39 })] {
            let mut last = cell(
                "R",
                Some(if owner {
                    TableAlignment::Left
                } else {
                    TableAlignment::Right
                }),
            );
            if owner {
                let Block::Paragraph { inline_layout, .. } = &mut last.blocks[0] else {
                    unreachable!()
                };
                inline_layout.row_hints.push(mant_ir::RowLayoutHint {
                    row: 0,
                    alignment: mant_ir::TextAlignment::Right,
                    ..Default::default()
                });
            }
            let mut table = block(vec![data(vec![cell("L", None), last])], frame);
            if let Block::Table {
                column_preferences, ..
            } = &mut table
            {
                column_preferences.widths = vec![4, 5];
            }
            assert_eq!(at_width(&table, Some(40), false).find('R'), Some(expected));
        }
    }
}

#[test]
fn final_cell_alignment_origins_are_bounded_before_accepting_column_placement() {
    for declared in [false, true] {
        let mut short = cell("R", Some(TableAlignment::Right));
        if let Block::Paragraph { layout, .. } = &mut short.blocks[0] {
            layout.indent_columns = 3;
        }
        let mut table = block(
            vec![data(vec![short]), data(vec![cell("LONGERWORD", None)])],
            None,
        );
        if let Block::Table {
            layout,
            column_preferences,
            ..
        } = &mut table
        {
            layout.indent_columns = 4090;
            if declared {
                column_preferences.widths = vec![10];
            }
        }
        let text = at_width(&table, Some(8192), false);
        assert_eq!(text.lines().next().unwrap().find('R'), Some(4093));
        assert!(text.contains("LONGERWORD"));
    }
}

#[test]
fn one_rows_hard_cell_boundary_selects_source_order_fallback_for_the_whole_table() {
    let mut first = cell("A", None);
    first.break_after = true;
    let table = block(
        vec![
            data(vec![first, cell("B", None)]),
            data(vec![cell("C", None), cell("D", None)]),
            TableRow {
                kind: TableRowKind::HorizontalRule,
                cells: vec![],
            },
        ],
        Some(TableAlignment::Center),
    );
    assert_eq!(at_width(&table, Some(40), false), "A\nB\nC\nD\n---");
}

#[test]
fn declared_layout_rules_use_declared_slots_and_leave_unruled_tail_columns_blank() {
    let mut table = block(
        vec![
            data(vec![cell("A", None), cell("B", None)]),
            TableRow {
                kind: TableRowKind::LayoutRule {
                    cells: vec![
                        TableRuleCellKind::Horizontal,
                        TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                cells: vec![],
            },
        ],
        Some(TableAlignment::Left),
    );
    if let Block::Table {
        column_preferences,
        column_count,
        ..
    } = &mut table
    {
        column_preferences.widths = vec![4, 5];
        *column_count = Some(3);
    }
    let text = at_width(&table, Some(40), false);
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        ["A     B", "----  =====   "]
    );
}

#[test]
fn rule_extents_follow_the_stack_when_the_gap_budget_precludes_a_grid() {
    let mut table = block(
        vec![
            data(vec![cell("A", None), cell("B", None)]),
            TableRow {
                kind: TableRowKind::LayoutRule {
                    cells: vec![
                        TableRuleCellKind::Horizontal,
                        TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                cells: vec![],
            },
            TableRow {
                kind: TableRowKind::HorizontalRule,
                cells: vec![],
            },
        ],
        Some(TableAlignment::Center),
    );
    if let Block::Table {
        column_preferences, ..
    } = &mut table
    {
        column_preferences.gap_columns = u16::MAX;
    }
    assert_eq!(at_width(&table, Some(40), false), "A\nB\n---\n===\n---");
}

#[test]
fn finite_frame_alignment_proofs_use_the_cell_area_instead_of_the_parent_viewport() {
    let mut text = cell("X", Some(TableAlignment::Left));
    if let Block::Paragraph { inline_layout, .. } = &mut text.blocks[0] {
        inline_layout.row_hints.push(mant_ir::RowLayoutHint {
            row: 0,
            alignment: mant_ir::TextAlignment::Right,
            ..Default::default()
        });
    }
    let mut table = block(vec![data(vec![text])], Some(TableAlignment::Left));
    if let Block::Table { layout, .. } = &mut table {
        layout.indent_columns = 4090;
    }
    assert_eq!(
        at_width(&table, Some(4100), false),
        format!("{}X", " ".repeat(4090))
    );
    if let Block::Table {
        rows,
        column_preferences,
        ..
    } = &mut table
    {
        column_preferences.widths = vec![8];
        rows[0].cells[0].alignment = Some(TableAlignment::Right);
        if let Block::Paragraph { inline_layout, .. } = &mut rows[0].cells[0].blocks[0] {
            inline_layout.row_hints[0].alignment = mant_ir::TextAlignment::Center;
        }
    }
    // The owner contributes three cells of padding. Its already consumed
    // receipt suppresses the cell's Right preference in both proof and output.
    assert_eq!(
        at_width(&table, Some(4100), false),
        format!("{}X", " ".repeat(4093))
    );
}
