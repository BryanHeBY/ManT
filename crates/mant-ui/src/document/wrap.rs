//! Converts logical document lines into exact terminal rows.
//!
//! Wrapping, table layout, link projection, and search-cell projection share
//! one width model here. Keeping them together prevents navigation, search,
//! and rendering from disagreeing about the columns visible in the terminal.

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use mant_ir::TableAlignment;

use super::{
    LineSurface, LinkTarget, LogicalLine, WrapMode,
    model::{LogicalTableCell, LogicalTableLayout, LogicalTableRow},
};
use crate::theme;

mod alignment;
mod cells;
mod links;
mod prepared;
mod table;
#[cfg(test)]
mod workload;
use cells::{StyledCell, tldr_decoration_width, wrapped_cells_to_line};
use mant_render::line_layout::{PreparedLine, WrapMode as PlanMode};
use prepared::styled_units;
use table::render_table_row_with_links;

/// Keep a useful reading area without changing the logical source origins.
/// Reduce both origins by the same displacement whenever indentation would
/// leave less than 16 cells (half the available width on a narrow viewport).
/// The one/two-cell fallback necessarily reserves the entire content area.
fn readable_origins(first: usize, continuation: usize, available: usize) -> (usize, usize) {
    let content = if available <= 2 {
        available
    } else {
        (available / 2).min(16)
    };
    let reduction = first
        .max(continuation)
        .saturating_sub(available.saturating_sub(content));
    (
        first.saturating_sub(reduction),
        continuation.saturating_sub(reduction),
    )
}

#[derive(Clone)]
pub(super) struct WrappedLine {
    pub(super) source_end: Option<usize>,
    pub(super) anchors: Vec<String>,
    pub(super) line: Line<'static>,
    pub(super) links: Vec<WrappedLink>,
    pub(super) search_cells: Vec<WrappedSearchCell>,
    pub(super) copy_map: RowCopyMap,
}

/// Display coordinates of generated hint padding and the original content end.
/// This map is derived from the same cells as links/search, never whitespace.
#[derive(Clone, Debug, Default)]
pub(super) struct RowCopyMap {
    alignment: Option<alignment::PreparedAlignment>,
    pub(super) omitted: Vec<std::ops::Range<usize>>,
    pub(super) end: Option<usize>,
    /// Original TAB expansion has significant width in declared fields;
    /// ordinary trailing ASCII separators remain pending instead.
    pub(super) tab_end: usize,
    /// A visual decoration row contributes no selection newline.
    pub(super) skip_row: bool,
}

impl RowCopyMap {
    pub(super) fn append_shifted(&mut self, other: &Self, offset: usize, width: usize) {
        if other.skip_row {
            // A decoration-only child can share a visual row with another
            // cell's body. Omit its own cells, rather than relying on the
            // combined row also being decoration-only.
            self.omitted.push(offset..offset.saturating_add(width));
            return;
        }
        self.omitted.extend(
            other
                .omitted
                .iter()
                .map(|range| offset.saturating_add(range.start)..offset.saturating_add(range.end)),
        );
        if other.tab_end != 0 {
            self.tab_end = self.tab_end.max(offset.saturating_add(other.tab_end));
        }
        let content_end = other.end.unwrap_or(width);
        if content_end == 0 {
            // An empty hard row cannot acquire copyable content from its
            // parent column's placement. Preserve the explicit empty end.
            self.end.get_or_insert(0);
            return;
        }
        let end = offset.saturating_add(content_end);
        self.end = Some(self.end.unwrap_or(0).max(end));
    }
}

#[derive(Clone, Copy)]
pub(super) struct WrappedSearchCell {
    pub(super) group: usize,
    pub(super) source_index: usize,
    pub(super) character: char,
    pub(super) start_column: usize,
    pub(super) end_column: usize,
}

#[derive(Clone)]
pub(super) struct WrappedLink {
    pub(super) target: LinkTarget,
    pub(super) start_column: usize,
    pub(super) end_column: usize,
}

#[cfg(test)]
pub(super) fn wrap_line(line: &LogicalLine, width: usize) -> Vec<Line<'static>> {
    wrap_line_with_links(line, width)
        .into_iter()
        .map(|wrapped| wrapped.line)
        .collect()
}

pub(super) fn wrap_line_with_links(line: &LogicalLine, width: usize) -> Vec<WrappedLine> {
    let mut rows = wrap_logical_line(line, width);
    attach_reference_marks(line, &mut rows);
    rows
}

/// Resolve each contiguous table at this viewport once. Every data and rule
/// row shares the same fallback, extent and translation; nested cells use the
/// same entry point without storing a second body or a viewport cache.
pub(super) fn wrap_lines_with_links(
    lines: &[LogicalLine],
    width: usize,
) -> impl Iterator<Item = Vec<WrappedLine>> + '_ {
    let mut start = 0;
    let mut pending = Vec::new().into_iter();
    std::iter::from_fn(move || {
        if let Some(rows) = pending.next() {
            return Some(rows);
        }
        let line = lines.get(start)?;
        let Some(table) = &line.table_row else {
            start += 1;
            return Some(wrap_line_with_links(line, width));
        };
        let end = start
            + lines[start..]
                .iter()
                .take_while(|other| {
                    other.indent == line.indent
                        && other
                            .table_row
                            .as_ref()
                            .is_some_and(|row| std::sync::Arc::ptr_eq(&row.layout, &table.layout))
                })
                .count();
        let mut rows = table::render_table_rows(&lines[start..end], width);
        for (line, rows) in lines[start..end].iter().zip(&mut rows) {
            attach_reference_marks(line, rows);
        }
        start = end;
        pending = rows.into_iter();
        pending.next()
    })
}

fn attach_reference_marks(line: &LogicalLine, rows: &mut [WrappedLine]) {
    for mark in &line.reference_marks {
        let index = rows
            .iter()
            .position(|row| row.source_end.is_some_and(|end| mark.scalar_offset < end))
            .or_else(|| rows.iter().rposition(|row| !row.copy_map.skip_row))
            .unwrap_or_else(|| rows.len().saturating_sub(1));
        if let Some(row) = rows.get_mut(index) {
            row.anchors.push(mark.id.to_string());
        }
    }
}

fn wrap_logical_line(line: &LogicalLine, width: usize) -> Vec<WrappedLine> {
    if let Some(table) = &line.table_row {
        return render_table_row_with_links(line.indent, table, width);
    }
    if let Some(row) = surface_row(line, width) {
        return vec![row];
    }

    let decoration_width = tldr_decoration_width(line, width);
    let (first_indent, continuation_indent) = readable_origins(
        line.indent,
        line.continuation_indent,
        width.saturating_sub(decoration_width),
    );
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let prepared = PreparedLine::new(&text, line.indent);
    let mut styled = styled_units(line, &prepared);
    let mode = match line.wrap_mode {
        WrapMode::Word => PlanMode::Word,
        WrapMode::Character => PlanMode::Character,
    };
    let plans = prepared.wrap(
        width.saturating_sub(decoration_width),
        first_indent,
        continuation_indent,
        mode,
    );
    let mut result = Vec::with_capacity(plans.len());
    for (index, plan) in plans.iter().enumerate() {
        let cells =
            &mut styled.cells[styled.offsets[plan.units.start]..styled.offsets[plan.units.end]];
        let available = width
            .saturating_sub(decoration_width)
            .saturating_sub(plan.origin)
            .max(1);
        bound_first_grapheme(cells, available);
        let layout_padding = if index == 0 {
            line.layout_padding
        } else {
            line.continuation_layout_padding
        };
        let mut row = wrapped_cells_to_line(line, width, plan.origin, layout_padding, cells);
        append_seam(&mut row, line, &prepared, plan.seam_after.clone());
        result.push(row);
    }
    result
}

/// Soft separators retain their exact source identity for logical search,
/// even when no terminal cell paints them. Their zero-width range belongs to
/// the preceding visual row; generated padding never enters this stream.
fn append_seam(
    row: &mut WrappedLine,
    line: &LogicalLine,
    prepared: &PreparedLine<'_>,
    seam: std::ops::Range<usize>,
) {
    if seam.is_empty() {
        return;
    }
    let start = prepared
        .units()
        .partition_point(|unit| unit.bytes.end <= seam.start);
    let Some(unit) = prepared.units().get(start) else {
        return;
    };
    let column = row.copy_map.end.unwrap_or(0);
    for (index, character) in prepared.source()[seam.clone()].chars().enumerate() {
        #[cfg(test)]
        workload::visit(1);
        let source_index = unit.scalars.start + index;
        if !line
            .layout_scalars
            .iter()
            .any(|range| range.contains(&source_index))
        {
            row.search_cells.push(WrappedSearchCell {
                group: 0,
                source_index,
                character,
                start_column: column,
                end_column: column,
            });
        }
    }
    // Reference anchors in an omitted separator still belong to this row.
    row.source_end = Some(unit.scalars.start + prepared.source()[seam].chars().count());
}

fn surface_row(line: &LogicalLine, width: usize) -> Option<WrappedLine> {
    match line.surface {
        LineSurface::TldrTop => {
            return Some(WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: panel_border(width, '┌', '┐'),
                links: Vec::new(),
                search_cells: Vec::new(),
                copy_map: RowCopyMap::default(),
            });
        }
        LineSurface::TldrBottom => {
            return Some(WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: panel_border(width, '└', '┘'),
                links: Vec::new(),
                search_cells: Vec::new(),
                copy_map: RowCopyMap::default(),
            });
        }
        LineSurface::Divider => {
            return Some(WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: Line::from(Span::styled(
                    "─".repeat(width),
                    theme::style(theme::StyleRole::Rule),
                )),
                links: Vec::new(),
                search_cells: Vec::new(),
                copy_map: RowCopyMap::default(),
            });
        }
        LineSurface::Rule => {
            let indent = readable_origins(line.indent, line.indent, width).0;
            return Some(WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(
                        "─".repeat(width.saturating_sub(indent)),
                        theme::style(theme::StyleRole::Rule),
                    ),
                ]),
                links: Vec::new(),
                search_cells: Vec::new(),
                copy_map: RowCopyMap::default(),
            });
        }
        LineSurface::DoubleRule => {
            let indent = readable_origins(line.indent, line.indent, width).0;
            return Some(WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(
                        "═".repeat(width.saturating_sub(indent)),
                        theme::style(theme::StyleRole::Rule),
                    ),
                ]),
                links: Vec::new(),
                search_cells: Vec::new(),
                copy_map: RowCopyMap::default(),
            });
        }
        LineSurface::Normal | LineSurface::Code | LineSurface::Tldr => {}
    }
    None
}

fn bound_first_grapheme(cells: &mut [StyledCell], available: usize) {
    if let Some(first) = cells.first_mut()
        && first.width > available
    {
        // A double-width glyph cannot occupy a one-column viewport.
        // Preserve its semantic search cell while rendering one bounded
        // replacement cell, just as control characters are sanitized.
        first.display_character = Some('\u{fffd}');
        first.width = 1;
        for cell in cells
            .iter_mut()
            .skip(1)
            .take_while(|cell| !cell.grapheme_start)
        {
            cell.display_character = None;
        }
    }
}

fn panel_border(width: usize, left: char, right: char) -> Line<'static> {
    let style = theme::style(theme::StyleRole::TldrSurface)
        .patch(theme::style(theme::StyleRole::TldrFrame));
    if width == 1 {
        return Line::from(Span::styled(left.to_string(), style));
    }
    Line::from(Span::styled(
        format!("{left}{}{right}", "─".repeat(width.saturating_sub(2))),
        style,
    ))
}
