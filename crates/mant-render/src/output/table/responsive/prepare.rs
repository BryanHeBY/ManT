//! One source visit and conservative wrapping of prepared decoration receipts.
use super::{Block, CellText, Owner, Row, TableCell, TableRow, TableText, render_table_cell};
pub(super) fn prepare<'a, T: TableText>(
    rows: &'a [TableRow],
    positioned: Vec<Vec<mant_ir::PositionedTableCell<'a>>>,
    render_cell: &mut impl FnMut(&TableCell) -> super::super::RenderedCell<T>,
) -> Vec<Row<'a, T>> {
    rows.iter()
        .zip(positioned)
        .filter(|(row, _)| !mant_ir::table_row_is_navigation_only(row))
        .map(|(source, cells)| Row {
            source,
            owners: cells
                .into_iter()
                .map(|owner| Owner {
                    column: owner.column,
                    span: usize::try_from(owner.cell.column_span)
                        .unwrap_or(usize::MAX)
                        .max(1),
                    row_span: owner.cell.row_span,
                    dense_rows: None,
                    text: render_table_cell(owner.cell, render_cell),
                    wrap: !owner.cell.blocks.is_empty()
                        && owner.cell.blocks.iter().all(|block| {
                            matches!(block, Block::Paragraph { .. } | Block::VerticalSpace { .. })
                        }),
                })
                .collect(),
        })
        .collect::<Vec<_>>()
}

pub(super) fn wrap_owner<T: TableText>(owner: &mut Owner<T>, width: usize) -> bool {
    if owner.text.rows.iter().all(|row| row.width() <= width) {
        return true;
    }
    let mut rows = Vec::new();
    for row in &owner.text.rows {
        if row.width() > width {
            let Some(wrapped) = row.wrap_within(width) else {
                return false;
            };
            rows.extend(wrapped);
        } else {
            rows.push(row.clone());
        }
    }
    owner.dense_rows = Some(rows);
    true
}
pub(super) fn wrap<T: TableText>(cell: &mut CellText<T>, width: usize) -> bool {
    let mut rows = Vec::new();
    for row in &cell.rows {
        if row.width() > width {
            let Some(wrapped) = row.wrap_within(width) else {
                return false;
            };
            rows.extend(wrapped);
        } else {
            rows.push(row.clone());
        }
    }
    cell.rows = rows;
    cell.widths = cell.rows.iter().map(TableText::width).collect();
    true
}
