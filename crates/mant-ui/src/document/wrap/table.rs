//! A table's data and rules share one viewport decision and final region.
use super::{Line, LogicalLine, LogicalTableRow, RowCopyMap, WrappedLine};

mod borders;
mod cells;
mod content;
mod declared;
mod rules;

use cells::stack_table_cells;
use content::{render_table_columns, table_column_widths};
use declared::render_declared_columns;

pub(super) fn render_table_row_with_links(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
) -> Vec<WrappedLine> {
    resolve_rows(indent, &[table], width)
        .pop()
        .unwrap_or_default()
}

pub(super) fn render_table_rows(lines: &[LogicalLine], width: usize) -> Vec<Vec<WrappedLine>> {
    let tables = lines
        .iter()
        .map(|line| line.table_row.as_ref().expect("table group"))
        .collect::<Vec<_>>();
    resolve_rows(lines[0].indent, &tables, width)
}

fn resolve_rows(indent: usize, tables: &[&LogicalTableRow], width: usize) -> Vec<Vec<WrappedLine>> {
    let layout = &tables[0].layout;
    let indent = super::readable_origins(indent, indent, width).0;
    let total_available = width.saturating_sub(indent).max(1);
    let show_outer = layout.rails.outer && total_available >= 5;
    let side = usize::from(show_outer) * 2;
    let available = total_available.saturating_sub(side * 2).max(1);
    let cell_indent = indent + side;
    let gap =
        usize::from(layout.column_preferences.gap_columns) + usize::from(layout.rails.columns);
    let natural = natural_width(layout);
    let explicit = layout.alignment.is_some();
    let area = if explicit {
        natural.min(available).max(1)
    } else {
        available
    };
    let widths = table_column_widths(&layout.preferred_widths, area, gap);
    let mut stacked = tables
        .iter()
        .any(|table| layout.requires_stack(&table.cells))
        || widths.is_none();
    let mut rendered = Vec::with_capacity(tables.len());
    if !stacked {
        for table in tables {
            let rows = if table.rules.is_some() {
                Some(Vec::new())
            } else if table.cells.is_empty() {
                Some(vec![empty_row()])
            } else if layout.column_preferences.widths.is_empty() {
                render_table_columns(
                    cell_indent,
                    table,
                    widths.as_deref().unwrap_or_default(),
                    gap,
                    cell_indent.saturating_add(area),
                )
            } else {
                render_declared_columns(cell_indent, table, area)
            };
            let Some(rows) = rows else {
                stacked = true;
                break;
            };
            rendered.push(rows);
        }
    }
    if stacked {
        rendered = tables
            .iter()
            .map(|table| {
                if table.rules.is_some() {
                    Vec::new()
                } else if table.cells.is_empty() {
                    vec![empty_row()]
                } else {
                    stack_table_cells(cell_indent, table, width.saturating_sub(side))
                }
            })
            .collect();
    }
    // Measure the prepared output, never source text or theme padding. Explicit
    // frames also retain unoccupied logical columns. A stacked fallback has no
    // rectangular column grid and uses only its actual source-order extent.
    let extent = rendered_extent(
        &rendered,
        cell_indent,
        if stacked { None } else { widths.as_deref() },
        gap,
    );
    // Sequential declared fields have no parallel base-column grid. Decorate
    // their actual final extent rather than the unrelated preferred widths.
    let grid_widths = (!stacked && layout.column_preferences.widths.is_empty())
        .then_some(widths.as_deref())
        .flatten();
    for (table, rows) in tables.iter().zip(&mut rendered) {
        if let Some(kinds) = &table.rules {
            *rows = rules::render_rules(
                cell_indent,
                table.whole_rule,
                kinds,
                grid_widths,
                gap,
                extent,
            );
        }
    }
    borders::decorate(
        tables,
        &mut rendered,
        indent,
        grid_widths,
        gap,
        extent,
        show_outer,
    );
    if !stacked {
        let offset = table_offset(layout.alignment, total_available, extent + side * 2, indent);
        for row in rendered.iter_mut().flatten() {
            translate(row, offset);
        }
    }
    rendered
}

fn rendered_extent(
    rows: &[Vec<WrappedLine>],
    indent: usize,
    widths: Option<&[usize]>,
    gap: usize,
) -> usize {
    let occupied = rows
        .iter()
        .flatten()
        .map(|row| super::super::inline::spans_width(&row.line.spans).saturating_sub(indent))
        .max()
        .unwrap_or(0);
    occupied
        .max(widths.map_or(0, |widths| {
            widths
                .iter()
                .sum::<usize>()
                .saturating_add(gap.saturating_mul(widths.len().saturating_sub(1)))
        }))
        .max(1)
}

fn table_offset(
    alignment: Option<mant_ir::TableAlignment>,
    available: usize,
    extent: usize,
    indent: usize,
) -> usize {
    let offset = match alignment {
        Some(mant_ir::TableAlignment::Center) => available.saturating_sub(extent) / 2,
        Some(mant_ir::TableAlignment::Right) => available.saturating_sub(extent),
        Some(mant_ir::TableAlignment::Left) | None => 0,
    };
    if offset.saturating_add(indent).saturating_add(extent) <= mant_ir::geometry::MAX_COLUMN_PADDING
    {
        offset
    } else {
        0
    }
}

fn natural_width(layout: &super::LogicalTableLayout) -> usize {
    let gap =
        usize::from(layout.column_preferences.gap_columns) + usize::from(layout.rails.columns);
    let widths = &layout.preferred_widths;
    if let Some(columns) = mant_ir::geometry::DeclaredColumns::new(&layout.column_preferences) {
        let count = widths.len().max(layout.column_preferences.widths.len());
        (0..count)
            .map(|index| {
                columns
                    .start(index)
                    .saturating_add(usize::from(layout.rails.columns).saturating_mul(index))
                    .saturating_add(
                        widths.get(index).copied().unwrap_or(1).max(
                            layout
                                .column_preferences
                                .widths
                                .get(index)
                                .copied()
                                .map_or(1, usize::from),
                        ),
                    )
            })
            .max()
            .unwrap_or(1)
    } else {
        widths
            .iter()
            .sum::<usize>()
            .saturating_add(gap.saturating_mul(widths.len().saturating_sub(1)))
            .max(1)
    }
}

fn empty_row() -> WrappedLine {
    WrappedLine {
        source_end: None,
        anchors: Vec::new(),
        line: Line::default(),
        links: Vec::new(),
        search_cells: Vec::new(),
        copy_map: RowCopyMap::default(),
    }
}

#[derive(Clone, Copy)]
enum PrefixOwnership {
    Structural,
    Decoration,
}

fn translate(row: &mut WrappedLine, offset: usize) {
    place_row(row, offset, PrefixOwnership::Decoration);
}

/// All coordinate maps move with the glyphs. Structural reading origins
/// remain copyable when authored cells exist; theme/frame/alignment padding
/// does not. Empty hard rows retain their explicit zero-content copy end.
fn place_row(row: &mut WrappedLine, offset: usize, ownership: PrefixOwnership) {
    if offset == 0 || row.line.spans.iter().all(|span| span.content.is_empty()) {
        return;
    }
    let original_width = super::super::inline::spans_width(&row.line.spans);
    row.line
        .spans
        .insert(0, super::Span::raw(" ".repeat(offset)));
    for link in &mut row.links {
        link.start_column += offset;
        link.end_column += offset;
    }
    for cell in &mut row.search_cells {
        cell.start_column += offset;
        cell.end_column += offset;
    }
    let mut map = RowCopyMap {
        skip_row: row.copy_map.skip_row,
        ..RowCopyMap::default()
    };
    if matches!(ownership, PrefixOwnership::Decoration) {
        map.omitted.push(0..offset);
    }
    map.append_shifted(&row.copy_map, offset, original_width);
    row.copy_map = map;
}
