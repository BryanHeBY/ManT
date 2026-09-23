//! Dense table/block/row/cell references at the handle-bound FFI boundary.

use super::super::{
    BLOCK_TABLE, NativeStructuredError, StructuredSlices, alloc_error, relation_error,
};
use super::{dense_key, valid_required_key};

pub(super) fn validate_tables(slices: &StructuredSlices<'_>) -> Result<(), NativeStructuredError> {
    let mut table_blocks = Vec::new();
    table_blocks
        .try_reserve_exact(slices.blocks.len())
        .map_err(alloc_error)?;
    table_blocks.resize(slices.blocks.len(), false);
    for (index, table) in slices.tables.iter().enumerate() {
        let block = table
            .block
            .checked_sub(1)
            .and_then(|key| slices.blocks.get(key as usize));
        if table.key != dense_key(index)?
            || block.is_none_or(|block| block.kind != BLOCK_TABLE || block.table != table.key)
            || table.fixed_view != 0
            || !valid_required_key(table.provenance, slices.provenances.len())
            || table.reserved != 0
            || table_blocks
                .get(table.block as usize - 1)
                .is_none_or(|seen| *seen)
        {
            return Err(relation_error());
        }
        table_blocks[table.block as usize - 1] = true;
    }
    if slices
        .blocks
        .iter()
        .enumerate()
        .any(|(index, block)| (block.kind == BLOCK_TABLE) != table_blocks[index])
    {
        return Err(relation_error());
    }
    let mut row_counts = Vec::new();
    row_counts
        .try_reserve_exact(slices.tables.len())
        .map_err(alloc_error)?;
    row_counts.resize(slices.tables.len(), 0_u32);
    for (index, row) in slices.table_rows.iter().enumerate() {
        let expected = row
            .table
            .checked_sub(1)
            .and_then(|key| row_counts.get_mut(key as usize));
        if row.key != dense_key(index)?
            || expected.as_ref().is_none_or(|count| row.ordinal != **count)
            || !(1..=4).contains(&row.kind)
            || row.point as usize > slices.content_points.len()
            || !valid_required_key(row.provenance, slices.provenances.len())
            || row.reserved != 0
        {
            return Err(relation_error());
        }
        let expected = expected.expect("validated table row");
        *expected = expected.checked_add(1).ok_or_else(relation_error)?;
    }
    validate_cells(slices)
}

fn validate_cells(slices: &StructuredSlices<'_>) -> Result<(), NativeStructuredError> {
    let mut previous_row = 0_u32;
    let mut next_column = 0_u32;
    let mut cell_owners = Vec::new();
    cell_owners
        .try_reserve_exact(slices.owners.len())
        .map_err(alloc_error)?;
    cell_owners.resize(slices.owners.len(), false);
    let mut row_cells = Vec::new();
    row_cells
        .try_reserve_exact(slices.table_rows.len())
        .map_err(alloc_error)?;
    row_cells.resize(slices.table_rows.len(), 0_u32);
    for (index, cell) in slices.table_cells.iter().enumerate() {
        let row = cell
            .row
            .checked_sub(1)
            .and_then(|key| slices.table_rows.get(key as usize));
        let owner = cell
            .owner
            .checked_sub(1)
            .and_then(|key| slices.owners.get(key as usize));
        let point = cell
            .point
            .checked_sub(1)
            .and_then(|key| slices.content_points.get(key as usize));
        if cell.row != previous_row {
            next_column = 0;
        }
        if cell.key != dense_key(index)?
            || row.is_none_or(|row| row.kind != 1 && row.kind != 4)
            || cell.row < previous_row
            || cell.column != next_column
            || owner.is_none_or(|owner| owner.kind != 6)
            || cell_owners
                .get(cell.owner as usize - 1)
                .is_none_or(|seen| *seen)
            || !(1..=5).contains(&cell.kind)
            || row.is_some_and(|row| row.kind == 4 && !matches!(cell.kind, 2 | 3))
            || !(1..=3).contains(&cell.alignment)
            || cell.row_span == 0
            || cell.column_span == 0
            || point.is_none_or(|point| {
                point.owner != cell.owner
                    || slices
                        .content_roots
                        .get(point.root as usize - 1)
                        .is_none_or(|root| root.kind != 4)
            })
            || !valid_required_key(cell.provenance, slices.provenances.len())
            || cell.reserved != 0
        {
            return Err(relation_error());
        }
        cell_owners[cell.owner as usize - 1] = true;
        row_cells[cell.row as usize - 1] = row_cells[cell.row as usize - 1]
            .checked_add(1)
            .ok_or_else(relation_error)?;
        previous_row = cell.row;
        next_column = cell
            .column
            .checked_add(cell.column_span)
            .ok_or_else(relation_error)?;
    }
    if slices
        .owners
        .iter()
        .enumerate()
        .any(|(index, owner)| (owner.kind == 6) != cell_owners[index])
    {
        return Err(relation_error());
    }
    if slices
        .table_rows
        .iter()
        .enumerate()
        .any(|(index, row)| row.kind == 4 && row_cells[index] == 0)
    {
        return Err(relation_error());
    }
    Ok(())
}
