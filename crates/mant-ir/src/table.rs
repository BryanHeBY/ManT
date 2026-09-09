//! Sparse logical-column placement shared by table consumers.
use crate::{TableCell, TableRow};

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
        .map(|cells| {
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
            blocks: Vec::new(),
            column_span: columns,
            row_span: rows,
            alignment: None,
        }
    }

    #[test]
    fn spans_and_explicit_vertical_continuations_keep_logical_columns() {
        let rows = vec![
            TableRow {
                cells: vec![cell(2, 2), cell(1, 1)],
            },
            TableRow {
                cells: vec![cell(2, 1), cell(1, 1)],
            },
            TableRow {
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
            }
        }
    }

    #[test]
    fn sparse_plan_preserves_payloads_without_span_sized_allocations() {
        let rows = [
            TableRow {
                cells: vec![cell(u16::MAX, 2), cell(0, 0)],
            },
            TableRow { cells: Vec::new() },
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
        assert!(matches!(&plan[1], TableRowPlan::Sparse { cells } if cells.is_empty()));
    }

    #[test]
    fn bounded_plan_keeps_empty_rows_and_explicit_vertical_continuations() {
        let rows = [
            TableRow {
                cells: vec![cell(2, 2), cell(1, 1)],
            },
            TableRow {
                cells: vec![cell(2, 1)],
            },
            TableRow { cells: Vec::new() },
        ];
        for (row, plan) in bounded_table_rows(&rows).iter().enumerate() {
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
        assert!(bounded_table_rows(&[]).is_empty());
    }
}
