//! Bounded table columns and stacked fallback, preserving cell-local payload.
use super::{
    Line, LogicalTableCell, LogicalTableLayout, LogicalTableRow, RowCopyMap, Span, TableAlignment,
    WrappedLine, WrappedLink, WrappedSearchCell, wrap_line_with_links,
};
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
    if table.layout.requires_stack(&table.cells) {
        return stack_table_cells(indent, table, width);
    }
    if !table.layout.column_preferences.widths.is_empty() {
        return render_declared_columns(indent, table, width, available);
    }
    let Some(column_widths) = table_column_widths(
        &table.layout.preferred_widths,
        available,
        usize::from(table.layout.column_preferences.gap_columns),
    ) else {
        return stack_table_cells(indent, table, width);
    };
    render_table_columns(
        indent,
        table,
        &column_widths,
        usize::from(table.layout.column_preferences.gap_columns),
        width,
    )
}

/// The same measured origins and sequential cell rows as plain terminal
/// output, while links/search/anchors travel with their original glyphs.
fn render_declared_columns(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
    available: usize,
) -> Vec<WrappedLine> {
    let Some(columns) = mant_ir::geometry::DeclaredColumns::new(&table.layout.column_preferences)
    else {
        return stack_table_cells(indent, table, width);
    };
    let preferred_start = |index: usize| {
        let start = columns.start(index);
        table
            .layout
            .column_preferences
            .advance_limit_columns
            .map_or(start, |limit| {
                start.min(usize::from(limit).saturating_mul(index))
            })
    };
    if table.cells.len() > mant_ir::geometry::MAX_DECLARED_COLUMNS
        || table
            .cells
            .iter()
            .enumerate()
            .any(|(i, _)| preferred_start(i) >= available)
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
                available.saturating_sub(preferred_start(index)).max(1),
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
    let Some(placements) = columns.place_at(&widths, mant_ir::geometry::coordinate(indent)) else {
        return stack_table_cells(indent, table, width);
    };
    let bounds = table
        .cells
        .iter()
        .map(|cell| cell.origin_bounds)
        .collect::<Vec<_>>();
    if !mant_ir::geometry::table_column_origins_fit(
        &bounds,
        &placements,
        mant_ir::geometry::coordinate(indent),
    ) {
        return stack_table_cells(indent, table, width);
    }
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
        .map(|pieces| placed_column_row(&pieces, &cells, &widths, indent))
        .collect()
}

fn placed_column_row(
    pieces: &[mant_ir::geometry::ColumnPiece],
    cells: &[Vec<WrappedLine>],
    widths: &[Vec<mant_ir::geometry::ColumnFieldWidth>],
    indent: usize,
) -> WrappedLine {
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
    if pieces.iter().all(|piece| {
        let row = &cells[piece.cell][piece.line];
        row.source_end.is_none() && row.line.spans.iter().all(|span| span.content.is_empty())
    }) {
        // Empty device rows have no advance; their table/parent origins
        // cannot become visible blank cells. Anchors still own this row.
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

fn render_layout_rule(
    indent: usize,
    rules: &[mant_ir::TableRuleCellKind],
    layout: &LogicalTableLayout,
    width: usize,
) -> Vec<WrappedLine> {
    let indent = super::readable_origins(indent, indent, width).0;
    let available = width.saturating_sub(indent).max(1);
    let gap = usize::from(layout.column_preferences.gap_columns);
    if gap.saturating_mul(rules.len().saturating_sub(1)) > mant_ir::geometry::MAX_COLUMN_PADDING {
        return rules
            .iter()
            .map(|rule| WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::raw(match rule {
                        mant_ir::TableRuleCellKind::Horizontal => "─",
                        mant_ir::TableRuleCellKind::DoubleHorizontal => "═",
                    }),
                ]),
                links: Vec::new(),
                search_cells: Vec::new(),
                copy_map: RowCopyMap::default(),
            })
            .collect();
    }
    let widths = table_column_widths(&layout.preferred_widths, available, gap)
        .filter(|widths| widths.len() == rules.len())
        .unwrap_or_else(|| {
            let base = available.saturating_sub(rules.len().saturating_sub(1) * gap);
            vec![(base / rules.len().max(1)).max(1); rules.len()]
        });
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    for (index, (rule, width)) in rules.iter().zip(widths).enumerate() {
        if index != 0 {
            spans.push(Span::raw(" ".repeat(gap)));
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
    let mut completed_tail = false;
    for cell in &table.cells {
        if cell.empty_boundary {
            pending_anchors.extend(cell.anchors.keys().cloned());
            if rows.is_empty() || completed_tail {
                rows.extend(wrap_table_cell(cell, width, indent, &mut next_group));
            }
            if let Some(last) = rows.last_mut() {
                last.anchors.append(&mut pending_anchors);
            }
            open_tail = false;
            completed_tail = true;
            continue;
        }
        if cell.lines.is_empty() && !cell.break_after {
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
        open_tail = cell.has_open_tail();
        completed_tail = cell.completed_tail;
    }
    // Content-derived projection retires one provisional final row before
    // passing its receipt outward. A work/viewport fallback changes columns,
    // not that tail policy; actual-origin/declared cells retain the open row.
    if open_tail
        && !table.layout.retain_open_tail
        && let Some(tail) = rows.pop()
    {
        pending_anchors.extend(tail.anchors);
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
            if row.source_end.is_none() && row.copy_map.end == Some(0) {
                // The cell cursor already proves an empty hard row. Retire
                // only its generated prefix, preserving authored spaces,
                // NBSP, and zero-width scalars with a source end.
                row.line.spans.clear();
            }
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
                .map(|row| super::super::inline::spans_width(&row.line.spans))
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

fn table_column_widths(
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
