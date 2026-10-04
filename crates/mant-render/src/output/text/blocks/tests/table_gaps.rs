//! Actual-parent gap cursors remain shared across cell and table boundaries.

use super::*;
use mant_ir::{ColumnPreferences, TableCell, TableRow};

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
        column_preferences: ColumnPreferences::default(),
        layout: LayoutHint {
            indent_columns: origin,
            spacing_before_lines: gap,
            ..Default::default()
        },
        source: None,
    }
}

fn body(gap: u16) -> Block {
    let mut block = paragraph("BODY", 3);
    if let Block::Paragraph { layout, .. } = &mut block {
        layout.spacing_before_lines = gap;
    }
    block
}

fn gap(rows: u16) -> Block {
    Block::VerticalSpace {
        lines: rows,
        source: None,
    }
}

#[test]
fn leading_cell_requests_share_the_actual_parent_at_every_nested_depth() {
    let renderer = super::super::super::plain_renderer();
    for depth in [0, 1, 2, 4] {
        let mut inner = body(3000);
        for _ in 0..depth {
            inner = table(vec![cell(vec![inner], false)], 0, 0);
        }
        let outer = table(vec![cell(vec![inner], false)], -2, 3000);
        let wire = serde_json::to_string(&outer).unwrap();
        let restored: Block = serde_json::from_str(&wire).unwrap();
        let blocks = [paragraph("BEFORE", 0), restored];
        assert!(mant_ir::geometry::has_bounded_gap(&blocks));
        let output = renderer.render_blocks(&blocks, 0);
        assert_eq!(output.matches('\n').count(), 4097, "depth={depth}");
        assert_eq!(output.lines().last(), Some(" BODY"));
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
        let blocks = [paragraph("BEFORE", 0), independent];
        assert!(!mant_ir::geometry::has_bounded_gap(&blocks));
        let output = renderer.render_blocks(&blocks, 0);
        assert_eq!(output.matches('\n').count(), 6001);
        assert_eq!(output.lines().last(), Some("     BODY"));
    }
}

#[test]
fn trailing_and_leading_cell_requests_use_one_cursor_before_the_next_body() {
    let renderer = super::super::super::plain_renderer();
    for break_after in [false, true] {
        let block = table(
            vec![
                cell(vec![paragraph("FIRST", 3), gap(3000)], break_after),
                cell(vec![body(3000)], false),
            ],
            -2,
            0,
        );
        let wire = serde_json::to_string(&block).unwrap();
        let restored: Block = serde_json::from_str(&wire).unwrap();
        assert_eq!(
            mant_ir::geometry::has_bounded_gap(std::slice::from_ref(&restored)),
            !break_after
        );
        let output = renderer.render_blocks(&[restored], 0);
        assert_eq!(
            output.matches('\n').count(),
            if break_after { 6001 } else { 4097 }
        );
        assert_eq!(output.lines().next(), Some(" FIRST"));
        assert_eq!(output.lines().last(), Some(" BODY"));
    }
}

#[test]
fn gap_only_cells_complete_once_and_whole_rows_start_the_next_gap_budget() {
    let renderer = super::super::super::plain_renderer();
    for break_after in [false, true] {
        let block = table(
            vec![
                cell(vec![gap(3000)], break_after),
                cell(vec![body(0)], false),
            ],
            -2,
            3000,
        );
        let output = renderer.render_blocks(&[paragraph("BEFORE", 0), block], 0);
        assert_eq!(output.matches('\n').count(), 4097);
        assert_eq!(output.lines().last(), Some(" BODY"));
        let gap_only = table(vec![cell(vec![gap(3000)], break_after)], -2, 0);
        let blocks = [paragraph("BEFORE", 0), gap_only, gap(3000), body(0)];
        assert!(!mant_ir::geometry::has_bounded_gap(&blocks));
        let output = renderer.render_blocks(&blocks, 0);
        assert_eq!(output.matches('\n').count(), 6001);
        assert_eq!(output.lines().last(), Some("   BODY"));
    }
}

#[test]
fn column_topology_fallback_keeps_positive_cell_budgets_independent() {
    let renderer = super::super::super::plain_renderer();
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
        let blocks = [paragraph("BEFORE", 0), block];
        assert_eq!(mant_ir::geometry::has_bounded_gap(&blocks), origin < 0);
        let output = renderer.render_blocks(&blocks, 0);
        assert_eq!(
            output.matches('\n').count(),
            if origin < 0 { 4097 } else { 6001 }
        );
        assert_eq!(
            output.lines().last(),
            Some(if origin < 0 { " BODY" } else { "     BODY" })
        );
    }
}

#[test]
fn saturated_nested_gap_only_receipts_do_not_add_an_empty_data_row() {
    let renderer = super::super::super::plain_renderer();
    for depth in [1, 2, 4] {
        for break_after in [false, true] {
            let mut inner = table(vec![cell(vec![gap(3000)], break_after)], 0, 0);
            for _ in 1..depth {
                inner = table(vec![cell(vec![inner], break_after)], 0, 0);
            }
            let outer = table(vec![cell(vec![inner], break_after)], -2, 4096);
            let wire = serde_json::to_string(&outer).unwrap();
            let restored: Block = serde_json::from_str(&wire).unwrap();
            let output = renderer.render_blocks(
                &[paragraph("BEFORE", 0), restored, paragraph("AFTER", 0)],
                0,
            );
            assert_eq!(
                output.matches('\n').count(),
                4097,
                "depth={depth}, close={break_after}"
            );
            assert_eq!(output.lines().next(), Some("BEFORE"));
            assert_eq!(output.lines().last(), Some("AFTER"));
        }
    }
}
