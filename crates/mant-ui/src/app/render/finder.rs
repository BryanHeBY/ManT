//! Document finder overlay and result hit-test geometry.
use crate::app::{
    App, PointerDrag,
    finder::{FinderTreeRow, document_category},
    fit_to_width,
};
use crate::{scrollbar::VerticalScrollbar, text::sanitize_terminal_text, theme};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;
impl App {
    pub(in crate::app) fn draw_document_finder(&mut self, frame: &mut Frame<'_>) {
        let width = 76.min(frame.area().width.saturating_sub(2));
        let height = 18.min(frame.area().height.saturating_sub(2));
        if width < 24 || height < 7 {
            self.clear_finder_geometry();
            return;
        }
        let area = Rect::new(
            frame.area().x + frame.area().width.saturating_sub(width) / 2,
            frame.area().y + frame.area().height.saturating_sub(height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title(" Open Document ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BLUE))
            .style(Style::default().bg(theme::BASE));
        let inner = block.inner(area).inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
        frame.render_widget(block, area);
        let [query_area, hint_area, results_area] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(inner);
        self.geometry.finder_query = query_area;

        let (before_cursor, after_cursor) = self.finder.draft.split_at(self.finder.cursor);
        let cursor_character = after_cursor.chars().next();
        let cursor_bytes = cursor_character.map_or(0, char::len_utf8);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Search: ", Style::default().fg(theme::YELLOW)),
                Span::styled(before_cursor.to_owned(), Style::default().fg(theme::TEXT)),
                Span::styled(
                    cursor_character.map_or_else(|| " ".to_owned(), |value| value.to_string()),
                    Style::default().fg(theme::BASE).bg(theme::TEXT),
                ),
                Span::styled(
                    after_cursor[cursor_bytes..].to_owned(),
                    Style::default().fg(theme::TEXT),
                ),
            ]))
            .style(Style::default().bg(theme::BASE)),
            query_area,
        );
        let result_count = self.finder.total;
        let hint = if self.finder.draft.is_empty() {
            format!("{result_count} documents · click/Enter open/toggle · wheel scroll · Esc close")
        } else {
            format!("{result_count} matches · click/Enter open · wheel scroll · Esc close")
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(theme::SUBTEXT)),
            hint_area,
        );

        self.draw_finder_results(frame, results_area);
    }

    fn draw_finder_results(&mut self, frame: &mut Frame<'_>, results_area: Rect) {
        let visible_height = usize::from(results_area.height);
        let row_count = self.finder.row_count();
        self.finder.ensure_selected_visible(visible_height);
        let scroll = self.finder.scroll;
        self.geometry.finder_results = results_area;
        self.geometry.finder_scrollbar =
            VerticalScrollbar::new(results_area, row_count, visible_height, scroll);
        let row_width = results_area
            .width
            .saturating_sub(u16::from(self.geometry.finder_scrollbar.is_some()));
        for visual_row in 0..visible_height.min(row_count.saturating_sub(scroll)) {
            let row = Rect::new(
                results_area.x,
                results_area.y + u16::try_from(visual_row).unwrap_or_default(),
                row_width,
                1,
            );
            let selected = scroll + visual_row == self.finder.selected;
            let value = if self.finder.draft.is_empty() {
                match &self.finder.tree[scroll + visual_row] {
                    FinderTreeRow::Folder { path, name, depth } => {
                        let marker = if self.finder.expanded(path) {
                            "▾"
                        } else {
                            "▸"
                        };
                        fit_to_width(
                            &format!("{}{marker} {name}/", "  ".repeat(*depth)),
                            usize::from(row.width),
                        )
                    }
                    FinderTreeRow::Document { index, depth } => {
                        let Some(document) = self.finder.catalog.get(*index) else {
                            continue;
                        };
                        fit_to_width(
                            &format!("{}  {}", "  ".repeat(*depth), document.address.name()),
                            usize::from(row.width),
                        )
                    }
                }
            } else {
                let match_index = self.finder.matches[scroll + visual_row];
                let Some(document) = self.finder.catalog.get(match_index) else {
                    continue;
                };
                let category = document_category(&document.address);
                let catalog_path = document.catalog_path();
                let name = sanitize_terminal_text(&catalog_path);
                let gap = usize::from(row.width)
                    .saturating_sub(name.width())
                    .saturating_sub(category.width())
                    .max(1);
                fit_to_width(
                    &format!("{name}{}{category}", " ".repeat(gap)),
                    usize::from(row.width),
                )
            };
            let style = if selected {
                Style::default()
                    .fg(theme::SELECTED_TEXT)
                    .bg(theme::SELECTED)
            } else {
                Style::default().fg(theme::TEXT).bg(theme::BASE)
            };
            frame.render_widget(Paragraph::new(Span::styled(value, style)), row);
        }
        if let Some(scrollbar) = self.geometry.finder_scrollbar {
            scrollbar.render(frame);
        } else if matches!(self.pointer.drag(), PointerDrag::FinderScrollbar(_)) {
            self.pointer.finish_drag();
        }
    }

    pub(in crate::app) fn clear_finder_geometry(&mut self) {
        self.geometry.finder_query = Rect::default();
        self.geometry.finder_results = Rect::default();
        self.geometry.finder_scrollbar = None;
        if matches!(self.pointer.drag(), PointerDrag::FinderScrollbar(_)) {
            self.pointer.finish_drag();
        }
    }
}
