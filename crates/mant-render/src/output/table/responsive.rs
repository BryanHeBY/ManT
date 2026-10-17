//! Responsive, span-aware placement of one prepared visit per source cell.
//!
//! CVS `out.c::tblcalc_literal()` fills T{} paragraphs under a measured width,
//! while `tbl_term.c::tbl_literal()` gives spanning owners the complete region.
//! Portable consumers use resolved preferences and their own viewport here.
use super::{
    CellText, ProjectedTable, ProjectedTableRow, TableOptions, TableText, rows::render_table_cell,
};
use mant_ir::{
    Block, TableAlignment, TableBorders, TableCell, TableGrid, TableRow, TableRowKind, geometry,
};
mod frame;
mod spans;
use frame::Frame;
mod fallback;
mod measurement;
mod placement;
mod prepare;
use fallback::stack;
use measurement::{alignment_padding, column_start, measure, owner_area, owner_width};
use placement::dense;
use prepare::{prepare, wrap_owner};

struct Owner<T> {
    column: usize,
    span: usize,
    row_span: u32,
    text: CellText<T>,
    wrap: bool,
    dense_rows: Option<Vec<T>>,
}
impl<T> Owner<T> {
    fn rows(&self) -> &[T] {
        self.dense_rows.as_deref().unwrap_or(&self.text.rows)
    }
}
struct Row<'a, T> {
    source: &'a TableRow,
    owners: Vec<Owner<T>>,
}

pub(super) fn project<T: TableText>(
    rows: &[TableRow],
    projection: TableOptions,
    borders: &TableBorders,
    render_cell: &mut impl FnMut(&TableCell) -> super::RenderedCell<T>,
) -> ProjectedTable<T> {
    let grid = TableGrid::with_column_count(rows, projection.column_count);
    let count = grid.column_count;
    let mut prepared = prepare(rows, grid.rows, render_cell);
    if prepared.is_empty() {
        return ProjectedTable::from_rows(Vec::new());
    }
    let frame = Frame::new(rows, borders, count, usize::from(projection.gap));
    let available = projection
        .viewport
        .map(|width| width.saturating_sub(geometry::padding(projection.origin)));
    let columns = measure(
        &prepared,
        count,
        frame.gap,
        available.map(|width| width.saturating_sub(frame.outside())),
    );
    let mut stacked = columns.is_none();
    let sparse = requires_sparse(&prepared, count);
    if let Some(columns) = &columns {
        for row in &mut prepared {
            for owner in &mut row.owners {
                let width = owner_width(columns, owner.column, owner.span, frame.gap);
                if owner.wrap && !wrap_owner(owner, width) {
                    stacked = true;
                }
                stacked |= owner.rows().iter().any(|actual| actual.width() > width)
                    || (owner.text.break_after && owner.column.saturating_add(owner.span) < count);
            }
            stacked |= row
                .owners
                .iter()
                .map(|owner| owner.rows().len())
                .max()
                .unwrap_or(1)
                .saturating_mul(count)
                > super::MAX_DENSE_PHYSICAL_SLOTS;
        }
        let frame_width = columns
            .iter()
            .sum::<usize>()
            .saturating_add(frame.gap.saturating_mul(count.saturating_sub(1)))
            .saturating_add(frame.outside());
        stacked |= frame_width > geometry::MAX_COLUMN_PADDING;
        let local = if projection.bounded_frame || !borders.is_empty() {
            Some(frame_width.saturating_sub(frame.outside()))
        } else {
            available
        };
        for row in &prepared {
            for owner in &row.owners {
                let start =
                    column_start(columns, owner.column, frame.gap).saturating_add(frame.left());
                let slot = owner_width(columns, owner.column, owner.span, frame.gap);
                let area = owner_area(
                    owner,
                    count,
                    start.saturating_sub(frame.left()),
                    slot,
                    local,
                );
                for value in owner.rows() {
                    let extra = alignment_padding(value, &owner.text, slot, Some(area));
                    stacked |= !owner.text.origins.fits_at(geometry::compose_origin(
                        projection.origin,
                        geometry::coordinate(start.saturating_add(extra)),
                    ));
                }
            }
        }
    }
    if stacked {
        return stack(prepared, projection, frame, available, sparse);
    }
    let columns = columns.expect("dense columns selected");
    dense(&prepared, &columns, frame, projection, available)
}
fn requires_sparse<T: TableText>(rows: &[Row<'_, T>], count: usize) -> bool {
    count > mant_ir::MAX_DENSE_TABLE_COLUMNS
        || rows.iter().any(|row| {
            row.owners
                .iter()
                .map(|owner| owner.rows().len())
                .max()
                .unwrap_or(1)
                .saturating_mul(count)
                > super::MAX_DENSE_PHYSICAL_SLOTS
        })
}
