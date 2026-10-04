//! Bounded table columns and stacked fallback, preserving cell-local payload.
use super::{
    Line, LogicalTableCell, LogicalTableLayout, LogicalTableRow, RowCopyMap, Span, TableAlignment,
    WrappedLine, WrappedLink, WrappedSearchCell, wrap_line_with_links,
};
const TABLE_COLUMN_GAP: usize = 2;
pub(super) fn render_table_row_with_links(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
) -> Vec<WrappedLine> {
    if let Some(rules) = &table.rules {
        return render_layout_rule(indent, rules, &table.layout, width);
    }
    if table.cells.is_empty() {
        return vec![WrappedLine {
            source_end: None,
            anchors: Vec::new(),
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
            copy_map: RowCopyMap::default(),
        }];
    }
    let indent = super::readable_origins(indent, indent, width).0;
    let available = width.saturating_sub(indent).max(1);
    if table.layout.force_stack {
        return stack_table_cells(indent, table, width);
    }
    if !table.layout.declared_widths.is_empty() {
        return render_declared_columns(indent, table, width, available);
    }
    let Some(column_widths) = table_column_widths(&table.layout.preferred_widths, available) else {
        return stack_table_cells(indent, table, width);
    };
    render_table_columns(indent, table, &column_widths)
}

/// The same measured origins and sequential cell rows as plain terminal
/// output, while links/search/anchors travel with their original glyphs.
fn render_declared_columns(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
    available: usize,
) -> Vec<WrappedLine> {
    let Some(columns) = mant_ir::geometry::DeclaredColumns::new(&table.layout.declared_widths)
    else {
        return stack_table_cells(indent, table, width);
    };
    if table.cells.len() > mant_ir::geometry::MAX_DECLARED_COLUMNS
        || table
            .cells
            .iter()
            .enumerate()
            .any(|(i, _)| columns.start(i) >= available)
    {
        return stack_table_cells(indent, table, width);
    }
    let mut group = 0;
    let cells = table
        .cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            wrap_table_cell(
                cell,
                available.saturating_sub(columns.start(index)).max(1),
                0,
                &mut group,
            )
        })
        .collect::<Vec<_>>();
    let widths = cells
        .iter()
        .zip(&table.cells)
        .map(|(rows, cell)| {
            rows.iter()
                .enumerate()
                .map(|(index, row)| mant_ir::geometry::ColumnFieldWidth {
                    content: mant_ir::geometry::declared_field_width(&row.line.to_string()),
                    output: super::super::inline::spans_width(&row.line.spans),
                    completed: cell.completed_tail && index + 1 == rows.len(),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let Some(placements) = columns.place(&widths) else {
        return stack_table_cells(indent, table, width);
    };
    if placements.iter().flatten().any(|piece| {
        piece
            .column
            .saturating_add(widths[piece.cell][piece.line].output)
            > available
    }) {
        return stack_table_cells(indent, table, width);
    }
    placements
        .into_iter()
        .map(|pieces| {
            let mut spans = vec![Span::raw(" ".repeat(indent))];
            let mut links = Vec::new();
            let mut search_cells = Vec::new();
            let mut anchors = Vec::new();
            let mut copy_map = RowCopyMap::default();
            let mut visible = 0_usize;
            for piece in pieces {
                let row = &cells[piece.cell][piece.line];
                spans.push(Span::raw(" ".repeat(piece.column.saturating_sub(visible))));
                spans.extend(row.line.spans.clone());
                let offset = indent.saturating_add(piece.column);
                copy_map.append_shifted(
                    &row.copy_map,
                    offset,
                    super::super::inline::spans_width(&row.line.spans),
                );
                links.extend(row.links.iter().map(|link| WrappedLink {
                    target: link.target.clone(),
                    start_column: offset.saturating_add(link.start_column),
                    end_column: offset.saturating_add(link.end_column),
                }));
                search_cells.extend(row.search_cells.iter().map(|cell| WrappedSearchCell {
                    group: cell.group,
                    join_before: cell.join_before,
                    character: cell.character,
                    start_column: offset.saturating_add(cell.start_column),
                    end_column: offset.saturating_add(cell.end_column),
                }));
                anchors.extend(row.anchors.iter().cloned());
                visible = piece
                    .column
                    .saturating_add(widths[piece.cell][piece.line].output);
            }
            WrappedLine {
                source_end: None,
                anchors,
                line: Line::from(spans),
                links,
                search_cells,
                copy_map,
            }
        })
        .collect()
}

fn render_layout_rule(
    indent: usize,
    rules: &[mant_ir::TableRuleCellKind],
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
    for (index, (rule, width)) in rules.iter().zip(widths).enumerate() {
        if index != 0 {
            spans.push(Span::raw(" ".repeat(TABLE_COLUMN_GAP)));
        }
        let glyph = match rule {
            mant_ir::TableRuleCellKind::Horizontal => '─',
            mant_ir::TableRuleCellKind::DoubleHorizontal => '═',
        };
        spans.push(Span::raw(glyph.to_string().repeat(width)));
    }
    vec![WrappedLine {
        source_end: None,
        anchors: Vec::new(),
        line: Line::from(spans),
        links: Vec::new(),
        search_cells: Vec::new(),
        copy_map: RowCopyMap::default(),
    }]
}

fn stack_table_cells(indent: usize, table: &LogicalTableRow, width: usize) -> Vec<WrappedLine> {
    let mut next_group = 0;
    let mut rows: Vec<WrappedLine> = Vec::new();
    let mut pending_anchors = Vec::new();
    let mut open_tail = false;
    for cell in &table.cells {
        if cell.lines.is_empty() {
            // No physical cell receipt was produced. Keep navigation without
            // applying the per-cell column-layout fallback to stacked rows.
            pending_anchors.extend(cell.anchors.keys().cloned());
            continue;
        }
        if open_tail && let Some(last) = rows.pop() {
            // An inline hard break opened the following physical row; it
            // did not complete a vertical-space row. Stacked fallback uses
            // that row for this next cell, just as the declared-column plan
            // does. Decide from the logical receipt before viewport padding,
            // never by trimming author-written whitespace from a rendered row.
            pending_anchors.extend(last.anchors);
        }
        let mut rendered = wrap_table_cell(cell, width, indent, &mut next_group);
        if let Some(first) = rendered.first_mut() {
            first.anchors.append(&mut pending_anchors);
        }
        rows.extend(rendered);
        open_tail = !cell.completed_tail
            && cell.lines.len() > 1
            && cell.lines.last().is_some_and(|line| {
                line.table_row.is_none() && line.spans.iter().all(|span| span.content.is_empty())
            });
    }
    if rows.is_empty() {
        rows.push(WrappedLine {
            source_end: None,
            anchors: Vec::new(),
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
            copy_map: RowCopyMap::default(),
        });
    }
    if let Some(last) = rows.last_mut() {
        last.anchors.append(&mut pending_anchors);
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
        if line.surface == super::super::LineSurface::Code && line.table_row.is_none() {
            // cells_to_line() appends exactly one renderer-owned
            // surface-fill span after every Code row. That viewport paint is
            // neither cell content nor a positioning cell; retire only this
            // known span, preserving every authored blank and glyph/style.
            for row in &mut wrapped {
                row.line.spans.pop();
            }
        }
        for row in &mut wrapped {
            for search_cell in &mut row.search_cells {
                search_cell.group = *next_group;
            }
        }
        *next_group += 1;
        rendered.extend(wrapped);
    }
    if rendered.is_empty() {
        // The parent omitted navigation-only rows before this point. An
        // actual data cell still owns a physical row even without glyphs.
        rendered.push(WrappedLine {
            source_end: None,
            anchors: Vec::new(),
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
            copy_map: RowCopyMap::default(),
        });
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
    rendered
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
    let row_count = rendered_cells
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(1);

    (0..row_count)
        .map(|row_index| {
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
                    spans.push(Span::raw(" ".repeat(TABLE_COLUMN_GAP)));
                    column_offset += TABLE_COLUMN_GAP;
                }
            }
            WrappedLine {
                source_end: None,
                anchors,
                line: Line::from(spans),
                links,
                search_cells,
                copy_map,
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
