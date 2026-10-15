//! Place prepared physical cell rows without revisiting their source owners.
use super::{CellText, MAX_DENSE_PHYSICAL_SLOTS, PhysicalRows, TableMode, TableText};
use mant_ir::{TableAlignment, TableCell, TableCellKind};

fn dense_height<T: TableText>(cells: &[Option<CellText<T>>]) -> usize {
    cells
        .iter()
        .filter_map(Option::as_ref)
        .map(CellText::line_count)
        .max()
        .unwrap_or(1)
}

pub(super) fn dense_mode<T: TableText>(
    cells: &[Option<CellText<T>>],
    gap: usize,
    origin: i32,
    viewport: Option<usize>,
    slots: Option<&[usize]>,
) -> TableMode {
    if cells
        .iter()
        .take(cells.len().saturating_sub(1))
        .flatten()
        .any(|cell| cell.break_after)
        || gap.saturating_mul(cells.len().saturating_sub(1)) > mant_ir::geometry::MAX_COLUMN_PADDING
    {
        return TableMode::Stack;
    }
    let height = dense_height(cells);
    if height
        .checked_mul(cells.len())
        .is_none_or(|work| work > MAX_DENSE_PHYSICAL_SLOTS)
    {
        return TableMode::Sparse;
    }
    if slots.zip(viewport).is_some_and(|(slots, viewport)| {
        slots
            .iter()
            .sum::<usize>()
            .saturating_add((gap + 1).saturating_mul(slots.len().saturating_sub(1)))
            > viewport
    }) || !dense_origins_fit(cells, height, gap, origin, viewport, slots)
    {
        return TableMode::Stack;
    }
    TableMode::Dense
}

pub(super) fn sparse_fits<T: TableText>(cells: &[(usize, CellText<T>)], origin: i32) -> bool {
    cells.iter().all(|(column, cell)| {
        let prefix = format!("column {}: ", column.saturating_add(1));
        cell.origins.fits_at(mant_ir::geometry::compose_origin(
            origin,
            mant_ir::geometry::coordinate(prefix.len()),
        ))
    })
}

fn separator(gap: usize) -> String {
    // Larger gaps only reach a single segment, which uses no separator.
    let gap = gap.min(mant_ir::geometry::MAX_COLUMN_PADDING);
    format!("{}|{}", " ".repeat(gap / 2), " ".repeat(gap - gap / 2))
}

pub(super) fn render_table_cell<T: TableText>(
    cell: &TableCell,
    render_cell: &mut impl FnMut(&TableCell) -> (T, bool, bool),
) -> CellText<T> {
    let (value, completed, physical) = match cell.kind {
        TableCellKind::Text => render_cell(cell),
        TableCellKind::HorizontalRule | TableCellKind::IsolatedHorizontalRule => {
            (T::plain("---"), false, true)
        }
        TableCellKind::DoubleHorizontalRule | TableCellKind::IsolatedDoubleHorizontalRule => {
            (T::plain("==="), false, true)
        }
    };
    let empty = value.is_empty();
    let closes_open_tail = cell.break_after && !completed && value.ends_with_break();
    let mut rows = value.physical_lines();
    // Retiring an occupied row's close must not retire an explicitly completed
    // empty source row. Keep this fact in the prepared cell for every fallback.
    if closes_open_tail {
        rows.push(T::plain(""));
    }
    let widths = rows.iter().map(TableText::width).collect();
    CellText {
        rows,
        widths,
        empty,
        physical: if physical {
            PhysicalRows::Present
        } else {
            PhysicalRows::Absent
        },
        alignment: cell.alignment,
        rule: match cell.kind {
            TableCellKind::HorizontalRule | TableCellKind::IsolatedHorizontalRule => Some('-'),
            TableCellKind::DoubleHorizontalRule | TableCellKind::IsolatedDoubleHorizontalRule => {
                Some('=')
            }
            TableCellKind::Text => None,
        },
        completed,
        break_after: cell.break_after,
        origins: mant_ir::geometry::table_cell_origin_bounds(cell),
    }
}

pub(super) fn stacked_physical_rows<T: TableText>(
    cells: Vec<CellText<T>>,
    viewport: Option<usize>,
) -> (Vec<T>, bool, bool) {
    let mut completed = false;
    let mut rows = Vec::new();
    for cell in cells {
        if cell.physical == PhysicalRows::Absent && !cell.break_after {
            continue;
        }
        if cell.break_after && !cell.completed && cell.empty {
            // An empty field can complete the current data row. With no
            // active row (or after an already completed row) it owns one
            // existing empty row, rather than disappearing from topology.
            if rows.is_empty() || completed {
                rows.extend(cell.rows);
            } else if let Some(last) = rows.last_mut() {
                for row in cell.rows {
                    last.append(&row);
                }
            }
            completed = true;
            continue;
        }
        let physical = cell
            .rows
            .into_iter()
            .map(|row| row.align_within(viewport))
            .collect::<Vec<_>>();
        completed =
            cell.completed || cell.break_after || physical.last().is_some_and(TableText::is_empty);
        rows.extend(physical);
    }
    if rows.is_empty() {
        rows.push(T::plain(""));
        completed = true;
    }
    (rows, completed, true)
}

pub(super) fn dense_physical_rows<T: TableText>(
    cells: &[Option<CellText<T>>],
    gap: usize,
    viewport: Option<usize>,
    slots: Option<&[usize]>,
) -> (Vec<T>, bool, bool) {
    let height = dense_height(cells);
    let completed = cells.iter().flatten().any(|cell| {
        (cell.completed || cell.break_after || cell.rows.last().is_some_and(TableText::is_empty))
            && cell.rows.len() == height
    });
    (
        assemble_dense_rows(cells, height, gap, viewport, slots),
        completed,
        false,
    )
}

fn dense_origins_fit<T: TableText>(
    cells: &[Option<CellText<T>>],
    height: usize,
    gap: usize,
    origin: i32,
    viewport: Option<usize>,
    slots: Option<&[usize]>,
) -> bool {
    let mut maximum = vec![0_usize; cells.len()];
    for line in 0..height {
        let mut column = 0_usize;
        for (index, cell) in cells.iter().enumerate() {
            let area = slots.and_then(|slots| {
                if index + 1 < cells.len() {
                    slots.get(index).copied()
                } else {
                    viewport.map(|width| width.saturating_sub(column))
                }
            });
            let extra = cell
                .as_ref()
                .and_then(|cell| {
                    cell.rows.get(line).map(|row| {
                        let owner = row.alignment_offset(area);
                        let spare = slots
                            .and_then(|slots| slots.get(index).copied())
                            .unwrap_or(0)
                            .saturating_sub(row.width().saturating_add(owner));
                        owner
                            + match if row.has_alignment() {
                                None
                            } else {
                                cell.alignment
                            } {
                                Some(TableAlignment::Center) => spare / 2,
                                Some(TableAlignment::Right) => spare,
                                _ => 0,
                            }
                    })
                })
                .unwrap_or(0);
            maximum[index] = maximum[index].max(column.saturating_add(extra));
            column = column
                .saturating_add(slots.map_or_else(
                    || {
                        cell.as_ref()
                            .and_then(|cell| cell.widths.get(line))
                            .copied()
                            .unwrap_or(0)
                    },
                    |slots| slots[index],
                ))
                .saturating_add(gap + 1);
        }
    }
    cells.iter().zip(maximum).all(|(cell, start)| {
        cell.as_ref().is_none_or(|cell| {
            cell.origins.fits_at(mant_ir::geometry::compose_origin(
                origin,
                mant_ir::geometry::coordinate(start),
            ))
        })
    })
}

fn assemble_dense_rows<T: TableText>(
    cells: &[Option<CellText<T>>],
    height: usize,
    gap: usize,
    viewport: Option<usize>,
    slots: Option<&[usize]>,
) -> Vec<T> {
    let separator = separator(gap);
    (0..height)
        .map(|line| {
            let mut segments = cells
                .iter()
                .map(|cell| {
                    cell.as_ref()
                        .and_then(|cell| cell.rows.get(line))
                        .cloned()
                        .unwrap_or_else(|| T::plain(""))
                })
                .collect::<Vec<_>>();
            let mut retired = Vec::new();
            while line > 0 && segments.last().is_some_and(TableText::is_empty) {
                retired.push(segments.pop().expect("empty tail"));
            }
            let mut slot_start = 0usize;
            for (column, segment) in segments.iter_mut().enumerate() {
                if let Some(character) = cells[column].as_ref().and_then(|cell| cell.rule)
                    && let Some(width) = slots.and_then(|slots| slots.get(column))
                    && *width <= mant_ir::geometry::MAX_COLUMN_PADDING
                {
                    *segment = T::plain(&character.to_string().repeat(*width));
                }
                let area = slots.and_then(|slots| {
                    if column + 1 == cells.len() {
                        viewport.map(|width| width.saturating_sub(slot_start))
                    } else {
                        slots.get(column).copied()
                    }
                });
                let alignment = cells
                    .get(column)
                    .and_then(Option::as_ref)
                    .and_then(|cell| cell.alignment);
                // Owner row hints and cell alignment have different slots.
                // A real owner hint takes precedence; it keeps the automatic
                // last-column viewport contract even with a default cell L.
                let has_owner_alignment = segment.has_alignment();
                let owner = segment.clone().align_within(area);
                *segment = align_cell(
                    owner,
                    slots.and_then(|slots| slots.get(column).copied()),
                    if has_owner_alignment { None } else { alignment },
                );
                if let Some(area) = area
                    && (column + 1 < cells.len() || cells[column].is_none())
                    && segment.width() < area
                {
                    segment.append(&T::plain(&" ".repeat(area - segment.width())));
                }
                slot_start = slot_start
                    .saturating_add(slots.map_or(0, |slots| slots[column]))
                    .saturating_add(gap + 1);
            }
            let mut output = T::join(&segments, &separator);
            for tail in retired.into_iter().rev() {
                output.append(&tail);
            }
            output
        })
        .collect()
}

fn align_cell<T: TableText>(row: T, width: Option<usize>, alignment: Option<TableAlignment>) -> T {
    let Some(width) = width else { return row };
    if row.is_empty() {
        return row;
    }
    let spare = width.saturating_sub(row.width());
    let padding = match alignment {
        Some(TableAlignment::Center) => spare / 2,
        Some(TableAlignment::Right) => spare,
        _ => 0,
    };
    if padding > mant_ir::geometry::MAX_COLUMN_PADDING {
        return row;
    }
    row.prefixed(&" ".repeat(padding))
}

pub(super) fn sparse_physical_rows<T: TableText>(
    cells: Vec<(usize, CellText<T>)>,
    origin: i32,
    viewport: Option<usize>,
) -> (Vec<T>, bool, bool) {
    if !sparse_fits(&cells, origin) {
        return stacked_physical_rows(cells.into_iter().map(|(_, cell)| cell).collect(), viewport);
    }
    let mut completed = false;
    let output = cells
        .into_iter()
        .filter(|(_, cell)| cell.physical == PhysicalRows::Present || cell.break_after)
        .flat_map(|(column, cell)| {
            let physical = cell.rows;
            completed = cell.completed
                || cell.break_after
                || physical.last().is_some_and(TableText::is_empty);
            physical
                .into_iter()
                .enumerate()
                .map(|(line, value)| {
                    value
                        .prefixed(&if line == 0 {
                            format!("column {}: ", column.saturating_add(1))
                        } else {
                            "  ".into()
                        })
                        .align_within(viewport)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    (output, completed, true)
}
