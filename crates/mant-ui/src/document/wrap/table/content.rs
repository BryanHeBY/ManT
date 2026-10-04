//! Content measurements place aligned cells within the bounded viewport.
use super::super::{
    Line, LogicalTableRow, RowCopyMap, Span, TableAlignment, WrappedLine, WrappedLink,
    WrappedSearchCell,
};
use super::cells::{stack_table_cells, wrap_table_cell};

pub(super) fn render_table_columns(
    indent: usize,
    table: &LogicalTableRow,
    column_widths: &[usize],
    gap: usize,
    width: usize,
) -> Vec<WrappedLine> {
    let mut next_search_group = 0;
    let rendered_cells = table
        .cells
        .iter()
        .zip(column_widths)
        .map(|(cell, column_width)| {
            if *column_width == 0 {
                return Vec::new();
            }
            let mut rows = wrap_table_cell(cell, *column_width, 0, &mut next_search_group);
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

    let measured = rendered_cells
        .iter()
        .map(|rows| {
            rows.iter()
                .map(|row| super::super::super::inline::spans_width(&row.line.spans))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if !content_column_origins_fit(indent, table, column_widths, gap, &measured, row_count) {
        return stack_table_cells(indent, table, width);
    }
    (0..row_count)
        .map(|row_index| {
            render_content_column_row(
                indent,
                table,
                column_widths,
                gap,
                &rendered_cells,
                &measured,
                row_index,
            )
        })
        .collect()
}

fn content_column_origins_fit(
    indent: usize,
    table: &LogicalTableRow,
    widths: &[usize],
    gap: usize,
    measured: &[Vec<usize>],
    height: usize,
) -> bool {
    let mut maximum = vec![0_usize; widths.len()];
    for row in 0..height {
        let mut column = 0_usize;
        let mut generated = gap.saturating_mul(widths.len().saturating_sub(1));
        for (index, width) in widths.iter().enumerate() {
            let used = measured[index].get(row).copied().unwrap_or(0);
            let free = width.saturating_sub(used);
            generated = generated.saturating_add(free);
            let left = match table
                .cells
                .get(index)
                .map_or(TableAlignment::Left, |cell| cell.alignment)
            {
                TableAlignment::Left => 0,
                TableAlignment::Center => free / 2,
                TableAlignment::Right => free,
            };
            maximum[index] = maximum[index].max(column.saturating_add(left));
            column = column.saturating_add(*width).saturating_add(gap);
        }
        if generated > mant_ir::geometry::MAX_COLUMN_PADDING {
            return false;
        }
    }
    let bounds = table
        .cells
        .iter()
        .map(|cell| cell.origin_bounds)
        .collect::<Vec<_>>();
    let pieces = maximum
        .into_iter()
        .enumerate()
        .map(|(cell, column)| mant_ir::geometry::ColumnPiece {
            cell,
            column,
            line: 0,
        })
        .collect::<Vec<_>>();
    mant_ir::geometry::table_column_origins_fit(
        &bounds,
        &[pieces],
        mant_ir::geometry::coordinate(indent),
    )
}

fn render_content_column_row(
    indent: usize,
    table: &LogicalTableRow,
    column_widths: &[usize],
    gap: usize,
    rendered_cells: &[Vec<WrappedLine>],
    measured: &[Vec<usize>],
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
        let alignment = table
            .cells
            .get(column)
            .map_or(TableAlignment::Left, |cell| cell.alignment);
        let mut used = 0;
        let mut left_padding = 0;
        if let Some(row) = cell_rows.and_then(|rows| rows.get(row_index)) {
            anchors.extend(row.anchors.iter().cloned());
            used = measured[column][row_index];
            let free = column_width.saturating_sub(used);
            left_padding = match alignment {
                TableAlignment::Left => 0,
                TableAlignment::Center => free / 2,
                TableAlignment::Right => free,
            };
            if left_padding > 0 {
                spans.push(Span::raw(" ".repeat(left_padding)));
            }
            spans.extend(row.line.spans.clone());
            copy_map.append_shifted(
                &row.copy_map,
                column_offset.saturating_add(left_padding),
                used,
            );
            links.extend(row.links.iter().map(|link| WrappedLink {
                target: link.target.clone(),
                start_column: column_offset + left_padding + link.start_column,
                end_column: column_offset + left_padding + link.end_column,
            }));
            search_cells.extend(row.search_cells.iter().map(|cell| WrappedSearchCell {
                group: cell.group,
                join_before: cell.join_before,
                character: cell.character,
                start_column: column_offset + left_padding + cell.start_column,
                end_column: column_offset + left_padding + cell.end_column,
            }));
        }
        spans.push(Span::raw(
            " ".repeat(column_width.saturating_sub(used + left_padding)),
        ));
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
