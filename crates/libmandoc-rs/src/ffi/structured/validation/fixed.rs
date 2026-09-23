//! Independent handle-bound validation of native physical geometry.

use unicode_width::UnicodeWidthStr;

use super::super::{
    BLOCK_FIXED_DISPLAY, NativeStructuredError, StructuredSlices, alloc_error, relation_error,
};
use super::{dense_key, preflight::validate_utf8_view, utf8_boundary, valid_required_key};

#[allow(clippy::too_many_lines)] // Keep the one-pass handle-bound relation checks together.
pub(super) fn validate_fixed(
    slices: &StructuredSlices<'_>,
    atom_scalar_starts: &[u32],
) -> Result<(), NativeStructuredError> {
    let mut owner_table = Vec::new();
    owner_table
        .try_reserve_exact(slices.owners.len())
        .map_err(alloc_error)?;
    owner_table.resize(slices.owners.len(), 0_u32);
    for cell in slices.table_cells {
        owner_table[cell.owner as usize - 1] = slices.table_rows[cell.row as usize - 1].table;
    }
    for (index, view) in slices.fixed_views.iter().enumerate() {
        let block = view
            .block
            .checked_sub(1)
            .and_then(|key| slices.blocks.get(key as usize));
        let valid_table = if view.table == 0 {
            block.is_some_and(|block| block.kind == BLOCK_FIXED_DISPLAY)
        } else {
            slices
                .tables
                .get(view.table as usize - 1)
                .is_some_and(|table| table.block == view.block && table.fixed_view == view.key)
        };
        if view.key != dense_key(index)?
            || !valid_required_key(view.owner, slices.owners.len())
            || block.is_none_or(|block| block.owner != view.owner || block.fixed_view != view.key)
            || !valid_table
            || !valid_required_key(view.provenance, slices.provenances.len())
            || view.reserved != 0
        {
            return Err(relation_error());
        }
    }
    for (index, table) in slices.tables.iter().enumerate() {
        if table.fixed_view != 0
            && slices
                .fixed_views
                .get(table.fixed_view as usize - 1)
                .is_none_or(|view| view.table as usize != index + 1)
        {
            return Err(relation_error());
        }
    }
    for (index, block) in slices.blocks.iter().enumerate() {
        if block.fixed_view != 0
            && slices
                .fixed_views
                .get(block.fixed_view as usize - 1)
                .is_none_or(|view| view.block as usize != index + 1)
        {
            return Err(relation_error());
        }
    }
    let mut previous_view = 0_u32;
    let mut ordinal = 0_u32;
    let mut total_columns = 0_u64;
    for (index, line) in slices.fixed_lines.iter().enumerate() {
        if line.view != previous_view {
            if line.view != previous_view + 1 {
                return Err(relation_error());
            }
            previous_view = line.view;
            ordinal = 0;
        }
        if line.key != dense_key(index)?
            || !valid_required_key(line.view, slices.fixed_views.len())
            || line.ordinal != ordinal
            || line.total_columns > 1_048_576
            || line.reserved != 0
        {
            return Err(relation_error());
        }
        ordinal = ordinal.checked_add(1).ok_or_else(relation_error)?;
        total_columns = total_columns
            .checked_add(u64::from(line.total_columns))
            .ok_or_else(relation_error)?;
        if total_columns > 32 * 1024 * 1024 {
            return Err(relation_error());
        }
    }
    if previous_view as usize != slices.fixed_views.len() {
        return Err(relation_error());
    }

    let mut occupied = Vec::new();
    let occupied_count = slices
        .placements
        .len()
        .checked_add(slices.decorations.len())
        .ok_or_else(relation_error)?;
    occupied
        .try_reserve_exact(occupied_count)
        .map_err(alloc_error)?;
    let mut previous_line = 0_u32;
    let mut ordinal = 0_u32;
    let mut scalar_work = 0_u64;
    for (index, placement) in slices.placements.iter().enumerate() {
        if placement.line != previous_line {
            if placement.line < previous_line {
                return Err(relation_error());
            }
            previous_line = placement.line;
            ordinal = 0;
        }
        let line = placement
            .line
            .checked_sub(1)
            .and_then(|key| slices.fixed_lines.get(key as usize));
        let view = line.and_then(|line| slices.fixed_views.get(line.view as usize - 1));
        if placement.key != dense_key(index)?
            || line.is_none_or(|line| placement.column_end > line.total_columns)
            || placement.ordinal != ordinal
            || placement.column_start > placement.column_end
            || placement.scalar_start > placement.scalar_end
            || placement.reserved != 0
        {
            return Err(relation_error());
        }
        ordinal = ordinal.checked_add(1).ok_or_else(relation_error)?;
        let view = view.ok_or_else(relation_error)?;
        match placement.target_kind {
            1 => {
                let atom = placement
                    .atom
                    .checked_sub(1)
                    .and_then(|key| slices.content_atoms.get(key as usize))
                    .ok_or_else(relation_error)?;
                if placement.point != 0
                    || placement.byte_start >= placement.byte_end
                    || (placement.column_start == placement.column_end
                        && placement.cell_map_kind != 2)
                    || !matches!(atom.kind, 1 | 2)
                    || !utf8_boundary(atom.text, placement.byte_start)
                    || !utf8_boundary(atom.text, placement.byte_end)
                    || (view.table != 0 && owner_table[atom.owner as usize - 1] != view.table)
                    || (view.table == 0 && atom.owner != view.owner)
                {
                    return Err(relation_error());
                }
                let text_len = usize::try_from(atom.text.len).map_err(|_| relation_error())?;
                let bytes = unsafe { std::slice::from_raw_parts(atom.text.ptr, text_len) };
                let text = std::str::from_utf8(bytes).map_err(|_| relation_error())?;
                let start = placement.byte_start as usize;
                let end = placement.byte_end as usize;
                let scalar_start = atom_scalar_starts[placement.atom as usize - 1]
                    .checked_add(
                        u32::try_from(text[..start].chars().count())
                            .map_err(|_| relation_error())?,
                    )
                    .ok_or_else(relation_error)?;
                let scalar_end = scalar_start
                    .checked_add(
                        u32::try_from(text[start..end].chars().count())
                            .map_err(|_| relation_error())?,
                    )
                    .ok_or_else(relation_error)?;
                if placement.scalar_start != scalar_start
                    || placement.scalar_end != scalar_end
                    || !valid_map(placement, scalar_end - scalar_start)
                {
                    return Err(relation_error());
                }
                scalar_work = scalar_work
                    .checked_add(
                        u64::from(placement.byte_end)
                            + u64::from(placement.byte_end - placement.byte_start),
                    )
                    .ok_or_else(relation_error)?;
                if scalar_work > 32 * 1024 * 1024 {
                    return Err(relation_error());
                }
                occupied.push((
                    placement.line,
                    placement.column_start,
                    placement.column_end,
                    placement.cell_map_kind == 3,
                ));
            }
            2 => {
                let point = placement
                    .point
                    .checked_sub(1)
                    .and_then(|key| slices.content_points.get(key as usize))
                    .ok_or_else(relation_error)?;
                if placement.atom != 0
                    || placement.byte_start != 0
                    || placement.byte_end != 0
                    || placement.column_start != placement.column_end
                    || placement.scalar_start != point.scalar_boundary
                    || placement.scalar_end != point.scalar_boundary
                    || placement.cell_map_kind != 1
                    || placement.cell_map_value != 0
                    || (view.table != 0 && owner_table[point.owner as usize - 1] != view.table)
                    || (view.table == 0 && point.owner != view.owner)
                {
                    return Err(relation_error());
                }
            }
            _ => return Err(relation_error()),
        }
    }

    previous_line = 0;
    ordinal = 0;
    for (index, decoration) in slices.decorations.iter().enumerate() {
        if decoration.line != previous_line {
            if decoration.line < previous_line {
                return Err(relation_error());
            }
            previous_line = decoration.line;
            ordinal = 0;
        }
        let line = decoration
            .line
            .checked_sub(1)
            .and_then(|key| slices.fixed_lines.get(key as usize));
        let text_len = validate_utf8_view(decoration.text)?;
        if text_len == 0 {
            return Err(relation_error());
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(
                decoration.text.ptr,
                usize::try_from(text_len).map_err(|_| relation_error())?,
            )
        };
        let text = std::str::from_utf8(bytes).map_err(|_| relation_error())?;
        if decoration.key != dense_key(index)?
            || line.is_none_or(|line| decoration.column_end > line.total_columns)
            || decoration.ordinal != ordinal
            || !(1..=3).contains(&decoration.kind)
            || decoration.column_start >= decoration.column_end
            || text.is_empty()
            || text.chars().any(char::is_control)
            || UnicodeWidthStr::width(text)
                != (decoration.column_end - decoration.column_start) as usize
            || !valid_required_key(decoration.provenance, slices.provenances.len())
            || decoration.reserved != 0
        {
            return Err(relation_error());
        }
        ordinal = ordinal.checked_add(1).ok_or_else(relation_error)?;
        occupied.push((
            decoration.line,
            decoration.column_start,
            decoration.column_end,
            false,
        ));
    }
    occupied.sort_unstable();
    for pair in occupied.windows(2) {
        if pair[0].0 == pair[1].0
            && pair[0].2 > pair[1].1
            && !(pair[0].1 == pair[1].1 && pair[0].2 == pair[1].2 && (pair[0].3 || pair[1].3))
        {
            return Err(relation_error());
        }
    }
    Ok(())
}

fn valid_map(placement: &super::super::PlacementView, scalars: u32) -> bool {
    let width = placement.column_end - placement.column_start;
    match placement.cell_map_kind {
        1 => {
            (1..=u32::from(u8::MAX)).contains(&placement.cell_map_value)
                && scalars.checked_mul(placement.cell_map_value) == Some(width)
        }
        2 | 3 => placement.cell_map_value == 0 && scalars == 1,
        _ => false,
    }
}
