//! Table-wide measurements and finite owner regions before placement.
use super::{CellText, Owner, Row, TableAlignment, TableCell, TableText, geometry};
pub(super) fn measure<T: TableText>(
    rows: &[Row<'_, T>],
    count: usize,
    gap: usize,
    available: Option<usize>,
) -> Option<Vec<usize>> {
    if count > mant_ir::MAX_DENSE_TABLE_COLUMNS {
        return None;
    }
    let mut widths = vec![1; count];
    for row in rows {
        for (owner, original) in row.owners.iter().zip(&row.source.cells) {
            if owner.span == 1 {
                let width = preferred_width(owner, original);
                widths[owner.column] = widths[owner.column].max(width);
            }
        }
    }
    for row in rows {
        for (owner, original) in row.owners.iter().zip(&row.source.cells) {
            if owner.span > 1 {
                let end = owner.column.saturating_add(owner.span).min(count);
                let width = preferred_width(owner, original);
                let missing =
                    width.saturating_sub(owner_width(&widths, owner.column, owner.span, gap));
                let covered = end.saturating_sub(owner.column).max(1);
                for (index, value) in widths[owner.column..end].iter_mut().enumerate() {
                    *value = value
                        .saturating_add(missing / covered + usize::from(index < missing % covered));
                }
            }
        }
    }
    let natural = widths
        .iter()
        .try_fold(0usize, |sum, width| sum.checked_add(*width))?
        .checked_add(gap.checked_mul(count.saturating_sub(1))?)?;
    geometry::allocate_table_columns(&widths, available.unwrap_or(natural), gap)
}
fn preferred_width<T: TableText>(owner: &Owner<T>, original: &TableCell) -> usize {
    let natural = owner.text.widths.iter().copied().max().unwrap_or(0);
    original.preferred_width_columns.map_or(natural, |width| {
        let word = if owner.wrap {
            owner
                .text
                .rows
                .iter()
                .map(TableText::longest_word_width)
                .max()
                .unwrap_or(0)
        } else {
            natural
        };
        usize::from(width).max(word)
    })
}
pub(super) fn owner_width(columns: &[usize], column: usize, span: usize, gap: usize) -> usize {
    let end = column.saturating_add(span).min(columns.len());
    columns
        .get(column..end)
        .unwrap_or_default()
        .iter()
        .sum::<usize>()
        .saturating_add(gap.saturating_mul(end.saturating_sub(column).saturating_sub(1)))
}
pub(super) fn column_start(columns: &[usize], column: usize, gap: usize) -> usize {
    columns[..column.min(columns.len())]
        .iter()
        .sum::<usize>()
        .saturating_add(gap.saturating_mul(column))
}
pub(super) fn owner_area<T>(
    owner: &Owner<T>,
    count: usize,
    start: usize,
    slot: usize,
    viewport: Option<usize>,
) -> usize {
    if owner.column.saturating_add(owner.span) == count {
        viewport.map_or(slot, |width| width.saturating_sub(start))
    } else {
        slot
    }
}
pub(super) fn alignment_padding<T: TableText>(
    value: &T,
    cell: &CellText<T>,
    slot: usize,
    area: Option<usize>,
) -> usize {
    let owner = value.alignment_offset(area);
    owner
        + if value.has_alignment() {
            0
        } else {
            match cell.alignment {
                Some(TableAlignment::Center) => slot.saturating_sub(value.width()) / 2,
                Some(TableAlignment::Right) => slot.saturating_sub(value.width()),
                _ => 0,
            }
        }
}
