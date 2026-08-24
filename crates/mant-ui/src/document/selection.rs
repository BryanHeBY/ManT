//! Width-aware document selection and plain-text extraction.

use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthChar;

use super::RenderedDocument;
use crate::theme;

/// One terminal-cell position in the fully rendered document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TextPosition {
    pub(crate) row: usize,
    pub(crate) column: usize,
}

/// A linear selection whose endpoints are inclusive terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RenderedSelection {
    pub(crate) anchor: TextPosition,
    pub(crate) focus: TextPosition,
}

impl RenderedSelection {
    pub(crate) const fn new(anchor: TextPosition) -> Self {
        Self {
            anchor,
            focus: anchor,
        }
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.anchor.row == self.focus.row && self.anchor.column == self.focus.column
    }

    fn normalized(self) -> (TextPosition, TextPosition) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    fn columns_for_row(self, row: usize) -> Option<(usize, usize)> {
        let (start, end) = self.normalized();
        if row < start.row || row > end.row {
            return None;
        }
        let start_column = if row == start.row { start.column } else { 0 };
        let end_column = if row == end.row {
            end.column.saturating_add(1)
        } else {
            usize::MAX
        };
        Some((start_column, end_column))
    }
}

impl RenderedDocument {
    /// Extract the selected visual cells as terminal-safe plain text.
    pub(crate) fn selected_text(&self, selection: RenderedSelection) -> String {
        let (start, end) = selection.normalized();
        let Some(lines) = self
            .text
            .lines
            .get(start.row..=end.row.min(self.row_count.saturating_sub(1)))
        else {
            return String::new();
        };
        lines
            .iter()
            .enumerate()
            .filter_map(|(offset, line)| {
                let row = start.row + offset;
                let surface = self.surfaces.get(row).copied()?;
                if matches!(
                    surface,
                    super::LineSurface::TldrTop
                        | super::LineSurface::TldrBottom
                        | super::LineSurface::Divider
                ) {
                    return None;
                }
                let (start_column, end_column) = selection
                    .columns_for_row(row)
                    .expect("selected row is inside normalized endpoints");
                let (start_column, end_column) =
                    if surface == super::LineSurface::Tldr && line.width() >= 6 {
                        let width = line.width();
                        (start_column.max(2), end_column.min(width.saturating_sub(2)))
                    } else {
                        (start_column, end_column)
                    };
                Some(
                    line_fragment(line, start_column, end_column)
                        .trim_end_matches(' ')
                        .to_owned(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn highlight_selection(
        &self,
        text: &mut Text<'static>,
        viewport_start: usize,
        selection: RenderedSelection,
    ) {
        for (offset, line) in text.lines.iter_mut().enumerate() {
            let row = viewport_start + offset;
            if row >= self.row_count {
                break;
            }
            let Some((start_column, end_column)) = selection.columns_for_row(row) else {
                continue;
            };
            *line = highlight_line(line, start_column, end_column);
        }
    }
}

fn line_fragment(line: &Line<'_>, start_column: usize, end_column: usize) -> String {
    let mut output = String::new();
    let mut column: usize = 0;
    let mut previous_selected = false;
    for span in &line.spans {
        for character in span.content.chars() {
            let width = character.width().unwrap_or(0);
            let next_column = column.saturating_add(width);
            let selected = if width == 0 {
                previous_selected
            } else {
                start_column < next_column && end_column > column
            };
            if selected {
                output.push(character);
            }
            previous_selected = selected;
            column = next_column;
        }
    }
    output
}

fn highlight_line(line: &Line<'static>, start_column: usize, end_column: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut column: usize = 0;
    let mut previous_selected = false;
    for span in &line.spans {
        let mut segment = String::new();
        let mut segment_selected = None;
        for character in span.content.chars() {
            let width = character.width().unwrap_or(0);
            let next_column = column.saturating_add(width);
            let selected = if width == 0 {
                previous_selected
            } else {
                start_column < next_column && end_column > column
            };
            if segment_selected.is_some_and(|current| current != selected) {
                spans.push(selection_span(
                    std::mem::take(&mut segment),
                    span.style,
                    segment_selected.expect("checked selection state"),
                ));
            }
            segment_selected = Some(selected);
            segment.push(character);
            previous_selected = selected;
            column = next_column;
        }
        if !segment.is_empty() {
            spans.push(selection_span(
                segment,
                span.style,
                segment_selected.unwrap_or(false),
            ));
        }
    }
    Line::from(spans)
}

fn selection_span(value: String, style: ratatui::style::Style, selected: bool) -> Span<'static> {
    Span::styled(
        value,
        if selected {
            style.fg(theme::SELECTED_TEXT).bg(theme::SELECTED)
        } else {
            style
        },
    )
}

#[cfg(test)]
mod tests {
    use ratatui::{
        style::{Color, Style},
        text::{Line, Span},
    };

    use super::{highlight_line, line_fragment};
    use crate::theme;

    #[test]
    fn fragments_follow_terminal_cells_without_splitting_wide_characters() {
        let line = Line::from(vec![
            Span::raw("a"),
            Span::styled("日e\u{301}😀z", Style::default().fg(Color::Green)),
        ]);

        assert_eq!(line_fragment(&line, 1, 3), "日");
        assert_eq!(line_fragment(&line, 3, 4), "e\u{301}");
        assert_eq!(line_fragment(&line, 4, 5), "😀");
        assert_eq!(line_fragment(&line, 5, 6), "😀");
    }

    #[test]
    fn selection_highlight_preserves_styles_outside_the_selected_cells() {
        let line = Line::from(vec![
            Span::styled("ab", Style::default().fg(Color::Green)),
            Span::styled("日e\u{301}", Style::default().fg(Color::Blue)),
        ]);

        let highlighted = highlight_line(&line, 1, 4);
        assert_eq!(highlighted.to_string(), "ab日e\u{301}");
        assert_eq!(highlighted.spans[0].style.fg, Some(Color::Green));
        assert_eq!(highlighted.spans[0].style.bg, None);
        assert_eq!(highlighted.spans[1].style.bg, Some(theme::SELECTED));
        assert_eq!(highlighted.spans[2].style.bg, Some(theme::SELECTED));
        assert_eq!(highlighted.spans[3].style.bg, None);
    }
}
