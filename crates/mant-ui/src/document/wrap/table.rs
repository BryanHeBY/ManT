//! Bounded table columns and stacked fallback, preserving cell-local payload.
use super::{
    Line, LogicalTableCell, LogicalTableLayout, LogicalTableRow, Span, TableAlignment, WrappedLine,
    WrappedLink, WrappedSearchCell, wrap_line_with_links,
};
const TABLE_COLUMN_GAP: usize = 2;
const MAX_ALIGNMENT_PASSES: usize = 16;

struct AlignedTableCell {
    rows: Vec<AlignedTableRow>,
}

struct AlignedTableRow {
    row: WrappedLine,
    left_padding: usize,
}

struct ResolvedAlignedLine {
    line: super::LogicalLine,
    /// Alignment origin already used to resolve conditional placement.
    ///
    /// The first resolved definition row is one formatter field: choosing its
    /// Fit/Stacked branch and positioning that field must be one transaction.
    /// Later logical rows remain independent table fields.
    committed_padding: Option<usize>,
}
pub(super) fn render_table_row_with_links(
    indent: usize,
    geometry_offset: usize,
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
        }];
    }
    let indent = super::readable_origins(indent, indent, width).0;
    let available = width.saturating_sub(indent).max(1);
    if table.layout.force_stack {
        return stack_table_cells(indent, geometry_offset, table, width);
    }
    let Some(column_widths) = table_column_widths(&table.layout.preferred_widths, available) else {
        return stack_table_cells(indent, geometry_offset, table, width);
    };
    render_table_columns(indent, geometry_offset, table, &column_widths)
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
    }]
}

fn stack_table_cells(
    indent: usize,
    geometry_offset: usize,
    table: &LogicalTableRow,
    width: usize,
) -> Vec<WrappedLine> {
    let mut next_group = 0;
    let mut rows = table
        .cells
        .iter()
        .flat_map(|cell| {
            align_table_cell(cell, width, indent, geometry_offset, &mut next_group)
                .rows
                .into_iter()
                .map(|aligned| pad_wrapped_row(aligned.row, aligned.left_padding))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(WrappedLine {
            source_end: None,
            anchors: Vec::new(),
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
        });
    }
    rows
}

fn render_table_columns(
    indent: usize,
    geometry_offset: usize,
    table: &LogicalTableRow,
    column_widths: &[usize],
) -> Vec<WrappedLine> {
    let mut next_search_group = 0;
    let mut cell_origin = geometry_offset.saturating_add(indent);
    let rendered_cells = table
        .cells
        .iter()
        .zip(column_widths)
        .map(|(cell, column_width)| {
            let origin = cell_origin;
            cell_origin = cell_origin
                .saturating_add(*column_width)
                .saturating_add(TABLE_COLUMN_GAP);
            if *column_width == 0 {
                return AlignedTableCell { rows: Vec::new() };
            }
            align_table_cell(cell, *column_width, 0, origin, &mut next_search_group)
        })
        .collect::<Vec<_>>();
    let row_count = rendered_cells
        .iter()
        .map(|cell| cell.rows.len())
        .max()
        .unwrap_or(0)
        .max(1);

    (0..row_count)
        .map(|row_index| {
            let mut anchors = Vec::new();
            let mut spans = Vec::new();
            let mut links = Vec::new();
            let mut search_cells = Vec::new();
            let mut column_offset = indent;
            if indent > 0 {
                spans.push(Span::raw(" ".repeat(indent)));
            }
            for (column, column_width) in column_widths.iter().enumerate() {
                let rendered_cell = rendered_cells.get(column);
                let mut used = 0;
                let aligned = rendered_cell.and_then(|cell| cell.rows.get(row_index));
                let left_padding = aligned.map_or(0, |row| row.left_padding);
                if let Some(aligned) = aligned {
                    let row = &aligned.row;
                    anchors.extend(row.anchors.iter().cloned());
                    used = super::super::inline::spans_width(&row.line.spans);
                    if left_padding > 0 {
                        spans.push(Span::raw(" ".repeat(left_padding)));
                    }
                    spans.extend(row.line.spans.clone());
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
            }
        })
        .collect()
}

/// Resolve alignment together with the content that depends on its absolute
/// origin. Pinned CVS `term_flushln()` measures the field, applies
/// `TERMP_CENTER`/`TERMP_RIGHT`, and then commits the same field. Tabs and
/// conditional definition placement therefore cannot be resolved before the
/// alignment offset is known.
fn align_table_cell(
    cell: &LogicalTableCell,
    width: usize,
    render_indent: usize,
    origin: usize,
    next_search_group: &mut usize,
) -> AlignedTableCell {
    let mut rows = Vec::new();
    let mut logical_rows = Vec::with_capacity(cell.lines.len());
    for line in &cell.lines {
        logical_rows.push(rows.len());
        let mut line = line.clone();
        line.shift_origin(render_indent);
        for resolved in resolve_conditional_alignment(&line, width, origin, cell.alignment) {
            rows.extend(align_logical_line(
                &resolved.line,
                width,
                origin,
                cell.alignment,
                resolved.committed_padding,
                next_search_group,
            ));
        }
    }
    if rows.is_empty() && !cell.anchors.is_empty() {
        rows.push(AlignedTableRow {
            row: WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: Line::default(),
                links: Vec::new(),
                search_cells: Vec::new(),
            },
            left_padding: 0,
        });
    }
    for (id, logical) in &cell.anchors {
        let row = logical_rows
            .get(*logical)
            .copied()
            .unwrap_or(rows.len().saturating_sub(1));
        if let Some(row) = rows.get_mut(row) {
            row.row.anchors.push(id.clone());
        }
    }
    AlignedTableCell { rows }
}

fn resolve_conditional_alignment(
    line: &super::LogicalLine,
    width: usize,
    origin: usize,
    alignment: TableAlignment,
) -> Vec<ResolvedAlignedLine> {
    if line.conditional_definition.is_none() || alignment == TableAlignment::Left {
        let mut line = line.clone();
        line.shift_geometry_origin(origin);
        return line
            .resolved_lines(width)
            .iter()
            .cloned()
            .map(|mut resolved| {
                resolved.geometry_offset = 0;
                ResolvedAlignedLine {
                    line: resolved,
                    committed_padding: None,
                }
            })
            .collect();
    }
    let padding = stable_alignment_padding(width, alignment, |padding| {
        let mut line = line.clone();
        line.shift_geometry_origin(origin.saturating_add(padding));
        line.resolved_lines(width.saturating_sub(padding))
            .iter()
            .find_map(|resolved| {
                // Pinned CVS `term_flushln()` calls `term_fill()` against the
                // whole table field before adding CENTER/RIGHT indentation.
                // Measure the selected first field at that width as well;
                // shrinking it by the candidate padding here would make line
                // wrapping feed back into alignment and collapse the field.
                wrap_line_with_links(resolved, width)
                    .into_iter()
                    .next()
                    .map(|row| super::super::inline::spans_width(&row.line.spans))
            })
            .unwrap_or_default()
    });
    let mut line = line.clone();
    line.shift_geometry_origin(origin.saturating_add(padding));
    line.resolved_lines(width.saturating_sub(padding))
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, mut resolved)| {
            resolved.geometry_offset = 0;
            ResolvedAlignedLine {
                line: resolved,
                committed_padding: (index == 0).then_some(padding),
            }
        })
        .collect()
}

fn align_logical_line(
    line: &super::LogicalLine,
    width: usize,
    origin: usize,
    alignment: TableAlignment,
    committed_padding: Option<usize>,
    next_search_group: &mut usize,
) -> Vec<AlignedTableRow> {
    let padding = committed_padding.unwrap_or_else(|| {
        stable_alignment_padding(width, alignment, |padding| {
            let mut line = line.clone();
            line.shift_geometry_origin(origin.saturating_add(padding));
            wrap_line_with_links(&line, width.saturating_sub(padding))
                .iter()
                .map(|row| super::super::inline::spans_width(&row.line.spans))
                .max()
                .unwrap_or_default()
        })
    });
    let mut line = line.clone();
    line.shift_geometry_origin(origin.saturating_add(padding));
    let mut wrapped = wrap_line_with_links(&line, width.saturating_sub(padding));
    for row in &mut wrapped {
        for search_cell in &mut row.search_cells {
            search_cell.group = *next_search_group;
        }
    }
    *next_search_group += 1;
    let has_tab = line.spans.iter().any(|span| span.content.contains('\t'));
    wrapped
        .into_iter()
        .map(|row| {
            let used = super::super::inline::spans_width(&row.line.spans);
            AlignedTableRow {
                row,
                left_padding: if committed_padding.is_some() || has_tab {
                    padding
                } else {
                    alignment_padding(width, used, alignment).max(padding)
                },
            }
        })
        .collect()
}

fn stable_alignment_padding(
    width: usize,
    alignment: TableAlignment,
    mut used_at: impl FnMut(usize) -> usize,
) -> usize {
    let mut padding = 0;
    let mut seen = Vec::new();
    for _ in 0..MAX_ALIGNMENT_PASSES {
        let next = alignment_padding(width, used_at(padding), alignment);
        if next == padding {
            return padding;
        }
        if seen.contains(&next) {
            return padding.min(next);
        }
        seen.push(padding);
        padding = next.min(width);
    }
    padding
}

fn alignment_padding(width: usize, used: usize, alignment: TableAlignment) -> usize {
    let free = width.saturating_sub(used);
    match alignment {
        TableAlignment::Left => 0,
        TableAlignment::Center => free / 2,
        TableAlignment::Right => free,
    }
}

fn pad_wrapped_row(mut row: WrappedLine, padding: usize) -> WrappedLine {
    if padding == 0 {
        return row;
    }
    row.line.spans.insert(0, Span::raw(" ".repeat(padding)));
    for link in &mut row.links {
        link.start_column = link.start_column.saturating_add(padding);
        link.end_column = link.end_column.saturating_add(padding);
    }
    for cell in &mut row.search_cells {
        cell.start_column = cell.start_column.saturating_add(padding);
        cell.end_column = cell.end_column.saturating_add(padding);
    }
    row
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
