//! Sparse logical-column placement shared by table consumers.
use crate::{TableCell, TableRow, TableRowKind, TableRuleCellKind};

/// A cell and its zero-based logical starting column.
#[derive(Clone, Copy, Debug)]
pub struct PositionedTableCell<'a> {
    /// Starting column, accounting for every preceding horizontal span.
    pub column: usize,
    /// The original cell; spanning content is never duplicated.
    pub cell: &'a TableCell,
}

/// Maximum dense width in the bounded portable table plan.
/// Wider spans retain source cells and explicit logical positions instead of
/// allocating one separator/empty slot per covered column.
pub const MAX_DENSE_TABLE_COLUMNS: usize = 256;

/// A bounded, source-borrowing row shape, without labels or rendered text.
#[derive(Debug)]
pub enum TableRowPlan<'a> {
    /// An intentionally empty data row that occupies one physical line.
    Empty,
    /// A whole-row horizontal rule authored in the data section.
    WholeRule {
        /// Whether the native row requested a double rule.
        double: bool,
    },
    /// A layout-only rule retaining one strength per logical column.
    LayoutRule {
        /// Rule strengths in logical column order.
        cells: &'a [TableRuleCellKind],
    },
    /// Complete logical slots, including covered columns and missing cells.
    Dense {
        /// Original cells occur once; covered and missing columns are `None`.
        slots: Vec<Option<&'a TableCell>>,
    },
    /// Source cells only, for a table wider than the dense-column budget.
    Sparse {
        /// Zero-based starting columns; text/Markdown adapters own any labels.
        cells: Vec<PositionedTableCell<'a>>,
    },
}

/// Plan portable row composition without amplifying large horizontal spans.
///
/// All rows use the same table-wide width. At most 256 slots per row are
/// allocated; wider tables preserve each original cell once with its logical
/// column. No strings, presentation roles, or document facts are copied.
#[must_use]
pub fn bounded_table_rows(rows: &[TableRow]) -> Vec<TableRowPlan<'_>> {
    let grid = TableGrid::new(rows);
    grid.rows
        .into_iter()
        .zip(rows)
        .map(|(cells, row)| {
            match &row.kind {
                TableRowKind::HorizontalRule => {
                    return TableRowPlan::WholeRule { double: false };
                }
                TableRowKind::DoubleHorizontalRule => {
                    return TableRowPlan::WholeRule { double: true };
                }
                TableRowKind::LayoutRule { cells } => {
                    return TableRowPlan::LayoutRule { cells };
                }
                TableRowKind::Data if row.cells.is_empty() => return TableRowPlan::Empty,
                TableRowKind::Data => {}
            }
            if grid.column_count <= MAX_DENSE_TABLE_COLUMNS {
                let slots = dense_slots(&cells, grid.column_count);
                TableRowPlan::Dense { slots }
            } else {
                TableRowPlan::Sparse { cells }
            }
        })
        .collect()
}

/// Renderer-neutral table coordinates, with storage proportional to cells,
/// not span widths. Rows retain explicit empty vertical-continuation cells;
/// `row_span` describes the original owner and does not shift later rows.
#[derive(Debug)]
pub struct TableGrid<'a> {
    /// Positioned cells in source row order.
    pub rows: Vec<Vec<PositionedTableCell<'a>>>,
    /// Maximum logical row width, including horizontal spans.
    pub column_count: usize,
}

impl<'a> TableGrid<'a> {
    /// Place cells without allocating a dense grid. Invalid zero spans are
    /// treated as one for defensive presentation; IR validation rejects them.
    #[must_use]
    pub fn new(rows: &'a [TableRow]) -> Self {
        let mut column_count = 0;
        let rows = rows
            .iter()
            .map(|row| {
                if let TableRowKind::LayoutRule { cells } = &row.kind {
                    column_count = column_count.max(cells.len());
                }
                let mut column = 0usize;
                let cells = row
                    .cells
                    .iter()
                    .map(|cell| {
                        let positioned = PositionedTableCell { column, cell };
                        column = column.saturating_add(usize::from(cell.column_span.max(1)));
                        positioned
                    })
                    .collect();
                column_count = column_count.max(column);
                cells
            })
            .collect();
        Self { rows, column_count }
    }

    /// Expand one row into logical slots only within the caller's column
    /// budget. Covered columns and missing trailing cells yield `None`.
    #[must_use]
    pub fn slots(&self, row: usize, max_columns: usize) -> Option<Vec<Option<&'a TableCell>>> {
        if self.column_count > max_columns {
            return None;
        }
        let row = self.rows.get(row)?;
        Some(dense_slots(row, self.column_count))
    }
}

fn dense_slots<'a>(row: &[PositionedTableCell<'a>], columns: usize) -> Vec<Option<&'a TableCell>> {
    let mut slots = vec![None; columns];
    for positioned in row {
        slots[positioned.column] = Some(positioned.cell);
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cell(columns: u16, rows: u16) -> TableCell {
        TableCell {
            kind: crate::TableCellKind::Text,
            blocks: Vec::new(),
            column_span: columns,
            row_span: rows,
            alignment: None,
            source: None,
        }
    }

    #[test]
    fn spans_and_explicit_vertical_continuations_keep_logical_columns() {
        let rows = vec![
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![cell(2, 2), cell(1, 1)],
            },
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![cell(2, 1), cell(1, 1)],
            },
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![cell(1, 1), cell(1, 1), cell(1, 1)],
            },
        ];
        let grid = TableGrid::new(&rows);
        assert_eq!(grid.column_count, 3);
        assert_eq!(grid.rows[0][1].column, 2);
        assert_eq!(grid.rows[1][1].column, 2);
        assert_eq!(grid.rows[2][1].column, 1);
        let slots = grid.slots(0, 3).unwrap();
        assert!(slots[0].is_some() && slots[1].is_none() && slots[2].is_some());
        assert!(grid.slots(0, 2).is_none());
    }

    #[test]
    fn extreme_and_invalid_spans_do_not_allocate_dense_storage() {
        let rows = vec![TableRow {
            kind: crate::TableRowKind::Data,
            cells: vec![cell(u16::MAX, u16::MAX), cell(0, 0)],
        }];
        let grid = TableGrid::new(&rows);
        assert_eq!(grid.column_count, 65_536);
        assert_eq!(grid.rows[0].len(), 2);
        assert!(grid.slots(0, 256).is_none());
    }

    #[test]
    fn bounded_plan_keeps_dense_slots_through_the_exact_column_limit() {
        for (span, dense) in [(255, true), (256, false)] {
            let rows = [TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![cell(span, 1), cell(1, 1)],
            }];
            let plan = bounded_table_rows(&rows);
            match &plan[0] {
                TableRowPlan::Dense { slots } => {
                    assert!(dense);
                    assert_eq!(slots.len(), MAX_DENSE_TABLE_COLUMNS);
                    assert!(slots[1..255].iter().all(Option::is_none));
                    assert!(std::ptr::eq(slots[0].unwrap(), &raw const rows[0].cells[0]));
                    assert!(std::ptr::eq(
                        slots[255].unwrap(),
                        &raw const rows[0].cells[1]
                    ));
                }
                TableRowPlan::Sparse { cells } => {
                    assert!(!dense);
                    assert_eq!(cells.len(), 2);
                    assert_eq!(cells[1].column, 256);
                }
                TableRowPlan::Empty
                | TableRowPlan::WholeRule { .. }
                | TableRowPlan::LayoutRule { .. } => {
                    panic!("nonempty data row must retain a cell plan")
                }
            }
        }
    }

    #[test]
    fn sparse_plan_preserves_payloads_without_span_sized_allocations() {
        let rows = [
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![cell(u16::MAX, 2), cell(0, 0)],
            },
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: Vec::new(),
            },
        ];
        let plan = bounded_table_rows(&rows);
        let TableRowPlan::Sparse { cells } = &plan[0] else {
            panic!("wide table must be sparse")
        };
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].column, 0);
        assert_eq!(cells[1].column, 65_535);
        assert!(std::ptr::eq(cells[0].cell, &raw const rows[0].cells[0]));
        assert!(std::ptr::eq(cells[1].cell, &raw const rows[0].cells[1]));
        assert!(matches!(&plan[1], TableRowPlan::Empty));
    }

    #[test]
    fn bounded_plan_keeps_empty_rows_and_explicit_vertical_continuations() {
        let rows = [
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![cell(2, 2), cell(1, 1)],
            },
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![cell(2, 1)],
            },
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: Vec::new(),
            },
        ];
        let plans = bounded_table_rows(&rows);
        for (row, plan) in plans[..2].iter().enumerate() {
            let TableRowPlan::Dense { slots } = plan else {
                panic!("small table must be dense")
            };
            assert_eq!(slots.len(), 3);
            for (column, slot) in slots.iter().enumerate() {
                assert_eq!(
                    slot.is_some(),
                    (row == 0 && column != 1) || (row == 1 && column == 0)
                );
            }
        }
        assert!(matches!(plans[2], TableRowPlan::Empty));
        assert!(bounded_table_rows(&[]).is_empty());
    }

    #[test]
    fn bounded_plan_retains_whole_row_rule_strength_without_cells() {
        let rows = [
            TableRow {
                kind: crate::TableRowKind::HorizontalRule,
                cells: Vec::new(),
            },
            TableRow {
                kind: crate::TableRowKind::DoubleHorizontalRule,
                cells: Vec::new(),
            },
        ];
        assert!(matches!(
            bounded_table_rows(&rows).as_slice(),
            [
                TableRowPlan::WholeRule { double: false },
                TableRowPlan::WholeRule { double: true }
            ]
        ));
    }

    #[test]
    fn table_row_kind_json_round_trip_is_closed_and_backward_compatible() {
        let data = TableRow {
            kind: crate::TableRowKind::Data,
            cells: Vec::new(),
        };
        let data_json = serde_json::to_value(&data).unwrap();
        assert!(data_json.get("kind").is_none());
        assert_eq!(
            serde_json::from_value::<TableRow>(serde_json::json!({"cells": []})).unwrap(),
            data
        );

        for (kind, expected) in [
            (
                crate::TableRowKind::HorizontalRule,
                serde_json::json!("horizontal-rule"),
            ),
            (
                crate::TableRowKind::DoubleHorizontalRule,
                serde_json::json!("double-horizontal-rule"),
            ),
            (
                crate::TableRowKind::LayoutRule {
                    cells: vec![
                        crate::TableRuleCellKind::Horizontal,
                        crate::TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                serde_json::json!({"layout-rule":{"cells":["horizontal","double-horizontal"]}}),
            ),
        ] {
            let row = TableRow {
                kind,
                cells: Vec::new(),
            };
            let encoded = serde_json::to_value(&row).unwrap();
            assert_eq!(encoded["kind"], expected);
            assert_eq!(serde_json::from_value::<TableRow>(encoded).unwrap(), row);
        }
        assert!(
            serde_json::from_value::<TableRow>(serde_json::json!({
                "kind": "future-rule",
                "cells": []
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<TableRow>(serde_json::json!({
                "kind": {"layout-rule": {
                    "cells": ["horizontal"],
                    "future": true
                }},
                "cells": []
            }))
            .is_err()
        );
    }

    #[test]
    fn table_cell_kind_json_round_trip_is_closed_and_backward_compatible() {
        let text = cell(1, 1);
        let text_json = serde_json::to_value(&text).unwrap();
        assert!(text_json.get("kind").is_none());
        assert_eq!(
            serde_json::from_value::<TableCell>(serde_json::json!({
                "blocks": [],
                "alignment": null
            }))
            .unwrap(),
            text
        );

        for kind in [
            crate::TableCellKind::HorizontalRule,
            crate::TableCellKind::DoubleHorizontalRule,
            crate::TableCellKind::IsolatedHorizontalRule,
            crate::TableCellKind::IsolatedDoubleHorizontalRule,
        ] {
            let rule = TableCell { kind, ..cell(1, 1) };
            let encoded = serde_json::to_value(&rule).unwrap();
            assert_eq!(serde_json::from_value::<TableCell>(encoded).unwrap(), rule);
        }
        assert!(
            serde_json::from_value::<TableCell>(serde_json::json!({
                "kind": "future-rule",
                "blocks": []
            }))
            .is_err()
        );
    }
}
