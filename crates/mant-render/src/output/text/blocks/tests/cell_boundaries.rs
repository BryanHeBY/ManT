//! Source-neutral closed cell boundaries, independent of column preferences.

use super::*;
use mant_ir::TableRow;

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
            widths: vec![4, 4],
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

#[test]
fn occupied_cell_boundaries_close_without_adding_a_blank_row() {
    let renderer = super::super::super::plain_renderer();
    for preferences in preferences() {
        let block = table(
            vec![
                cell(vec![paragraph("FIRST", 0)], true),
                cell(vec![paragraph("SECOND", 0)], false),
            ],
            preferences,
        );
        let output = renderer.render_blocks(&[block], 0);
        let rows = output.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), 2, "{output:?}");
        assert_eq!(rows[0].trim_start(), "FIRST");
        assert_eq!(rows[1].trim_start(), "SECOND");
    }
    let block = table(
        vec![cell(vec![paragraph("FIRST", 0)], true)],
        mant_ir::ColumnPreferences::default(),
    );
    let output = renderer.render_blocks(&[block], 0);
    assert_eq!(output, "FIRST\n");
    assert_eq!(output.lines().count(), 1);
}

#[test]
fn boundaries_complete_open_rows_and_merge_existing_completed_rows_once() {
    let renderer = super::super::super::plain_renderer();
    for preferences in preferences() {
        for first in [
            vec![literal("FIRST\n")],
            vec![
                paragraph("FIRST", 0),
                Block::VerticalSpace {
                    lines: 1,
                    source: None,
                },
            ],
        ] {
            let block = table(
                vec![cell(first, true), cell(vec![paragraph("SECOND", 0)], false)],
                preferences.clone(),
            );
            let output = renderer.render_blocks(&[block], 0);
            let rows = output.lines().collect::<Vec<_>>();
            assert_eq!(rows.len(), 3, "{output:?}");
            assert_eq!(rows[0].trim_start(), "FIRST");
            assert_eq!(rows[1], "");
            assert_eq!(rows[2].trim_start(), "SECOND");
        }
    }
}

#[test]
fn nested_cell_boundaries_preserve_completion_through_all_table_paths() {
    let renderer = super::super::super::plain_renderer();
    for preferences in preferences() {
        for (first, distance) in [
            (vec![paragraph("FIRST", 0)], 1),
            (vec![literal("FIRST\n")], 2),
            (
                vec![
                    paragraph("FIRST", 0),
                    Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    },
                ],
                2,
            ),
        ] {
            let inner = table(vec![cell(first, true)], preferences.clone());
            let outer = table(
                vec![
                    cell(vec![inner], false),
                    cell(vec![paragraph("SECOND", 0)], false),
                ],
                preferences.clone(),
            );
            let output = renderer.render_blocks(&[outer], 0);
            let rows = output.lines().collect::<Vec<_>>();
            assert_eq!(rows.len(), distance + 1, "{output:?}");
            assert_eq!(rows[0].trim_start(), "FIRST");
            assert_eq!(rows[distance].trim_start(), "SECOND");
        }
    }
}

#[test]
fn false_boundaries_keep_existing_dense_and_declared_connections() {
    let renderer = super::super::super::plain_renderer();
    let cells = vec![
        cell(vec![paragraph("A", 0)], false),
        cell(vec![paragraph("B", 0)], false),
    ];
    assert_eq!(
        renderer.render_blocks(
            &[table(cells.clone(), mant_ir::ColumnPreferences::default())],
            0
        ),
        "A | B"
    );
    let declared = mant_ir::ColumnPreferences {
        widths: vec![4, 4],
        ..Default::default()
    };
    assert_eq!(
        renderer.render_blocks(&[table(cells, declared)], 0),
        "A     B"
    );
}

#[test]
fn empty_boundaries_close_the_existing_data_row_in_all_projection_paths() {
    // A source-neutral empty data cell owns a row even after its invisible
    // graph is retired. Fresh NBRZW/\p controls ran all pristine profiles:
    // term_fill accepts the graph, term_field paints no glyph, and
    // term_flushln still closes the data row (term.c:217, 340-349, 397).
    let renderer = super::super::super::plain_renderer();
    for preferences in preferences() {
        let first = table(
            vec![
                cell(vec![], true),
                cell(vec![paragraph("SECOND", 0)], false),
            ],
            preferences.clone(),
        );
        let output = renderer.render_blocks(&[first], 0);
        let rows = output.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), 2, "{output:?}");
        assert_eq!(rows[0], "");
        assert_eq!(rows[1].trim_start(), "SECOND");
        let middle = table(
            vec![
                cell(vec![paragraph("FIRST", 0)], false),
                cell(vec![], true),
                cell(vec![paragraph("SECOND", 0)], false),
            ],
            preferences.clone(),
        );
        let output = renderer.render_blocks(&[middle], 0);
        let rows = output.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), 2, "{output:?}");
        assert!(rows[0].starts_with("FIRST"));
        assert_eq!(rows[1].trim_start(), "SECOND");
        let inner = table(vec![cell(vec![], true)], preferences.clone());
        assert_eq!(
            renderer.render_blocks(std::slice::from_ref(&inner), 0),
            "\n"
        );
        let nested = table(
            vec![
                cell(vec![inner], false),
                cell(vec![paragraph("SECOND", 0)], false),
            ],
            preferences,
        );
        let output = renderer.render_blocks(&[nested], 0);
        let rows = output.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), 2, "{output:?}");
        assert_eq!(rows[0], "");
        assert_eq!(rows[1].trim_start(), "SECOND");
    }
}

#[test]
fn closed_anchor_rows_keep_data_topology_without_hint_padding() {
    let renderer = super::super::super::plain_renderer();
    let empty = Block::Paragraph {
        children: vec![Inline::anchor("empty-data")],
        inline_layout: mant_ir::InlineLayout {
            row_hints: vec![mant_ir::RowLayoutHint {
                row: 0,
                indent_columns: 12,
            }],
        },
        layout: LayoutHint::default(),
        source: None,
    };
    let block = table(
        vec![
            cell(vec![empty], true),
            cell(vec![paragraph("SECOND", 0)], false),
        ],
        mant_ir::ColumnPreferences::default(),
    );
    assert_eq!(renderer.render_blocks(&[block], 0), "\nSECOND");
}

#[test]
fn nested_generic_fallback_retires_only_the_open_tail_at_every_depth() {
    let renderer = super::super::super::plain_renderer();
    let styled = BlockRenderer {
        names: None,
        locations: None,
        decorate: &|_, value| format!("\x1b[1m{value}\x1b[0m"),
    };
    for depth in [1, 2, 4] {
        for (value, distance) in [("FIRST\n", 1), ("FIRST\n\n", 2), ("FIRST\n \n", 2)] {
            let mut inner = table(
                vec![
                    cell(vec![paragraph("PREFIX", 0)], false),
                    cell(vec![literal(value)], false),
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
                    cell(vec![paragraph("SECOND", 0)], false),
                ],
                mant_ir::ColumnPreferences::default(),
            );
            let wire = serde_json::to_string(&outer).unwrap();
            let restored: Block = serde_json::from_str(&wire).unwrap();
            let output = renderer.render_blocks(std::slice::from_ref(&restored), 0);
            let painted = styled.render_blocks(&[restored], 0);
            assert_eq!(
                painted.replace("\x1b[1m", "").replace("\x1b[0m", ""),
                output
            );
            assert_eq!(
                painted.matches("\x1b[1m").count(),
                painted.matches("\x1b[0m").count()
            );
            let rows = output.lines().collect::<Vec<_>>();
            let first = rows.iter().position(|row| *row == "FIRST").unwrap();
            let second = rows.iter().position(|row| *row == "SECOND").unwrap();
            assert_eq!(
                second - first,
                distance,
                "depth={depth}, {value:?}, {output:?}"
            );
        }
    }
}

#[test]
fn structural_empty_data_rows_consume_incoming_gaps_across_all_table_strategies() {
    let renderer = super::super::super::plain_renderer();
    for preferences in preferences() {
        for origin in [0, -2] {
            let mut empty = table(Vec::new(), preferences.clone());
            if let Block::Table { layout, .. } = &mut empty {
                layout.indent_columns = origin;
            }
            assert_eq!(
                renderer.render_blocks(std::slice::from_ref(&empty), 0),
                "\n"
            );
            assert_eq!(
                renderer.render_blocks(
                    &[paragraph("BEFORE", 0), empty.clone(), paragraph("AFTER", 0)],
                    0
                ),
                "BEFORE\n\nAFTER"
            );
            let blocks = [
                Block::VerticalSpace {
                    lines: 3000,
                    source: None,
                },
                empty,
                Block::VerticalSpace {
                    lines: 3000,
                    source: None,
                },
                paragraph("BODY", 0),
            ];
            assert!(!mant_ir::geometry::has_bounded_gap(&blocks));
            let output = renderer.render_blocks(&blocks, 0);
            assert_eq!(output.lines().count(), 6002);
            assert_eq!(output.lines().last(), Some("BODY"));
        }
    }
}
