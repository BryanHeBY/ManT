//! Text cells, decoration and exact link/search column projection.
use super::{
    Line, LineSurface, LogicalLine, Span, Style, UnicodeWidthChar, WrappedLine, WrappedLink,
    WrappedSearchCell, theme,
};
#[derive(Clone, Copy)]
pub(super) struct StyledCell {
    pub(super) character: char,
    pub(super) display_character: char,
    pub(super) width: usize,
    pub(super) style: Style,
    pub(super) link_index: Option<usize>,
}

pub(super) fn styled_cells(line: &LogicalLine) -> Vec<StyledCell> {
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
                    display_character: ' ',
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
                display_character: character,
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

pub(super) fn fitting_prefix(cells: &[StyledCell], available: usize) -> usize {
    let mut width = 0;
    for (index, cell) in cells.iter().enumerate() {
        if width + cell.width > available {
            return index.max(1);
        }
        width += cell.width;
    }
    cells.len()
}

pub(super) fn trim_trailing_whitespace(cells: &[StyledCell], end: usize) -> usize {
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

    let framed_tldr = tldr_decoration_width(line, width) != 0;
    if framed_tldr {
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
            value.push(cell.display_character);
        }
        spans.push(Span::styled(value, current_style));
    }

    if let Some(color) = background {
        let content_width = cells.iter().map(|cell| cell.width).sum::<usize>();
        let fill = match line.surface {
            LineSurface::Code => width.saturating_sub(indent).saturating_sub(content_width),
            LineSurface::Tldr => width
                .saturating_sub(indent + content_width)
                .saturating_sub(tldr_decoration_width(line, width)),
            LineSurface::Normal
            | LineSurface::TldrTop
            | LineSurface::TldrBottom
            | LineSurface::Divider
            | LineSurface::Rule => 0,
        };
        spans.push(Span::styled(" ".repeat(fill), Style::default().bg(color)));
    }
    if framed_tldr {
        spans.push(Span::styled(
            " │",
            Style::default().fg(theme::MAUVE).bg(theme::TLDR_SURFACE),
        ));
    }
    Line::from(spans)
}

pub(super) fn wrapped_cells_to_line(
    line: &LogicalLine,
    width: usize,
    indent: usize,
    cells: &[StyledCell],
    join_with_space: bool,
) -> WrappedLine {
    let mut links = Vec::new();
    let mut search_cells = Vec::with_capacity(cells.len());
    let mut column = indent + tldr_decoration_width(line, width) / 2;
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
        anchors: Vec::new(),
        line: cells_to_line(line, width, indent, cells),
        links,
        search_cells,
    }
}

fn with_background(style: Style, background: Option<ratatui::style::Color>) -> Style {
    background.map_or(style, |color| style.bg(color))
}

pub(super) const fn tldr_decoration_width(line: &LogicalLine, width: usize) -> usize {
    if matches!(line.surface, LineSurface::Tldr) && width >= 6 {
        4
    } else {
        0
    }
}
