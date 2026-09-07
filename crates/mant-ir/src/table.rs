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
        let mut slots = vec![None; self.column_count];
        for positioned in row {
            slots[positioned.column] = Some(positioned.cell);
        }
        Some(slots)
    }
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
}
