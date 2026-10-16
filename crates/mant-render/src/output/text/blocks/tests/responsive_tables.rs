//! Independent portable geometry assertions: spans, borders and soft targets.
use super::table_frames::{at_width, block, cell, data};
use super::{Block, Inline, LayoutHint};
use mant_ir::{TableAlignment, TableBorderStyle as Stroke, TableBorders, TableRow, TableRowKind};

fn framed(mut table: Block, stroke: Stroke) -> Block {
    if let Block::Table { borders, .. } = &mut table {
        *borders = TableBorders {
            outer: Some(stroke),
            inner_columns: Some(stroke),
            inner_rows: Some(stroke),
        };
    }
    table
}
fn strip_ansi(value: &str) -> String {
    value.replace("\x1b[1m", "").replace("\x1b[0m", "")
}

#[test]
fn spanning_owners_align_inside_the_entire_region_without_a_covered_tail_pipe() {
    // Pure IR: the span owns both four-cell columns and their three-cell gap.
    for (alignment, position) in [
        (TableAlignment::Left, 0),
        (TableAlignment::Center, 4),
        (TableAlignment::Right, 8),
    ] {
        let mut owner = cell("XYZ", Some(alignment));
        owner.column_span = 2;
        let table = block(
            vec![
                data(vec![owner]),
                data(vec![cell("AAAA", None), cell("BBBB", None)]),
            ],
            Some(TableAlignment::Left),
        );
        let text = at_width(&table, Some(40), false);
        let rows = text.lines().collect::<Vec<_>>();
        assert_eq!(rows[0], format!("{}XYZ", " ".repeat(position)));
        assert_eq!(rows[1], "AAAA | BBBB");
        assert_eq!(strip_ansi(&at_width(&table, Some(40), true)), text);
        let restored: Block =
            serde_json::from_str(&serde_json::to_string(&table).unwrap()).unwrap();
        assert_eq!(at_width(&restored, Some(40), false), text);
    }
}

#[test]
fn borders_data_and_rules_share_one_finite_frame_for_both_strokes() {
    for (stroke, expected) in [
        (
            Stroke::Single,
            [
                "┌───┬───┐",
                "│ A │ B │",
                "├───┼───┤",
                "│ C │ D │",
                "└───┴───┘",
            ],
        ),
        (
            Stroke::Double,
            [
                "╔═══╦═══╗",
                "║ A ║ B ║",
                "╠═══╬═══╣",
                "║ C ║ D ║",
                "╚═══╩═══╝",
            ],
        ),
    ] {
        let table = framed(
            block(
                vec![
                    data(vec![cell("A", None), cell("B", None)]),
                    data(vec![cell("C", None), cell("D", None)]),
                ],
                Some(TableAlignment::Center),
            ),
            stroke,
        );
        let text = at_width(&table, Some(25), false);
        assert_eq!(
            text,
            expected.map(|row| format!("        {row}")).join("\n")
        );
        assert_eq!(strip_ansi(&at_width(&table, Some(25), true)), text);
    }
}

#[test]
fn preferred_paragraph_widths_wrap_in_columns_without_changing_authored_bytes() {
    for viewport in [28, 40, 90] {
        let mut prose = cell("Alpha beta gamma delta epsilon zeta", None);
        prose.preferred_width_columns = Some(12);
        let table = framed(
            block(
                vec![data(vec![cell("KEY", None), prose])],
                Some(TableAlignment::Left),
            ),
            Stroke::Single,
        );
        let text = at_width(&table, Some(viewport), false);
        let rows = text.lines().collect::<Vec<_>>();
        assert!(rows.len() > 3, "{text:?}");
        assert!(
            rows.iter()
                .all(|row| mant_ir::geometry::text_width(row) <= viewport),
            "{text:?}"
        );
        let restored = rows[1..rows.len() - 1]
            .iter()
            .map(|row| {
                let content = row.chars().skip(8).take(12).collect::<String>();
                content.trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(restored, "Alpha beta gamma delta epsilon zeta");
        assert_eq!(strip_ansi(&at_width(&table, Some(viewport), true)), text);
    }
}

#[test]
fn physical_literal_rows_survive_column_and_narrow_stack_placement() {
    for body in ["", "A\n\nB", "A\n"] {
        let mut literal = cell("", None);
        literal.blocks = vec![Block::Preformatted {
            children: vec![Inline::Text { value: body.into() }],
            inline_layout: mant_ir::InlineLayout::default(),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        }];
        let table = framed(
            block(vec![data(vec![literal])], Some(TableAlignment::Left)),
            Stroke::Single,
        );
        for width in [2, 80] {
            let text = at_width(&table, Some(width), false);
            if body == "A\n\nB" {
                assert_eq!(text.matches('A').count(), 1);
                assert_eq!(text.matches('B').count(), 1);
                assert_eq!(text.lines().count(), if width == 2 { 3 } else { 5 });
            }
            assert_eq!(strip_ansi(&at_width(&table, Some(width), true)), text);
        }
    }
}

#[test]
fn source_rules_keep_the_final_responsive_column_extent() {
    let mut prose = cell("Alpha beta gamma delta epsilon zeta", None);
    prose.preferred_width_columns = Some(12);
    let table = framed(
        block(
            vec![
                data(vec![cell("K", None), prose]),
                TableRow {
                    column_borders: vec![],
                    kind: TableRowKind::DoubleHorizontalRule,
                    cells: vec![],
                },
            ],
            Some(TableAlignment::Left),
        ),
        Stroke::Single,
    );
    let text = at_width(&table, Some(40), false);
    let widths = text
        .lines()
        .map(mant_ir::geometry::text_width)
        .collect::<Vec<_>>();
    assert!(widths.iter().all(|width| *width == widths[0]), "{text:?}");
    assert!(text.lines().any(|row| row.contains('═')));
}

#[test]
fn declared_fields_keep_their_sequential_offsets_inside_an_outside_frame() {
    let mut table = super::declared_column_table(&[3, 3], &["A", "B"]);
    if let Block::Table {
        borders,
        rows,
        alignment,
        column_preferences,
        ..
    } = &mut table
    {
        borders.outer = Some(Stroke::Single);
        column_preferences.gap_columns = 0;
        borders.inner_columns = Some(Stroke::Double);
        rows[0].cells[0].preferred_width_columns = Some(1);
        *alignment = Some(TableAlignment::Left);
    }
    // Declared field starts remain 0 and 3; an optional outside frame adds
    // two cells uniformly. It never converts the two fields into tbl slots.
    assert_eq!(
        at_width(&table, Some(40), false),
        "┌──────┐\n│ A  B │\n└──────┘"
    );
    assert_eq!(
        strip_ansi(&at_width(&table, Some(40), true)),
        at_width(&table, Some(40), false)
    );
    if let Block::Table { rows, .. } = &mut table {
        rows[0].cells[0].blocks = vec![Block::Preformatted {
            children: vec![Inline::Text {
                value: "A\n".into(),
            }],
            inline_layout: mant_ir::InlineLayout::default(),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        }];
    }
    let text = at_width(&table, Some(40), false);
    assert_eq!(text.matches('A').count(), 1);
    assert_eq!(text.matches('B').count(), 1);
    assert_eq!(text.lines().count(), 4, "{text:?}");
    assert_eq!(strip_ansi(&at_width(&table, Some(40), true)), text);
}

#[test]
fn responsive_shrinking_keeps_all_columns_and_unicode_decorations() {
    let mut prose = cell("Alpha beta gamma delta", None);
    prose.preferred_width_columns = Some(34);
    let table = framed(
        block(
            vec![data(vec![cell("KEY", None), prose])],
            Some(TableAlignment::Left),
        ),
        Stroke::Single,
    );
    let text = at_width(&table, Some(18), false);
    assert_eq!(text.lines().next(), Some("┌─────┬──────────┐"));
    assert!(
        text.lines()
            .all(|row| mant_ir::geometry::text_width(row) == 18),
        "{text:?}"
    );
    assert_eq!(strip_ansi(&at_width(&table, Some(18), true)), text);
    let mut wide = cell("α中e\u{301} β中e\u{301} γ中e\u{301}", None);
    wide.preferred_width_columns = Some(8);
    let table = framed(
        block(vec![data(vec![wide])], Some(TableAlignment::Left)),
        Stroke::Double,
    );
    let text = at_width(&table, Some(12), false);
    assert_eq!(strip_ansi(&at_width(&table, Some(12), true)), text);
    assert!(
        text.lines()
            .all(|row| mant_ir::geometry::text_width(row) == 12)
    );
    assert_eq!(text.matches('α').count(), 1);
    assert_eq!(text.matches('β').count(), 1);
    assert_eq!(text.matches('γ').count(), 1);
}

#[test]
fn vertical_owners_suppress_grid_segments_without_consuming_rule_rows() {
    let mut owner = cell("OWNER", None);
    owner.column_span = 2;
    owner.row_span = 2;
    let mut empty = cell("", None);
    empty.blocks.clear();
    let table = framed(
        block(
            vec![
                data(vec![owner, cell("A", None)]),
                TableRow {
                    column_borders: vec![],
                    kind: TableRowKind::HorizontalRule,
                    cells: vec![],
                },
                data(vec![empty.clone(), empty, cell("B", None)]),
            ],
            Some(TableAlignment::Left),
        ),
        Stroke::Single,
    );
    let text = at_width(&table, Some(40), false);
    let rows = text.lines().collect::<Vec<_>>();
    let owner = rows.iter().position(|row| row.contains("OWNER")).unwrap();
    assert_eq!(rows[owner].matches('│').count(), 3);
    assert_eq!(rows[owner + 1].matches('─').count(), 3, "{text}");
    assert_eq!(rows[owner + 1].matches('│').count(), 1, "{text}");
    assert!(rows[owner + 2].contains('B'));
    assert_eq!(rows[owner + 2].matches('│').count(), 3);
    assert_eq!(text.matches("OWNER").count(), 1);
}

#[test]
fn distinct_grid_and_outer_strokes_keep_both_authored_strengths() {
    let mut table = framed(
        block(
            vec![
                data(vec![cell("A", None), cell("B", None)]),
                data(vec![cell("C", None), cell("D", None)]),
            ],
            Some(TableAlignment::Left),
        ),
        Stroke::Single,
    );
    if let Block::Table { borders, .. } = &mut table {
        borders.outer = Some(Stroke::Double);
    }
    assert_eq!(
        at_width(&table, Some(40), false),
        [
            "╔═══╤═══╗",
            "║ A │ B ║",
            "╟───┼───╢",
            "║ C │ D ║",
            "╚═══╧═══╝"
        ]
        .join("\n")
    );
}

#[test]
fn short_preferred_columns_position_the_frame_without_generating_body_padding() {
    for (alignment, origin) in [
        (TableAlignment::Left, 0),
        (TableAlignment::Center, 23),
        (TableAlignment::Right, 46),
    ] {
        let mut content = cell("A", None);
        content.preferred_width_columns = Some(34);
        let table = block(vec![data(vec![content])], Some(alignment));
        assert_eq!(
            at_width(&table, Some(80), false),
            format!("{}A", " ".repeat(origin))
        );
    }
    let source = "UNBREAKABLE_WORD_TOKEN";
    let mut content = cell(source, None);
    content.preferred_width_columns = Some(4);
    let table = framed(
        block(vec![data(vec![content])], Some(TableAlignment::Center)),
        Stroke::Single,
    );
    let text = at_width(&table, Some(80), false);
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(text.contains(source));
    assert_eq!(strip_ansi(&at_width(&table, Some(80), true)), text);
}

#[test]
fn a_fully_covered_derived_grid_does_not_create_an_extra_physical_row() {
    let mut owner = cell("OWNER", None);
    owner.column_span = 2;
    owner.row_span = 2;
    let mut empty = cell("", None);
    empty.blocks.clear();
    let table = framed(
        block(
            vec![data(vec![owner]), data(vec![empty.clone(), empty])],
            Some(TableAlignment::Left),
        ),
        Stroke::Single,
    );
    let text = at_width(&table, Some(40), false);
    assert_eq!(text.lines().count(), 4, "{text}");
    assert_eq!(text.lines().nth(1).unwrap().matches('│').count(), 2);
    assert_eq!(text.lines().nth(2).unwrap().matches('│').count(), 2);
    assert!(!text.lines().last().unwrap().contains('┴'));
}

#[test]
fn empty_and_navigation_only_tables_do_not_manufacture_frame_rows() {
    for rows in [
        vec![],
        vec![data(vec![mant_ir::TableCell {
            blocks: vec![Block::Paragraph {
                children: vec![Inline::anchor("spot")],
                inline_layout: mant_ir::InlineLayout::default(),
                layout: LayoutHint::default(),
                source: None,
            }],
            ..cell("", None)
        }])],
    ] {
        for declared in [false, true] {
            let mut table = framed(
                block(rows.clone(), Some(TableAlignment::Center)),
                Stroke::Double,
            );
            if declared
                && let Block::Table {
                    column_preferences, ..
                } = &mut table
            {
                column_preferences.widths = vec![4];
            }
            assert_eq!(at_width(&table, Some(40), false), "");
        }
    }
}

#[test]
fn origin_preserving_fallback_keeps_an_outer_frame_without_double_indentation() {
    for origin in [0, 4] {
        let mut first = cell("A", None);
        if let Block::Paragraph { layout, .. } = &mut first.blocks[0] {
            layout.indent_columns = -1;
        }
        let mut table = framed(
            block(vec![data(vec![first, cell("B", None)])], None),
            Stroke::Single,
        );
        if let Block::Table { layout, .. } = &mut table {
            layout.indent_columns = origin;
        }
        let text = at_width(&table, Some(80), false);
        let rows = text.lines().collect::<Vec<_>>();
        let frame = usize::try_from((origin - 1).max(0)).unwrap();
        assert_eq!(rows.len(), 4, "{text:?}");
        assert_eq!(rows[0].find('┌'), Some(frame));
        assert_eq!(
            mant_ir::geometry::text_width(&rows[1][..rows[1].find('A').unwrap()]),
            frame + 2
        );
        assert_eq!(
            mant_ir::geometry::text_width(&rows[2][..rows[2].find('B').unwrap()]),
            usize::try_from(origin).unwrap() + 2
        );
        assert_eq!(strip_ansi(&at_width(&table, Some(80), true)), text);
    }
}

#[test]
fn stacked_layout_rules_do_not_expand_each_source_cell_to_a_complete_frame() {
    for count in [32, 64] {
        let mut first = cell(&"K".repeat(1024), None);
        first.break_after = true;
        let table = framed(
            block(
                vec![
                    data(vec![first, cell("B", None)]),
                    TableRow {
                        column_borders: vec![],
                        kind: TableRowKind::LayoutRule {
                            cells: (0..count)
                                .map(|index| {
                                    if index % 2 == 0 {
                                        mant_ir::TableRuleCellKind::Horizontal
                                    } else {
                                        mant_ir::TableRuleCellKind::DoubleHorizontal
                                    }
                                })
                                .collect(),
                        },
                        cells: vec![],
                    },
                ],
                Some(TableAlignment::Left),
            ),
            Stroke::Single,
        );
        let text = at_width(&table, Some(2048), false);
        assert_eq!(text.lines().count(), 5);
        assert_eq!(text.matches('K').count(), 1024);
        assert_eq!(
            text.matches('═').count(),
            513,
            "includes the final double-rule side padding"
        );
        assert!(text.chars().count() < 5200);
    }
}

#[test]
fn navigation_prefixes_cannot_change_a_physical_rows_edge_strokes() {
    let mut physical = data(vec![cell("A", None), cell("B", None)]);
    physical.column_borders = vec![mant_ir::TableColumnBorder {
        column: 1,
        style: Stroke::Double,
    }];
    let mut table = block(vec![physical], Some(TableAlignment::Left));
    if let Block::Table { borders, .. } = &mut table {
        borders.outer = Some(Stroke::Single);
    }
    let expected = at_width(&table, Some(40), false);
    assert_eq!(expected.lines().next(), Some("┌───╥───┐"));
    if let Block::Table { rows, .. } = &mut table {
        let mut carrier = cell("", None);
        if let Block::Paragraph { children, .. } = &mut carrier.blocks[0] {
            *children = vec![Inline::anchor("spot")];
        }
        rows.insert(0, data(vec![carrier]));
    }
    assert_eq!(at_width(&table, Some(40), false), expected);
}

#[test]
fn sparse_fallback_keeps_each_rows_local_outside_edge_strengths() {
    let mut edges = data(vec![cell("A", None)]);
    edges.column_borders = vec![
        mant_ir::TableColumnBorder {
            column: 0,
            style: Stroke::Single,
        },
        mant_ir::TableColumnBorder {
            column: 257,
            style: Stroke::Double,
        },
    ];
    let mut table = block(
        vec![edges, data(vec![cell("B", None)])],
        Some(TableAlignment::Left),
    );
    if let Block::Table { column_count, .. } = &mut table {
        *column_count = Some(257);
    }
    let text = at_width(&table, Some(40), false);
    let rows = text.lines().collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "local edges do not invent top/bottom rules");
    assert!(
        rows[0].starts_with("│ ") && rows[0].ends_with(" ║"),
        "{text:?}"
    );
    assert!(!rows[1].contains('│') && !rows[1].contains('║'), "{text:?}");
    assert_eq!(text.matches('A').count(), 1);
    assert_eq!(text.matches('B').count(), 1);
}
