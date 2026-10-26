//! Text cells, decoration and exact link/search column projection.
use super::{
    Line, LineSurface, LogicalLine, Span, Style, WrappedLine, WrappedLink, WrappedSearchCell, theme,
};
#[derive(Clone, Copy)]
pub(super) struct StyledCell {
    pub(super) source_index: usize,
    pub(super) character: char,
    pub(super) display_character: Option<char>,
    pub(super) grapheme_start: bool,
    pub(super) whitespace: bool,
    pub(super) width: usize,
    pub(super) style: Style,
    pub(super) link_index: Option<usize>,
    pub(super) copyable: bool,
}

pub(super) fn styled_cells(line: &LogicalLine) -> Vec<StyledCell> {
    const TAB_STOP: usize = 8;

    let mut cells = Vec::new();
    let mut column = line.indent;
    let mut source_index = 0;
    // A grapheme can cross source-style boundaries. Segment the whole logical
    // row, then give each indivisible terminal glyph its first scalar's style.
    // Keep scalar source cells for search/reference coordinates, charging its
    // terminal width once and never wrapping in the middle of the cluster.
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let mut spans = line.spans.iter();
    let mut span = spans.next();
    let mut span_end = span.map_or(0, |span| span.content.len());
    for grapheme in mant_render::cells::graphemes(&text) {
        while grapheme.bytes().start >= span_end {
            span = spans.next();
            span_end += span.map_or(0, |span| span.content.len());
            if span.is_none() {
                break;
            }
        }
        let style = span.map_or_else(Style::default, |span| span.style);
        let source_width = grapheme.columns();
        let link_index = grapheme_link(line, source_index, grapheme.text().chars().count());
        if grapheme.text() == "\t" {
            let spaces = TAB_STOP - column % TAB_STOP;
            cells.extend((0..spaces).map(|_| {
                StyledCell {
                    source_index,
                    character: ' ',
                    display_character: Some(' '),
                    grapheme_start: true,
                    whitespace: true,
                    width: 1,
                    style,
                    link_index,
                    copyable: !line
                        .layout_scalars
                        .iter()
                        .any(|range| range.contains(&source_index)),
                }
            }));
            column += spaces;
            source_index += 1;
            continue;
        }
        let display_width = if grapheme.text().chars().any(char::is_control) {
            1
        } else {
            source_width
        };
        // Fixed nonbreaking spaces remain source cells even when a narrow
        // viewport forces a character split inside their containing word.
        let whitespace = grapheme.text().chars().all(|character| {
            character.is_whitespace() && !matches!(character, '\u{a0}' | '\u{2007}' | '\u{202f}')
        });
        for (index, character) in grapheme.text().chars().enumerate() {
            let character = if character.is_control() {
                '\u{fffd}'
            } else {
                character
            };
            let cell_width = if index == 0 { display_width } else { 0 };
            cells.push(StyledCell {
                source_index,
                character,
                display_character: Some(character),
                grapheme_start: index == 0,
                whitespace,
                width: cell_width,
                style,
                link_index,
                copyable: !line
                    .layout_scalars
                    .iter()
                    .any(|range| range.contains(&source_index)),
            });
            column += cell_width;
            source_index += 1;
        }
    }
    cells
}

/// A terminal glyph is indivisible. A unique target covering any source scalar
/// owns the whole glyph. Distinct targets sharing it are deliberately not
/// directly clickable: document-reference selectors remain available,
/// rather than silently choosing the first target in traversal order.
fn grapheme_link(line: &LogicalLine, start: usize, scalars: usize) -> Option<usize> {
    let mut matches = line
        .links
        .iter()
        .enumerate()
        .filter(|(_, link)| link.start_scalar < start + scalars && link.end_scalar > start);
    let (index, first) = matches.next()?;
    matches
        .all(|(_, link)| link.target == first.target)
        .then_some(index)
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
    while result > 0 && cells[result - 1].whitespace {
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
        LineSurface::Code => theme::style(theme::StyleRole::CodeSurface).bg,
        LineSurface::Tldr => theme::style(theme::StyleRole::TldrSurface).bg,
        LineSurface::Normal
        | LineSurface::TldrTop
        | LineSurface::TldrBottom
        | LineSurface::Divider
        | LineSurface::Rule
        | LineSurface::DoubleRule => None,
    };

    let framed_tldr = tldr_decoration_width(line, width) != 0;
    if framed_tldr {
        spans.push(Span::styled(
            "│ ",
            theme::style(theme::StyleRole::TldrSurface)
                .patch(theme::style(theme::StyleRole::TldrFrame)),
        ));
    }
    if indent > 0 {
        // Structural indentation belongs to the document, not to a compact
        // code surface. TLDR remains a full-width panel and therefore keeps
        // its indentation on the panel background.
        let style = if line.surface == LineSurface::Tldr {
            theme::style(theme::StyleRole::TldrSurface)
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
            if let Some(character) = cell.display_character {
                value.push(character);
            }
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
            | LineSurface::Rule
            | LineSurface::DoubleRule => 0,
        };
        spans.push(Span::styled(" ".repeat(fill), Style::default().bg(color)));
    }
    if framed_tldr {
        spans.push(Span::styled(
            " │",
            theme::style(theme::StyleRole::TldrSurface)
                .patch(theme::style(theme::StyleRole::TldrFrame)),
        ));
    }
    Line::from(spans)
}

/// Align only the content after a fixed structural prefix on the first row.
/// Returned cells carry exact omission receipts for copying and searching.
fn aligned_cells(
    line: &LogicalLine,
    width: usize,
    indent: usize,
    cells: &[StyledCell],
) -> (Option<Vec<StyledCell>>, usize) {
    let prefix_end =
        cells.partition_point(|cell| cell.source_index < line.alignment_prefix_scalars);
    let prefix_width = cells[..prefix_end]
        .iter()
        .map(|cell| cell.width)
        .sum::<usize>();
    let alignment_padding = if line.defer_alignment {
        0
    } else {
        line.alignment.padding(
            width.saturating_sub(indent + tldr_decoration_width(line, width) + prefix_width),
            cells[prefix_end..].iter().map(|cell| cell.width).sum(),
        )
    };
    // The marker belongs to its structural list. Position the paragraph's
    // content after that fixed prefix; subsequent wrapped rows have no prefix.
    let positioned_cells = (prefix_end > 0 && alignment_padding > 0).then(|| {
        let mut positioned = Vec::with_capacity(cells.len() + alignment_padding);
        positioned.extend_from_slice(&cells[..prefix_end]);
        positioned.extend(std::iter::repeat_n(
            StyledCell {
                source_index: cells[prefix_end].source_index,
                character: ' ',
                display_character: Some(' '),
                grapheme_start: true,
                whitespace: true,
                width: 1,
                style: Style::default(),
                link_index: None,
                copyable: false,
            },
            alignment_padding,
        ));
        positioned.extend_from_slice(&cells[prefix_end..]);
        positioned
    });
    let leading_alignment = if prefix_end == 0 {
        alignment_padding
    } else {
        0
    };
    (positioned_cells, leading_alignment)
}

pub(super) fn wrapped_cells_to_line(
    line: &LogicalLine,
    width: usize,
    indent: usize,
    layout_padding: usize,
    cells: &[StyledCell],
    join_with_space: bool,
) -> WrappedLine {
    // Place after wrapping, using the same grapheme cells as navigation,
    // search and selection. Generated alignment cells are never copy text.
    let (positioned_cells, leading_alignment) = aligned_cells(line, width, indent, cells);
    let cells = positioned_cells.as_deref().unwrap_or(cells);
    let indent = indent.saturating_add(leading_alignment);
    let layout_padding = layout_padding.saturating_add(leading_alignment);
    let mut links = Vec::new();
    let mut search_cells = Vec::with_capacity(cells.len());
    let mut column = indent + tldr_decoration_width(line, width) / 2;
    let mut grapheme_column = column;
    let mut active: Option<(usize, usize, usize)> = None;
    let mut copy_map = super::RowCopyMap::default();
    if line.defer_alignment && line.alignment != mant_ir::TextAlignment::Left {
        let prefix_end =
            cells.partition_point(|cell| cell.source_index < line.alignment_prefix_scalars);
        copy_map.alignment = Some(super::alignment::PreparedAlignment {
            placement: line.alignment,
            prefix: column
                + cells[..prefix_end]
                    .iter()
                    .map(|cell| cell.width)
                    .sum::<usize>(),
            content: cells[prefix_end..].iter().map(|cell| cell.width).sum(),
        });
    }
    if layout_padding > 0 {
        let start = column.saturating_sub(layout_padding.min(indent));
        copy_map.omitted.push(start..column);
    }
    for (index, cell) in cells.iter().enumerate() {
        if cell.grapheme_start {
            grapheme_column = column;
        }
        let next_column = column + cell.width;
        if !cell.copyable && cell.width > 0 {
            if let Some(previous) = copy_map.omitted.last_mut()
                && previous.end == column
            {
                previous.end = next_column;
            } else {
                copy_map.omitted.push(column..next_column);
            }
        }
        if cell.copyable {
            search_cells.push(WrappedSearchCell {
                group: 0,
                join_before: index == 0 && join_with_space,
                character: cell.character,
                start_column: grapheme_column,
                end_column: next_column,
            });
        }
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
    // A real empty hard row has no copyable content. Its structural prefix
    // and viewport paint still render, but cannot become authored spaces.
    // A whitespace-only source row has cells and retains its exact end.
    copy_map.end = Some(if cells.is_empty() { 0 } else { column });
    WrappedLine {
        source_end: cells.last().map(|cell| cell.source_index + 1),
        anchors: Vec::new(),
        line: cells_to_line(line, width, indent, cells),
        links,
        search_cells,
        copy_map,
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
