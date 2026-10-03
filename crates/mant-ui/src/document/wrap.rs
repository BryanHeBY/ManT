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

mod cells;
mod table;
use cells::{
    StyledCell, fitting_prefix, styled_cells, tldr_decoration_width, trim_trailing_whitespace,
    wrapped_cells_to_line,
};
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

pub(super) struct WrappedLine {
    pub(super) source_end: Option<usize>,
    pub(super) anchors: Vec<String>,
    pub(super) line: Line<'static>,
    pub(super) links: Vec<WrappedLink>,
    pub(super) search_cells: Vec<WrappedSearchCell>,
}

#[derive(Clone, Copy)]
pub(super) struct WrappedSearchCell {
    pub(super) group: usize,
    pub(super) join_before: bool,
    pub(super) character: char,
    pub(super) start_column: usize,
    pub(super) end_column: usize,
}

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
    for mark in &line.reference_marks {
        let index = rows
            .iter()
            .position(|row| row.source_end.is_some_and(|end| mark.scalar_offset < end))
            .unwrap_or_else(|| rows.len().saturating_sub(1));
        if let Some(row) = rows.get_mut(index) {
            row.anchors.push(mark.id.to_string());
        }
    }
    rows
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
    let mut cells = styled_cells(line);

    if cells.is_empty() {
        return vec![wrapped_cells_to_line(line, width, first_indent, &[], false)];
    }

    let mut result = Vec::new();
    let mut first_row = true;
    let mut join_with_space = false;
    while !cells.is_empty() {
        let indent = if first_row {
            first_indent
        } else {
            continuation_indent
        };
        let available = width
            .saturating_sub(indent)
            .saturating_sub(decoration_width)
            .max(1);
        bound_first_grapheme(&mut cells, available);
        let fit = fitting_prefix(&cells, available);
        if fit == cells.len() {
            result.push(wrapped_cells_to_line(
                line,
                width,
                indent,
                &cells,
                join_with_space,
            ));
            break;
        }

        if line.wrap_mode == WrapMode::Character {
            result.push(wrapped_cells_to_line(
                line,
                width,
                indent,
                &cells[..fit],
                join_with_space,
            ));
            cells.drain(..fit);
            join_with_space = false;
        } else {
            join_with_space = wrap_word_row(
                line,
                width,
                indent,
                &mut cells,
                fit,
                join_with_space,
                &mut result,
            );
        }
        first_row = false;
    }
    result
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
            });
        }
        LineSurface::TldrBottom => {
            return Some(WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                line: panel_border(width, '└', '┘'),
                links: Vec::new(),
                search_cells: Vec::new(),
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

fn wrap_word_row(
    line: &LogicalLine,
    width: usize,
    indent: usize,
    cells: &mut Vec<StyledCell>,
    fit: usize,
    join_with_space: bool,
    result: &mut Vec<WrappedLine>,
) -> bool {
    let split = cells[..fit]
        .iter()
        .rposition(|cell| cell.grapheme_start && cell.whitespace)
        .filter(|position| *position > 0)
        .unwrap_or(fit);
    let row_end = trim_trailing_whitespace(cells, split);
    let emitted_row = row_end != 0;
    let mut removed_separator = if row_end == 0 {
        // A fitted prefix can contain both indentation and the next word.
        // Dropping an empty whitespace row consumes only its separator, not
        // the visible cells following it (term.c::term_fill keeps graph cells
        // after a break opportunity for the next accepted segment).
        let consumed = if split < fit { split + 1 } else { fit };
        cells.drain(..consumed.max(1));
        true
    } else {
        result.push(wrapped_cells_to_line(
            line,
            width,
            indent,
            &cells[..row_end],
            join_with_space,
        ));
        let removed_separator = split < fit;
        let consumed = if removed_separator { split + 1 } else { split };
        cells.drain(..consumed.max(1));
        removed_separator
    };
    while cells.first().is_some_and(|cell| cell.whitespace) {
        cells.remove(0);
        removed_separator = true;
    }
    if emitted_row {
        removed_separator
    } else {
        join_with_space || removed_separator
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
