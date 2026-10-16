//! Prepared cells occupy complete logical spans before viewport placement.
use super::super::{
    Line, LogicalTableRow, RowCopyMap, Span, TableAlignment, WrappedLine, WrappedLink,
    WrappedSearchCell,
};
use super::cells::wrap_table_cell;

pub(super) fn span_end(table: &LogicalTableRow, column: usize, count: usize) -> usize {
    column
        .saturating_add(
            table
                .cells
                .get(column)
                .map_or(1, |cell| cell.column_span.max(1)),
        )
        .min(count)
}

pub(super) fn span_width(widths: &[usize], start: usize, end: usize, gap: usize) -> usize {
    widths
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .sum::<usize>()
        .saturating_add(gap.saturating_mul(end.saturating_sub(start).saturating_sub(1)))
}

pub(super) fn render_table_columns(
    indent: usize,
    table: &LogicalTableRow,
    widths: &[usize],
    gap: usize,
    viewport: usize,
) -> Option<Vec<WrappedLine>> {
    let mut group = 0;
    let mut cells = table
        .cells
        .iter()
        .enumerate()
        .map(|(column, cell)| {
            if cell.occupancy != super::super::super::model::TableCellOccupancy::Content
                || column >= widths.len()
            {
                return Vec::new();
            }
            let area = span_width(widths, column, span_end(table, column, widths.len()), gap);
            let mut rows = wrap_table_cell(cell, area, 0, &mut group, true);
            if cell.has_open_tail()
                && let Some(tail) = rows.pop()
                && let Some(last) = rows.last_mut()
            {
                last.anchors.extend(tail.anchors);
            }
            rows
        })
        .collect::<Vec<_>>();
    let height = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
    // Missing physical slots are derived layout work, not authored content.
    // Preserve the prepared owners via the table-wide fallback before a tall
    // cell can expand every sparse neighboring column on each visual row.
    if height.saturating_mul(widths.len()) > 16_384 {
        return None;
    }
    if !position_cells(indent, table, widths, gap, viewport, &mut cells, height) {
        return None;
    }
    Some(
        (0..height)
            .map(|row| render_row(indent, table, widths, gap, &cells, row))
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
    let total = span_width(widths, 0, widths.len(), gap);
    let mut generated = vec![total; height];
    let mut column = 0;
    let mut start = 0;
    while column < widths.len() {
        let end = span_end(table, column, widths.len());
        let area = span_width(widths, column, end, gap);
        let owner_area = if end == widths.len() {
            viewport.saturating_sub(indent.saturating_add(start))
        } else {
            area
        };
        if let Some(rows) = cells.get_mut(column) {
            for (line, row) in rows.iter_mut().enumerate() {
                let before = super::super::super::inline::spans_width(&row.line.spans);
                let owner = row.alignment_padding(owner_area);
                let free = area.saturating_sub(before);
                let offset = owner.unwrap_or_else(|| match table.cells[column].alignment {
                    TableAlignment::Left => 0,
                    TableAlignment::Center => free / 2,
                    TableAlignment::Right => free,
                });
                generated[line] = generated[line]
                    .saturating_sub(area)
                    .saturating_add(free.max(offset));
                if offset > mant_ir::geometry::MAX_COLUMN_PADDING
                    || !table.cells[column]
                        .origin_bounds
                        .fits_at(mant_ir::geometry::coordinate(
                            indent.saturating_add(start).saturating_add(offset),
                        ))
                {
                    return false;
                }
                if owner.is_some() {
                    row.position_alignment(owner_area);
                } else {
                    super::translate(row, offset);
                }
            }
        }
        start = start.saturating_add(area).saturating_add(gap);
        column = end;
    }
    generated
        .iter()
        .all(|padding| *padding <= mant_ir::geometry::MAX_COLUMN_PADDING)
}

fn append_cell(output: &mut WrappedLine, row: &WrappedLine, column: usize) {
    output.anchors.extend(row.anchors.iter().cloned());
    let used = super::super::super::inline::spans_width(&row.line.spans);
    output.line.spans.extend(row.line.spans.clone());
    output.copy_map.append_shifted(&row.copy_map, column, used);
    output
        .links
        .extend(row.links.iter().map(|link| WrappedLink {
            target: link.target.clone(),
            start_column: column + link.start_column,
            end_column: column + link.end_column,
        }));
    output
        .search_cells
        .extend(row.search_cells.iter().map(|cell| WrappedSearchCell {
            group: cell.group,
            join_before: cell.join_before,
            character: cell.character,
            start_column: column + cell.start_column,
            end_column: column + cell.end_column,
        }));
}

fn render_row(
    indent: usize,
    table: &LogicalTableRow,
    widths: &[usize],
    gap: usize,
    cells: &[Vec<WrappedLine>],
    row_index: usize,
) -> WrappedLine {
    let mut output = WrappedLine {
        source_end: None,
        anchors: Vec::new(),
        line: Line::default(),
        links: Vec::new(),
        search_cells: Vec::new(),
        copy_map: RowCopyMap::default(),
    };
    if indent > 0 {
        output.line.spans.push(Span::raw(" ".repeat(indent)));
    }
    let mut column = 0;
    let mut start = indent;
    let mut any_physical = false;
    let mut all_skipped = true;
    while column < widths.len() {
        let end = span_end(table, column, widths.len());
        let area = span_width(widths, column, end, gap);
        let mut used = 0;
        if let Some(row) = cells.get(column).and_then(|rows| rows.get(row_index)) {
            used = super::super::super::inline::spans_width(&row.line.spans);
            any_physical |= row.source_end.is_some()
                || row.line.spans.iter().any(|span| !span.content.is_empty());
            all_skipped &= row.copy_map.skip_row;
            append_cell(&mut output, row, start);
        }
        if row_index == 0
            && cells.get(column).is_none_or(Vec::is_empty)
            && let Some(cell) = table.cells.get(column)
        {
            output.anchors.extend(cell.anchors.keys().cloned());
        }
        output
            .line
            .spans
            .push(Span::raw(" ".repeat(area.saturating_sub(used))));
        start = start.saturating_add(area);
        if end < widths.len() {
            if table.layout.rails.columns {
                let base = gap.saturating_sub(1);
                let left = base / 2;
                output.line.spans.push(Span::raw(" ".repeat(left)));
                let style = super::borders::column_style(table, end, widths.len());
                output.line.spans.push(super::borders::vertical_span(style));
                output.copy_map.omitted.push(start + left..start + left + 1);
                output.line.spans.push(Span::raw(" ".repeat(base - left)));
            } else {
                output.line.spans.push(Span::raw(" ".repeat(gap)));
            }
            start = start.saturating_add(gap);
        }
        column = end;
    }
    if !any_physical && !table.layout.rails.columns {
        output.line.spans.clear();
    }
    output.copy_map.skip_row = any_physical && all_skipped;
    output
}

pub(super) fn table_column_widths(
    preferred: &[usize],
    available: usize,
    gap: usize,
) -> Option<Vec<usize>> {
    mant_ir::geometry::allocate_table_columns(preferred, available, gap)
}
