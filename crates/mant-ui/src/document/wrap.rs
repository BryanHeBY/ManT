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

use super::{LineSurface, LinkTarget, LogicalLine, WrapMode, model::LogicalTableRow};
use crate::theme;

const TABLE_COLUMN_GAP: usize = 2;

#[derive(Clone, Copy)]
struct StyledCell {
    character: char,
    width: usize,
    style: Style,
    link_index: Option<usize>,
}

pub(super) struct WrappedLine {
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
                line: panel_border(width, '┌', '┐'),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::TldrBottom => {
            return vec![WrappedLine {
                line: panel_border(width, '└', '┘'),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Divider => {
            return vec![WrappedLine {
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

    let decoration_width = usize::from(line.surface == LineSurface::Tldr) * 4;
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

fn styled_cells(line: &LogicalLine) -> Vec<StyledCell> {
    const TAB_STOP: usize = 8;

    let mut cells = Vec::new();
    let mut column = line.indent;
    let mut source_column = 0;
    for span in &line.spans {
        for character in span.content.chars() {
            let source_width = character.width().unwrap_or(0);
            let link_index = line.links.iter().position(|link| {
                link.start_column < source_column + source_width && link.end_column > source_column
            });
            if character == '\t' {
                let spaces = TAB_STOP - column % TAB_STOP;
                cells.extend((0..spaces).map(|_| StyledCell {
                    character: ' ',
                    width: 1,
                    style: span.style,
                    link_index,
                }));
                column += spaces;
                source_column += spaces;
                continue;
            }
            let character = if character.is_control() {
                '\u{fffd}'
            } else {
                character
            };
            let cell_width = character.width().unwrap_or(0);
            cells.push(StyledCell {
                character,
                width: cell_width,
                style: span.style,
                link_index,
            });
            column += cell_width;
            source_column += source_width;
        }
    }
    cells
}

fn render_table_row_with_links(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
) -> Vec<WrappedLine> {
    if table.cells.is_empty() {
        return vec![WrappedLine {
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
        }];
    }
    let indent = indent.min(width.saturating_sub(1));
    let available = width.saturating_sub(indent).max(1);
    let Some(column_widths) = table_column_widths(&table.layout.preferred_widths, available) else {
        return stack_table_cells(indent, table, width);
    };
    render_table_columns(indent, table, &column_widths)
}

fn stack_table_cells(indent: usize, table: &LogicalTableRow, width: usize) -> Vec<WrappedLine> {
    let mut rows = table
        .cells
        .iter()
        .flat_map(|cell| cell.lines.iter())
        .flat_map(|line| {
            let mut line = line.clone();
            line.indent = line.indent.saturating_add(indent);
            line.continuation_indent = line.continuation_indent.saturating_add(indent);
            wrap_line_with_links(&line, width)
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(WrappedLine {
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
        });
    }
    rows
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
            let mut rendered = Vec::new();
            for line in &cell.lines {
                let group = next_search_group;
                next_search_group += 1;
                let mut wrapped = wrap_line_with_links(line, *column_width);
                for row in &mut wrapped {
                    for cell in &mut row.search_cells {
                        cell.group = group;
                    }
                }
                rendered.extend(wrapped);
            }
            rendered
        })
        .collect::<Vec<_>>();
    let row_count = rendered_cells.iter().map(Vec::len).max().unwrap_or(1);

    (0..row_count)
        .map(|row_index| {
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
                    used = UnicodeWidthStr::width(row.line.to_string().as_str());
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

fn fitting_prefix(cells: &[StyledCell], available: usize) -> usize {
    let mut width = 0;
    for (index, cell) in cells.iter().enumerate() {
        if width + cell.width > available {
            return index.max(1);
        }
        width += cell.width;
    }
    cells.len()
}

fn trim_trailing_whitespace(cells: &[StyledCell], end: usize) -> usize {
    let mut result = end;
    while result > 0 && cells[result - 1].character.is_whitespace() {
        result -= 1;
    }
    result
}

fn cells_to_line(
    line: &LogicalLine,
    width: usize,
    indent: usize,
    cells: &[StyledCell],
) -> Line<'static> {
    let mut spans = Vec::new();
    let background = match line.surface {
        LineSurface::Code => Some(theme::SURFACE),
        LineSurface::Tldr => Some(theme::TLDR_SURFACE),
        LineSurface::Normal
        | LineSurface::TldrTop
        | LineSurface::TldrBottom
        | LineSurface::Divider
        | LineSurface::Rule => None,
    };

    if line.surface == LineSurface::Tldr {
        spans.push(Span::styled(
            "│ ",
            Style::default().fg(theme::MAUVE).bg(theme::TLDR_SURFACE),
        ));
    }
    if indent > 0 {
        // Structural indentation belongs to the document, not to a compact
        // code surface. TLDR remains a full-width panel and therefore keeps
        // its indentation on the panel background.
        let style = if line.surface == LineSurface::Tldr {
            Style::default().bg(theme::TLDR_SURFACE)
        } else {
            Style::default()
        };
        spans.push(Span::styled(" ".repeat(indent), style));
    }

    if let Some(first) = cells.first() {
        let mut current_style = with_background(first.style, background);
        let mut value = String::new();
        for cell in cells {
            let style = with_background(cell.style, background);
            if style != current_style {
                spans.push(Span::styled(std::mem::take(&mut value), current_style));
                current_style = style;
            }
            value.push(cell.character);
        }
        spans.push(Span::styled(value, current_style));
    }

    if let Some(color) = background {
        let content_width = cells.iter().map(|cell| cell.width).sum::<usize>();
        let fill = match line.surface {
            LineSurface::Code => width.saturating_sub(indent).saturating_sub(content_width),
            LineSurface::Tldr => width
                .saturating_sub(indent + content_width)
                .saturating_sub(4),
            LineSurface::Normal
            | LineSurface::TldrTop
            | LineSurface::TldrBottom
            | LineSurface::Divider
            | LineSurface::Rule => 0,
        };
        spans.push(Span::styled(" ".repeat(fill), Style::default().bg(color)));
    }
    if line.surface == LineSurface::Tldr {
        spans.push(Span::styled(
            " │",
            Style::default().fg(theme::MAUVE).bg(theme::TLDR_SURFACE),
        ));
    }
    Line::from(spans)
}

fn wrapped_cells_to_line(
    line: &LogicalLine,
    width: usize,
    indent: usize,
    cells: &[StyledCell],
    join_with_space: bool,
) -> WrappedLine {
    let mut links = Vec::new();
    let mut search_cells = Vec::with_capacity(cells.len());
    let mut column = indent + usize::from(line.surface == LineSurface::Tldr) * 2;
    let mut active: Option<(usize, usize, usize)> = None;
    for (index, cell) in cells.iter().enumerate() {
        let next_column = column + cell.width;
        search_cells.push(WrappedSearchCell {
            group: 0,
            join_before: index == 0 && join_with_space,
            character: cell.character,
            start_column: column,
            end_column: next_column,
        });
        match (active, cell.link_index) {
            (Some((index, start, _)), Some(next)) if index == next => {
                active = Some((index, start, next_column));
            }
            (Some((index, start, end)), next) => {
                links.push(WrappedLink {
                    target: line.links[index].target.clone(),
                    start_column: start,
                    end_column: end,
                });
                active = next.map(|index| (index, column, next_column));
            }
            (None, Some(index)) => active = Some((index, column, next_column)),
            (None, None) => {}
        }
        column = next_column;
    }
    if let Some((index, start, end)) = active {
        links.push(WrappedLink {
            target: line.links[index].target.clone(),
            start_column: start,
            end_column: end,
        });
    }
    WrappedLine {
        line: cells_to_line(line, width, indent, cells),
        links,
        search_cells,
    }
}

fn with_background(style: Style, background: Option<ratatui::style::Color>) -> Style {
    background.map_or(style, |color| style.bg(color))
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
