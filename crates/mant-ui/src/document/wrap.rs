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
use mant_ir::{ContentPointKey, FixedLineKey};

use super::{
    LineSurface, LinkIdentity, LogicalLine, WrapMode,
    model::{LogicalTableCell, LogicalTableLayout, LogicalTableRow},
};
use crate::theme;

mod cells;
mod table;
use cells::{
    fitting_prefix, styled_cells, tldr_decoration_width, trim_trailing_whitespace,
    visit_styled_cells, wrapped_cells_to_line,
};
use table::render_table_row_with_links;

/// Keep a useful reading area without changing the logical source origins.
/// Reduce both origins by the same displacement whenever indentation would
/// leave less than 16 cells (half the available width on a narrow viewport).
/// The one/two-cell fallback necessarily reserves the entire content area.
pub(super) fn readable_origins(
    first: usize,
    continuation: usize,
    available: usize,
) -> (usize, usize) {
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
    /// The current table cell's boundary, independent of text alignment.
    pub(super) cell_points: Vec<(ContentPointKey, usize)>,
    /// Points within the cell payload, translated with aligned content.
    pub(super) points: Vec<(ContentPointKey, usize)>,
    pub(super) fixed_lines: Vec<FixedLineKey>,
    pub(super) fixed_points: Vec<(ContentPointKey, usize)>,
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
    pub(super) identity: LinkIdentity,
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
    wrap_line_with_links_at_offset(line, width, 0)
}

pub(super) fn wrap_line_with_links_at_offset(
    line: &LogicalLine,
    width: usize,
    horizontal_offset: usize,
) -> Vec<WrappedLine> {
    let mut rows = wrap_logical_line(line, width, horizontal_offset);
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

#[allow(clippy::too_many_lines)]
fn wrap_logical_line(
    line: &LogicalLine,
    width: usize,
    horizontal_offset: usize,
) -> Vec<WrappedLine> {
    if let Some(table) = &line.table_row {
        return render_table_row_with_links(line.indent, table, width);
    }
    match line.surface {
        LineSurface::TldrTop => {
            return vec![WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                cell_points: Vec::new(),
                points: Vec::new(),
                fixed_lines: Vec::new(),
                fixed_points: Vec::new(),
                line: panel_border(width, '┌', '┐'),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::TldrBottom => {
            return vec![WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                cell_points: Vec::new(),
                points: Vec::new(),
                fixed_lines: Vec::new(),
                fixed_points: Vec::new(),
                line: panel_border(width, '└', '┘'),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Divider => {
            return vec![WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                cell_points: Vec::new(),
                points: Vec::new(),
                fixed_lines: Vec::new(),
                fixed_points: Vec::new(),
                line: Line::from(Span::styled(
                    "─".repeat(width),
                    Style::default().fg(theme::OVERLAY),
                )),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Rule => {
            let indent = readable_origins(line.indent, line.indent, width).0;
            return vec![WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                cell_points: Vec::new(),
                points: Vec::new(),
                fixed_lines: Vec::new(),
                fixed_points: Vec::new(),
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
        LineSurface::DoubleRule => {
            let indent = readable_origins(line.indent, line.indent, width).0;
            return vec![WrappedLine {
                source_end: None,
                anchors: Vec::new(),
                cell_points: Vec::new(),
                points: Vec::new(),
                fixed_lines: Vec::new(),
                fixed_points: Vec::new(),
                line: Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(
                        "═".repeat(width.saturating_sub(indent)),
                        Style::default().fg(theme::OVERLAY),
                    ),
                ]),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Normal | LineSurface::Code | LineSurface::Fixed | LineSurface::Tldr => {}
    }

    if line.wrap_mode == WrapMode::NoWrap {
        let end = horizontal_offset.saturating_add(width);
        let mut column = 0_usize;
        let mut selected = Vec::new();
        let mut started = false;
        let mut padding = 0;
        visit_styled_cells(line, |cell| {
            if cell.grapheme_start {
                let next = column.saturating_add(cell.width);
                if next > end {
                    return false;
                }
                if column >= horizontal_offset && !started {
                    started = true;
                    padding = column - horizontal_offset;
                }
                column = next;
            }
            if started {
                selected.push(cell);
            }
            true
        });
        return vec![wrapped_cells_to_line(
            line, width, padding, &selected, false,
        )];
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
        if let Some(first) = cells.first_mut()
            && first.width > available
        {
            // A double-width glyph cannot occupy a one-column viewport.
            // Preserve its semantic search cell while rendering one bounded
            // replacement cell, just as control characters are sanitized.
            first.display_character = Some('\u{fffd}');
            first.display_override = None;
            first.width = 1;
            for cell in cells
                .iter_mut()
                .skip(1)
                .take_while(|cell| !cell.grapheme_start)
            {
                cell.display_character = None;
                cell.display_override = None;
            }
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
                .rposition(|cell| cell.grapheme_start && cell.whitespace)
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
            while cells.first().is_some_and(|cell| cell.whitespace) {
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

#[cfg(test)]
mod no_wrap_tests {
    use std::num::NonZeroU32;

    use ratatui::{style::Modifier, text::Span};

    use super::*;
    use crate::document::{LogicalLinkRange, model::LinkIdentity};

    #[test]
    fn million_column_fixed_row_projects_only_the_requested_window() {
        // Pinned CVS term.c::term_flushln/term_field keeps native physical
        // rows; the exact .Bd -literal input was checked with the reference.
        let line = LogicalLine::plain(0, "x".repeat(1_000_000), Style::default())
            .surface(LineSurface::Fixed)
            .wrap_mode(WrapMode::NoWrap);
        let first = wrap_line_with_links_at_offset(&line, 8, 0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].line.to_string(), "xxxxxxxx");
        let tail = wrap_line_with_links_at_offset(&line, 8, 999_992);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].line.to_string(), "xxxxxxxx");
    }

    #[test]
    fn left_clipped_wide_glyph_keeps_link_cells_and_source_style() {
        // Pinned CVS term.c::term_field places the UTF-8 glyph at its native
        // visual column; viewport clipping does not split that glyph.
        let identity = LinkIdentity::NativeFixed(NonZeroU32::new(1).unwrap());
        let linked = Style::default().add_modifier(Modifier::UNDERLINED);
        let mut line = LogicalLine::plain(0, "", Style::default())
            .surface(LineSurface::Fixed)
            .wrap_mode(WrapMode::NoWrap);
        line.spans = vec![Span::raw("abc"), Span::styled("界LINK", linked)];
        line.links.push(LogicalLinkRange {
            identity,
            start_scalar: 4,
            end_scalar: 8,
        });
        let clipped = wrap_line_with_links_at_offset(&line, 5, 4);
        assert_eq!(clipped[0].line.to_string(), " LINK");
        assert_eq!(clipped[0].links.len(), 1);
        assert_eq!(clipped[0].links[0].identity, identity);
        assert_eq!(
            (
                clipped[0].links[0].start_column,
                clipped[0].links[0].end_column
            ),
            (1, 5)
        );
        assert!(
            clipped[0]
                .line
                .spans
                .iter()
                .any(|span| span.content == "LINK"
                    && span.style.add_modifier.contains(Modifier::UNDERLINED))
        );
        let complete = wrap_line_with_links_at_offset(&line, 3, 3);
        assert_eq!(complete[0].line.to_string(), "界L");
    }
}
