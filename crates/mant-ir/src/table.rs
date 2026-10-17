//! Sparse logical-column placement shared by table consumers.
use crate::{Block, Inline, TableCell, TableCellKind, TableRow, TableRowKind, TableRuleCellKind};

/// A navigation carrier has targets but no physical row payload.
///
/// Empty text, literal rows, hard breaks, vertical space, explicit cell
/// closures and rules remain physical content. Only anchors and transparent
/// emphasis wrappers inside paragraphs in one unspanned cell qualify. Multi-cell and spanning rows
/// retain their physical topology; an ordinary empty data row does not qualify.
#[must_use]
pub fn table_row_is_navigation_only(row: &TableRow) -> bool {
    fn anchors(nodes: &[Inline]) -> Option<usize> {
        nodes.iter().try_fold(0_usize, |count, node| {
            let added = match node {
                Inline::Anchor { .. } => 1,
                Inline::Strong { children } | Inline::Emphasis { children } => anchors(children)?,
                _ => return None,
            };
            Some(count.saturating_add(added))
        })
    }
    matches!(row.kind, TableRowKind::Data)
        && row.column_borders.is_empty()
        && row.cells.len() == 1
        && row
            .cells
            .iter()
            .try_fold(0_usize, |count, cell| {
                if cell.kind != TableCellKind::Text
                    || cell.column_span != 1
                    || cell.row_span != 1
                    || cell.break_after
                {
                    return None;
                }
                let cell_anchors = cell.blocks.iter().try_fold(0_usize, |count, block| {
                    let Block::Paragraph {
                        children, layout, ..
                    } = block
                    else {
                        return None;
                    };
                    if layout.spacing_before_lines > 0 {
                        return None;
                    }
                    anchors(children).map(|added| count.saturating_add(added))
                })?;
                (cell_anchors > 0).then_some(count.saturating_add(cell_anchors))
            })
            .is_some_and(|anchors| anchors > 0)
}

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
    bounded_table_rows_with_column_count(rows, None)
}

/// Plan rows with an optional declared logical column range.
///
/// Like [`bounded_table_rows`], this never allocates dense slots above
/// [`MAX_DENSE_TABLE_COLUMNS`], including when a declared count is very large.
/// Counts smaller than the actual rows are expanded defensively; validated
/// documents reject such counts instead.
#[must_use]
pub fn bounded_table_rows_with_column_count(
    rows: &[TableRow],
    column_count: Option<u32>,
) -> Vec<TableRowPlan<'_>> {
    let grid = TableGrid::with_column_count(rows, column_count);
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

/// Logical range occupied by cells, spans and layout-only rule columns.
///
/// This scans original cells without allocating covered or missing columns.
/// Whole-row rules alone do not imply a column count.
#[must_use]
pub fn table_logical_column_count(rows: &[TableRow]) -> usize {
    rows.iter().fold(0, |maximum, row| {
        let cells = row.cells.iter().fold(0_usize, |columns, cell| {
            columns.saturating_add(usize::try_from(cell.column_span.max(1)).unwrap_or(usize::MAX))
        });
        let rules = match &row.kind {
            TableRowKind::LayoutRule { cells } => cells.len(),
            _ => 0,
        };
        maximum.max(cells).max(rules)
    })
}

/// Renderer-neutral table coordinates, with storage proportional to cells,
/// not span widths. Rows retain explicit empty vertical-continuation cells;
/// `row_span` describes the original owner and does not shift later rows.
#[derive(Debug)]
pub struct TableGrid<'a> {
    /// Positioned cells in source row order.
    pub rows: Vec<Vec<PositionedTableCell<'a>>>,
    /// Logical column range, including spans and any declared empty tail.
    pub column_count: usize,
}

pub(crate) fn checked_table_logical_columns(rows: &[TableRow]) -> Option<u32> {
    rows.iter().try_fold(0, |maximum, row| {
        let cells = row.cells.iter().try_fold(0_u32, |columns, cell| {
            columns.checked_add(cell.column_span.max(1))
        })?;
        let rules = match &row.kind {
            TableRowKind::LayoutRule { cells } => u32::try_from(cells.len()).ok()?,
            _ => 0,
        };
        Some(maximum.max(cells).max(rules))
    })
}

impl<'a> TableGrid<'a> {
    /// Place cells without allocating a dense grid. Invalid zero spans are
    /// treated as one for defensive presentation; IR validation rejects them.
    #[must_use]
    pub fn new(rows: &'a [TableRow]) -> Self {
        Self::with_column_count(rows, None)
    }

    /// Place cells within a declared logical range, retaining empty tails.
    ///
    /// Storage stays proportional to actual cells even for `u32::MAX` columns.
    /// Invalid undersized counts are expanded to retain all original cells.
    #[must_use]
    pub fn with_column_count(rows: &'a [TableRow], column_count: Option<u32>) -> Self {
        let mut column_count = column_count
            .and_then(|count| usize::try_from(count).ok())
            .unwrap_or(0);
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
                        column = column.saturating_add(
                            usize::try_from(cell.column_span.max(1)).unwrap_or(usize::MAX),
                        );
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
mod tests;
