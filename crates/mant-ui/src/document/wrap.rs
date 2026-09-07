//! Converts logical document lines into exact terminal rows.
//!
//! Wrapping, table layout, link projection, and search-cell projection share
//! one width model here. Keeping them together prevents navigation, search,
//! and rendering from disagreeing about the columns visible in the terminal.

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use mant_ir::TableAlignment;

use super::{
    LineSurface, LinkTarget, LogicalLine, WrapMode,
    model::{LogicalTableCell, LogicalTableRow},
};
use crate::theme;

mod cells;
mod table;
use cells::{
    fitting_prefix, styled_cells, tldr_decoration_width, trim_trailing_whitespace,
    wrapped_cells_to_line,
};
use table::render_table_row_with_links;

pub(super) struct WrappedLine {
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

#[allow(clippy::too_many_lines)]
pub(super) fn wrap_line_with_links(line: &LogicalLine, width: usize) -> Vec<WrappedLine> {
    if let Some(table) = &line.table_row {
        return render_table_row_with_links(line.indent, table, width);
    }
    match line.surface {
        LineSurface::TldrTop => {
            return vec![WrappedLine {
                anchors: Vec::new(),
                line: panel_border(width, '┌', '┐'),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::TldrBottom => {
            return vec![WrappedLine {
                anchors: Vec::new(),
                line: panel_border(width, '└', '┘'),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Divider => {
            return vec![WrappedLine {
                anchors: Vec::new(),
                line: Line::from(Span::styled(
                    "─".repeat(width),
                    Style::default().fg(theme::OVERLAY),
                )),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Rule => {
            let indent = line.indent.min(width.saturating_sub(1));
            return vec![WrappedLine {
                anchors: Vec::new(),
                line: Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(
                        "─".repeat(width.saturating_sub(indent)),
                        Style::default().fg(theme::OVERLAY),
                    ),
                ]),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Normal | LineSurface::Code | LineSurface::Tldr => {}
    }

    let decoration_width = tldr_decoration_width(line, width);
    let mut cells = styled_cells(line);

    if cells.is_empty() {
        return vec![wrapped_cells_to_line(
            line,
            width,
            line.indent.min(width.saturating_sub(1)),
            &[],
            false,
        )];
    }

    let mut result = Vec::new();
    let mut first_row = true;
    let mut join_with_space = false;
    while !cells.is_empty() {
        let indent = if first_row {
            line.indent
        } else {
            line.continuation_indent
        }
        .min(width.saturating_sub(1));
        let available = width
            .saturating_sub(indent)
            .saturating_sub(decoration_width)
            .max(1);
        if let Some(first) = cells.first_mut()
            && first.width > available
        {
            // A double-width glyph cannot occupy a one-column viewport.
            // Preserve its semantic search cell while rendering one bounded
            // replacement cell, just as control characters are sanitized.
            first.display_character = '\u{fffd}';
            first.width = 1;
        }
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
            let split = cells[..fit]
                .iter()
                .rposition(|cell| cell.character.is_whitespace())
                .filter(|position| *position > 0)
                .unwrap_or(fit);
            let row_end = trim_trailing_whitespace(&cells, split);
            let emitted_row = row_end != 0;
            let mut removed_separator = if row_end == 0 {
                cells.drain(..fit.max(1));
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
            while cells
                .first()
                .is_some_and(|cell| cell.character.is_whitespace())
            {
                cells.remove(0);
                removed_separator = true;
            }
            join_with_space = if emitted_row {
                removed_separator
            } else {
                join_with_space || removed_separator
            };
        }
        first_row = false;
    }
    result
}

fn panel_border(width: usize, left: char, right: char) -> Line<'static> {
    let style = Style::default().fg(theme::MAUVE).bg(theme::TLDR_SURFACE);
    if width == 1 {
        return Line::from(Span::styled(left.to_string(), style));
    }
    Line::from(Span::styled(
        format!("{left}{}{right}", "─".repeat(width.saturating_sub(2))),
        style,
    ))
}
