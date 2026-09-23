//! Bounded table columns and stacked fallback, preserving cell-local payload.
use super::{
    Line, LogicalTableCell, LogicalTableLayout, LogicalTableRow, Span, TableAlignment, WrappedLine,
    WrappedLink, WrappedSearchCell, wrap_line_with_links,
};
use mant_ir::ContentPointKey;
const TABLE_COLUMN_GAP: usize = 2;
pub(super) fn render_table_row_with_links(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
) -> Vec<WrappedLine> {
    if let Some(rules) = &table.rules {
        return render_layout_rule(indent, rules, &table.rule_points, &table.layout, width);
    }
    if table.cells.is_empty() {
        return vec![WrappedLine {
            source_end: None,
            anchors: Vec::new(),
            cell_points: Vec::new(),
            points: Vec::new(),
            fixed_lines: Vec::new(),
            fixed_points: Vec::new(),
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
        }];
    }
    let indent = super::readable_origins(indent, indent, width).0;
    let available = width.saturating_sub(indent).max(1);
    if table.layout.force_stack {
        return stack_table_cells(indent, table, width);
    }
    let Some(column_widths) = table_column_widths(&table.layout.preferred_widths, available) else {
        return stack_table_cells(indent, table, width);
    };
    render_table_columns(indent, table, &column_widths)
}

fn render_layout_rule(
    indent: usize,
    rules: &[mant_ir::TableRuleCellKind],
    rule_points: &[Option<mant_ir::ContentPointKey>],
    layout: &LogicalTableLayout,
    width: usize,
) -> Vec<WrappedLine> {
    let indent = super::readable_origins(indent, indent, width).0;
    let available = width.saturating_sub(indent).max(1);
    let widths = table_column_widths(&layout.preferred_widths, available)
        .filter(|widths| widths.len() == rules.len())
        .unwrap_or_else(|| {
            let base = available.saturating_sub(rules.len().saturating_sub(1) * TABLE_COLUMN_GAP);
            vec![(base / rules.len().max(1)).max(1); rules.len()]
        });
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    let mut cell_points = Vec::new();
    let mut column = indent;
    for (index, (rule, width)) in rules.iter().zip(widths).enumerate() {
        if index != 0 {
            spans.push(Span::raw(" ".repeat(TABLE_COLUMN_GAP)));
            column += TABLE_COLUMN_GAP;
        }
        if let Some(point) = rule_points.get(index).copied().flatten() {
            cell_points.push((point, column));
        }
        let glyph = match rule {
            mant_ir::TableRuleCellKind::Horizontal => '─',
            mant_ir::TableRuleCellKind::DoubleHorizontal => '═',
        };
        spans.push(Span::raw(glyph.to_string().repeat(width)));
        column += width;
    }
    vec![WrappedLine {
        source_end: None,
        anchors: Vec::new(),
        cell_points,
        points: Vec::new(),
        fixed_lines: Vec::new(),
        fixed_points: Vec::new(),
        line: Line::from(spans),
        links: Vec::new(),
        search_cells: Vec::new(),
    }]
}

fn stack_table_cells(indent: usize, table: &LogicalTableRow, width: usize) -> Vec<WrappedLine> {
    let mut next_group = 0;
    let mut rows = table
        .cells
        .iter()
        .flat_map(|cell| wrap_table_cell(cell, width, indent, &mut next_group))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(WrappedLine {
            source_end: None,
            anchors: Vec::new(),
            cell_points: Vec::new(),
            points: Vec::new(),
            fixed_lines: Vec::new(),
            fixed_points: Vec::new(),
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
        });
    }
    rows
}

/// Preserve child-local coordinates until the actual cell width is known.
/// The resulting anchors travel with visual rows through columns, stacking
/// and nested tables, just like links and search coordinates.
fn wrap_table_cell(
    cell: &LogicalTableCell,
    width: usize,
    indent: usize,
    next_group: &mut usize,
) -> Vec<WrappedLine> {
    let mut rendered = Vec::new();
    let mut logical_rows = Vec::with_capacity(cell.lines.len());
    for line in &cell.lines {
        logical_rows.push(rendered.len());
        let mut line = line.clone();
        line.indent = line.indent.saturating_add(indent);
        line.continuation_indent = line.continuation_indent.saturating_add(indent);
        let mut wrapped = wrap_line_with_links(&line, width);
        for row in &mut wrapped {
            // The inner table's boundaries are payload of this outer cell.
            // Only this cell's own point remains pinned to its column start.
            row.points.append(&mut row.cell_points);
            for search_cell in &mut row.search_cells {
                search_cell.group = *next_group;
            }
        }
        *next_group += 1;
        rendered.extend(wrapped);
    }
    if rendered.is_empty() && (!cell.anchors.is_empty() || cell.point.is_some()) {
        rendered.push(WrappedLine {
            source_end: None,
            anchors: Vec::new(),
            cell_points: Vec::new(),
            points: Vec::new(),
            fixed_lines: Vec::new(),
            fixed_points: Vec::new(),
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
        });
    }
    if let Some(point) = cell.point
        && let Some(first) = rendered.first_mut()
    {
        first.cell_points.push((point, indent));
    }
    for (id, logical) in &cell.anchors {
        let row = logical_rows
            .get(*logical)
            .copied()
            .unwrap_or(rendered.len().saturating_sub(1));
        if let Some(row) = rendered.get_mut(row) {
            row.anchors.push(id.clone());
        }
    }
    for (key, logical) in &cell.fixed_line_rows {
        if let Some(row) = logical_rows
            .get(*logical)
            .and_then(|index| rendered.get_mut(*index))
        {
            row.fixed_lines.push(*key);
        }
    }
    for (point, logical, column) in &cell.fixed_point_locations {
        if let Some(row) = logical_rows
            .get(*logical)
            .and_then(|index| rendered.get_mut(*index))
        {
            row.fixed_points.push((*point, indent + column));
        }
    }
    for (point, logical, column) in &cell.stacked_point_locations {
        if let Some(row) = logical_rows
            .get(*logical)
            .and_then(|index| rendered.get_mut(*index))
        {
            let column = super::readable_origins(indent + column, indent + column, width).0;
            row.points.push((*point, column));
        }
    }
    rendered
}

fn collect_column_points(
    cell_points: &mut Vec<(ContentPointKey, usize)>,
    points: &mut Vec<(ContentPointKey, usize)>,
    row: &WrappedLine,
    column_offset: usize,
    left_padding: usize,
) {
    // The current cell's point stays on the column boundary. Inner content
    // points move with the visible text under right/centre alignment.
    cell_points.extend(
        row.cell_points
            .iter()
            .map(|(point, local)| (*point, column_offset + local)),
    );
    points.extend(
        row.points
            .iter()
            .map(|(point, local)| (*point, column_offset + left_padding + local)),
    );
}

fn rendered_table_rows(cells: &[Vec<WrappedLine>]) -> usize {
    cells.iter().map(Vec::len).max().unwrap_or(0).max(1)
}

fn render_table_columns(
    indent: usize,
    table: &LogicalTableRow,
    column_widths: &[usize],
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
            wrap_table_cell(cell, *column_width, 0, &mut next_search_group)
        })
        .collect::<Vec<_>>();
    let row_count = rendered_table_rows(&rendered_cells);

    (0..row_count)
        .map(|row_index| {
            let mut anchors = Vec::new();
            let mut cell_points = Vec::new();
            let mut points = Vec::new();
            let mut fixed_lines = Vec::new();
            let mut fixed_points = Vec::new();
            let mut spans = Vec::new();
            let mut links = Vec::new();
            let mut search_cells = Vec::new();
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
                    used = super::super::inline::spans_width(&row.line.spans);
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
                    links.extend(row.links.iter().map(|link| WrappedLink {
                        identity: link.identity,
                        start_column: column_offset + left_padding + link.start_column,
                        end_column: column_offset + left_padding + link.end_column,
                    }));
                    collect_column_points(
                        &mut cell_points,
                        &mut points,
                        row,
                        column_offset,
                        left_padding,
                    );
                    fixed_lines.extend(row.fixed_lines.iter().copied());
                    fixed_points.extend(
                        row.fixed_points
                            .iter()
                            .map(|(point, local)| (*point, column_offset + left_padding + local)),
                    );
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
                    spans.push(Span::raw(" ".repeat(TABLE_COLUMN_GAP)));
                    column_offset += TABLE_COLUMN_GAP;
                }
            }
            WrappedLine {
                source_end: None,
                anchors,
                cell_points,
                points,
                fixed_lines,
                fixed_points,
                line: Line::from(spans),
                links,
                search_cells,
            }
        })
        .collect()
}

fn table_column_widths(preferred_widths: &[usize], available: usize) -> Option<Vec<usize>> {
    const SOFT_MINIMUM: usize = 8;

    if preferred_widths.is_empty() {
        return Some(Vec::new());
    }
    let gaps = preferred_widths.len().saturating_sub(1) * TABLE_COLUMN_GAP;
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
