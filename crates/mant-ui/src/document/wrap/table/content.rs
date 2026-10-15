//! Content measurements place aligned cells within the bounded viewport.
use super::super::{
    Line, LogicalTableRow, RowCopyMap, Span, TableAlignment, WrappedLine, WrappedLink,
    WrappedSearchCell,
};
use super::cells::wrap_table_cell;

pub(super) fn render_table_columns(
    indent: usize,
    table: &LogicalTableRow,
    column_widths: &[usize],
    gap: usize,
    width: usize,
) -> Option<Vec<WrappedLine>> {
    let mut next_search_group = 0;
    let mut rendered_cells = table
        .cells
        .iter()
        .zip(column_widths)
        .map(|(cell, column_width)| {
            if *column_width == 0 {
                return Vec::new();
            }
            let mut rows = wrap_table_cell(cell, *column_width, 0, &mut next_search_group, true);
            if cell.has_open_tail()
                && let Some(tail) = rows.pop()
                && let Some(last) = rows.last_mut()
            {
                last.anchors.extend(tail.anchors);
            }
            rows
        })
        .collect::<Vec<_>>();
    let row_count = rendered_cells
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(1);

    if !position_cells(
        indent,
        table,
        column_widths,
        gap,
        width,
        &mut rendered_cells,
        row_count,
    ) {
        return None;
    }
    Some(
        (0..row_count)
            .map(|row_index| {
                render_content_column_row(indent, column_widths, gap, &rendered_cells, row_index)
            })
            .collect(),
    )
}

fn position_cells(
    indent: usize,
    table: &LogicalTableRow,
    widths: &[usize],
    gap: usize,
    viewport: usize,
    cells: &mut [Vec<WrappedLine>],
    height: usize,
) -> bool {
    let row_width = widths.iter().fold(
        gap.saturating_mul(widths.len().saturating_sub(1)),
        |sum, width| sum.saturating_add(*width),
    );
    let mut generated = vec![row_width; height];
    let mut column = 0_usize;
    for (index, width) in widths.iter().enumerate() {
        let Some(rows) = cells.get_mut(index) else {
            column = column.saturating_add(*width).saturating_add(gap);
            continue;
        };
        let owner_area = if index + 1 == widths.len() {
            viewport.saturating_sub(indent.saturating_add(column))
        } else {
            *width
        };
        for (line, row) in rows.iter_mut().enumerate() {
            let before = super::super::super::inline::spans_width(&row.line.spans);
            let owner_padding = row.alignment_padding(owner_area);
            let free = width.saturating_sub(before);
            let offset = owner_padding.unwrap_or_else(|| match table.cells[index].alignment {
                TableAlignment::Left => 0,
                TableAlignment::Center => free / 2,
                TableAlignment::Right => free,
            });
            generated[line] = generated[line]
                .saturating_sub(*width)
                .saturating_add(free.max(offset));
            let origin = indent.saturating_add(column).saturating_add(offset);
            if offset > mant_ir::geometry::MAX_COLUMN_PADDING
                || !table.cells[index]
                    .origin_bounds
                    .fits_at(mant_ir::geometry::coordinate(origin))
            {
                return false;
            }
            if owner_padding.is_some() {
                row.position_alignment(owner_area);
            } else {
                super::translate(row, offset);
            }
        }
        column = column.saturating_add(*width).saturating_add(gap);
    }
    generated
        .iter()
        .all(|padding| *padding <= mant_ir::geometry::MAX_COLUMN_PADDING)
}

fn render_content_column_row(
    indent: usize,
    column_widths: &[usize],
    gap: usize,
    rendered_cells: &[Vec<WrappedLine>],
    row_index: usize,
) -> WrappedLine {
    let mut anchors = Vec::new();
    let mut spans = Vec::new();
    let mut links = Vec::new();
    let mut search_cells = Vec::new();
    let mut copy_map = RowCopyMap::default();
    let mut column_offset = indent;
    if indent > 0 {
        spans.push(Span::raw(" ".repeat(indent)));
    }
    for (column, column_width) in column_widths.iter().enumerate() {
        let cell_rows = rendered_cells.get(column);
        let mut used = 0;
        if let Some(row) = cell_rows.and_then(|rows| rows.get(row_index)) {
            anchors.extend(row.anchors.iter().cloned());
            used = super::super::super::inline::spans_width(&row.line.spans);
            spans.extend(row.line.spans.clone());
            copy_map.append_shifted(&row.copy_map, column_offset, used);
            links.extend(row.links.iter().map(|link| WrappedLink {
                target: link.target.clone(),
                start_column: column_offset + link.start_column,
                end_column: column_offset + link.end_column,
            }));
            search_cells.extend(row.search_cells.iter().map(|cell| WrappedSearchCell {
                group: cell.group,
                join_before: cell.join_before,
                character: cell.character,
                start_column: column_offset + cell.start_column,
                end_column: column_offset + cell.end_column,
            }));
        }
        spans.push(Span::raw(" ".repeat(column_width.saturating_sub(used))));
        column_offset += column_width;
        if column + 1 < column_widths.len() {
            spans.push(Span::raw(" ".repeat(gap)));
            column_offset += gap;
        }
    }
    if rendered_cells.iter().all(|rows| {
        rows.get(row_index).is_none_or(|row| {
            row.source_end.is_none() && row.line.spans.iter().all(|span| span.content.is_empty())
        })
    }) {
        spans.clear();
    }
    WrappedLine {
        source_end: None,
        anchors,
        line: Line::from(spans),
        links,
        search_cells,
        copy_map,
    }
}

pub(super) fn table_column_widths(
    preferred_widths: &[usize],
    available: usize,
    gap: usize,
) -> Option<Vec<usize>> {
    const SOFT_MINIMUM: usize = 8;

    if preferred_widths.is_empty() {
        return Some(Vec::new());
    }
    let gaps = preferred_widths.len().saturating_sub(1) * gap;
    if gaps > mant_ir::geometry::MAX_COLUMN_PADDING {
        return None;
    }
    let usable = available.checked_sub(gaps)?;
    let preferred = preferred_widths
        .iter()
        .map(|width| (*width).max(1))
        .collect::<Vec<_>>();
    if preferred.iter().sum::<usize>() <= usable {
        return Some(preferred);
    }

    let mut widths = preferred
        .iter()
        .map(|width| (*width).min(SOFT_MINIMUM))
        .collect::<Vec<_>>();
    let minimum = widths.iter().sum::<usize>();
    if minimum > usable {
        return None;
    }

    let mut remaining = usable - minimum;
    while remaining > 0 {
        let mut advanced = false;
        for (width, preferred) in widths.iter_mut().zip(&preferred) {
            if *width < *preferred {
                *width += 1;
                remaining -= 1;
                advanced = true;
                if remaining == 0 {
                    break;
                }
            }
        }
        if !advanced {
            break;
        }
    }
    Some(widths)
}
