//! Bounded plain table projection for text output.
use mant_ir::{
    TableAlignment, TableCell, TableRow, TableRowPlan, TableRuleCellKind,
    bounded_table_rows_with_column_count,
};

mod responsive;
mod rows;
#[cfg(test)]
mod tests;
use rows::{
    dense_mode, dense_physical_rows, render_table_cell, sparse_fits, sparse_physical_rows,
    stacked_physical_rows,
};

const MAX_DENSE_PHYSICAL_SLOTS: usize = 16_384;

pub(super) trait TableText: Clone {
    fn longest_word_width(&self) -> usize {
        self.width()
    }
    fn wrap_within(&self, _width: usize) -> Option<Vec<Self>> {
        None
    }

    fn has_alignment(&self) -> bool {
        false
    }
    fn alignment_offset(&self, _width: Option<usize>) -> usize {
        0
    }
    fn align_within(self, _width: Option<usize>) -> Self {
        self
    }
    fn plain(value: &str) -> Self;
    fn is_empty(&self) -> bool;
    fn width(&self) -> usize;
    fn physical_lines(&self) -> Vec<Self>;
    fn ends_with_break(&self) -> bool;
    fn join(values: &[Self], separator: &str) -> Self;
    fn prefixed(self, prefix: &str) -> Self;
    fn append(&mut self, other: &Self);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PhysicalRows {
    Absent,
    Present,
}

struct CellText<T> {
    rows: Vec<T>,
    widths: Vec<usize>,
    empty: bool,
    physical: PhysicalRows,
    alignment: Option<TableAlignment>,
    rule: Option<char>,
    origins: mant_ir::geometry::CellOriginBounds,
    completed: bool,
    break_after: bool,
}

impl<T: TableText> CellText<T> {
    fn line_count(&self) -> usize {
        self.rows.len()
    }
}

enum PreparedRow<T> {
    Complete(Vec<T>, bool),
    Rule(Vec<TableRuleCellKind>, bool),
    Dense(Vec<Option<CellText<T>>>),
    Sparse(Vec<(usize, CellText<T>)>),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TableMode {
    Dense,
    Sparse,
    Stack,
}

/// A rendered data row or a rule awaiting the final table extent. Rules do
/// not establish a second viewport or choose a separate fallback policy.
pub(super) enum ProjectedTableRow<T> {
    Content {
        lines: Vec<T>,
        completed: bool,
        stacked: bool,
    },
    Rule(Vec<TableRuleCellKind>, Option<Vec<usize>>),
}

/// A cell's original physical receipt, independently of its visible text.
pub(super) struct RenderedCell<T> {
    pub(super) text: T,
    pub(super) completed: bool,
    pub(super) has_physical_rows: bool,
}

/// Whole-table geometry is metadata, never a synthetic content row.
pub(super) struct ProjectedTable<T> {
    pub(super) frame_width: usize,
    pub(super) rows: Vec<ProjectedTableRow<T>>,
}

impl<T> ProjectedTable<T> {
    fn from_rows(rows: Vec<ProjectedTableRow<T>>) -> Self {
        Self {
            frame_width: 0,
            rows,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct TableOptions {
    pub(super) gap: u16,
    pub(super) origin: i32,
    pub(super) viewport: Option<usize>,
    pub(super) bounded_frame: bool,
    pub(super) column_count: Option<u32>,
}

/// Prepare every source cell once, then place only its physical rows. Actual
/// parent/field/child origins and generated blanks share the generic bounds;
/// a failed projection reuses prepared cells in source order.
pub(super) fn projected_table_rows<T: TableText>(
    rows: &[TableRow],
    projection: TableOptions,
    borders: &mant_ir::TableBorders,
    mut render_cell: impl FnMut(&TableCell) -> RenderedCell<T>,
) -> ProjectedTable<T> {
    if responsive_geometry(rows, projection, borders) {
        return responsive::project(rows, projection, borders, &mut render_cell);
    }
    let gap = usize::from(projection.gap);
    let prepared = prepare_table_rows(rows, projection.column_count, &mut render_cell);
    let viewport = projection
        .viewport
        .map(|width| width.saturating_sub(mant_ir::geometry::padding(projection.origin)));
    let rule_slots = measured_alignment_slots(&prepared, viewport, true);
    let slots = measured_alignment_slots(
        &prepared,
        viewport,
        projection.bounded_frame || projection.column_count.is_some(),
    );
    let intrinsic = slots.as_ref().map(|slots| {
        slots
            .iter()
            .sum::<usize>()
            .saturating_add((gap + 1).saturating_mul(slots.len().saturating_sub(1)))
    });
    let local_width = if projection.bounded_frame {
        intrinsic
    } else {
        viewport
    };
    let initial = if projection.bounded_frame
        && intrinsic
            .zip(viewport)
            .is_some_and(|(used, available)| used > available)
    {
        TableMode::Stack
    } else {
        TableMode::Dense
    };
    let mode = prepared.iter().fold(initial, |mode, row| {
        mode.max(match row {
            PreparedRow::Dense(cells) => {
                dense_mode(cells, gap, projection.origin, local_width, slots.as_deref())
            }
            PreparedRow::Sparse(cells) => {
                if sparse_fits(cells, projection.origin) {
                    TableMode::Sparse
                } else {
                    TableMode::Stack
                }
            }
            _ => TableMode::Dense,
        })
    });
    let stack_width = viewport.map(|width| {
        width.min(
            mant_ir::geometry::MAX_COLUMN_PADDING
                .saturating_sub(mant_ir::geometry::padding(projection.origin)),
        )
    });
    let rows = prepared
        .into_iter()
        .map(|row| {
            let (lines, closed, stacked) = match row {
                PreparedRow::Complete(lines, closed) => (lines, closed, mode != TableMode::Dense),
                PreparedRow::Rule(cells, layout) => {
                    let widths = (layout && mode == TableMode::Dense)
                        .then(|| rule_slots.clone())
                        .flatten();
                    return ProjectedTableRow::Rule(cells, widths);
                }
                PreparedRow::Dense(cells) if mode == TableMode::Dense => {
                    dense_physical_rows(&cells, gap, local_width, slots.as_deref())
                }
                PreparedRow::Dense(cells) if mode == TableMode::Sparse => sparse_physical_rows(
                    cells
                        .into_iter()
                        .enumerate()
                        .filter_map(|(column, cell)| cell.map(|cell| (column, cell)))
                        .collect(),
                    projection.origin,
                    stack_width,
                ),
                PreparedRow::Dense(cells) => {
                    stacked_physical_rows(cells.into_iter().flatten().collect(), stack_width)
                }
                PreparedRow::Sparse(cells) if mode == TableMode::Stack => stacked_physical_rows(
                    cells.into_iter().map(|(_, cell)| cell).collect(),
                    stack_width,
                ),
                PreparedRow::Sparse(cells) => {
                    sparse_physical_rows(cells, projection.origin, stack_width)
                }
            };
            ProjectedTableRow::Content {
                lines,
                completed: closed,
                stacked,
            }
        })
        .collect();
    ProjectedTable::from_rows(rows)
}

fn prepare_table_rows<T: TableText>(
    rows: &[TableRow],
    column_count: Option<u32>,
    render_cell: &mut impl FnMut(&TableCell) -> RenderedCell<T>,
) -> Vec<PreparedRow<T>> {
    let mut prepared = Vec::new();
    for (plan, row) in bounded_table_rows_with_column_count(rows, column_count)
        .into_iter()
        .zip(rows)
    {
        if mant_ir::table_row_is_navigation_only(row) {
            continue;
        }
        let prepared_row = match plan {
            TableRowPlan::Empty => PreparedRow::Complete(vec![T::plain("")], true),
            TableRowPlan::WholeRule { double } => PreparedRow::Rule(
                vec![if double {
                    TableRuleCellKind::DoubleHorizontal
                } else {
                    TableRuleCellKind::Horizontal
                }],
                false,
            ),
            TableRowPlan::LayoutRule { cells } => PreparedRow::Rule(cells.to_vec(), true),
            TableRowPlan::Dense { slots } => PreparedRow::Dense(
                slots
                    .into_iter()
                    .map(|cell| cell.map(|cell| render_table_cell(cell, render_cell)))
                    .collect(),
            ),
            TableRowPlan::Sparse { cells } => PreparedRow::Sparse(
                cells
                    .into_iter()
                    .map(|cell| (cell.column, render_table_cell(cell.cell, render_cell)))
                    .collect(),
            ),
        };
        prepared.push(prepared_row);
    }
    prepared
}

/// Column measurements are intrinsic and table-wide. Source cell alignment
/// and an explicit finite table frame use these slots even without a viewport;
/// ordinary owner row hints retain their historical final-viewport contract.
fn measured_alignment_slots<T: TableText>(
    prepared: &[PreparedRow<T>],
    viewport: Option<usize>,
    bounded_frame: bool,
) -> Option<Vec<usize>> {
    let mut alignment = bounded_frame;
    let mut widths = Vec::new();
    for row in prepared {
        if let PreparedRow::Dense(cells) = row {
            widths.resize(widths.len().max(cells.len()), 1);
            for (column, cell) in cells.iter().enumerate() {
                if let Some(cell) = cell {
                    alignment |= cell.alignment.is_some()
                        || (viewport.is_some() && cell.rows.iter().any(TableText::has_alignment));
                    widths[column] =
                        widths[column].max(cell.widths.iter().copied().max().unwrap_or(0));
                }
            }
        }
    }
    alignment.then_some(widths)
}

fn responsive_geometry(
    rows: &[TableRow],
    projection: TableOptions,
    borders: &mant_ir::TableBorders,
) -> bool {
    !borders.is_empty()
        || rows.iter().any(|row| {
            !row.column_borders.is_empty()
                || row
                    .cells
                    .iter()
                    .any(|cell| cell.column_span > 1 || cell.preferred_width_columns.is_some())
        })
        || projection.viewport.is_some()
}
