//! Cell wrapping and stacked rows preserve open and completed tail receipts.
use super::super::{
    Line, LogicalTableCell, LogicalTableRow, RowCopyMap, WrappedLine, wrap_line_with_links,
};

pub(super) fn stack_table_cells(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
) -> Vec<WrappedLine> {
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
pub(super) fn wrap_table_cell(
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
        if line.surface == super::super::super::LineSurface::Code && line.table_row.is_none() {
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
