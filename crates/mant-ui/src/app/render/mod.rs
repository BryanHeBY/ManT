//! Frame composition; each panel records its own input geometry.
use crate::app::{App, PointerDrag};
use crate::{
    layout::{MIN_SIDEBAR_WIDTH, SIDEBAR_SPLITTER_WIDTH, maximum_sidebar_width, outline_visible},
    text::sanitize_terminal_text,
    theme,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

mod content;
mod finder;
mod navigation;
mod status;
#[cfg(test)]
pub(in crate::app) use navigation::sidebar_metadata;
impl App {
    /// Render one complete frame and retain its geometry for input hit testing.
    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let [menu_area, body_area, status_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        self.draw_menu(frame, menu_area);
        self.geometry.body = body_area;
        // This completed frame supersedes any host-announced resize dimensions.
        self.viewport_width = Some(body_area.width);
        if outline_visible(self.show_sidebar, body_area.width) {
            let sidebar_width = self
                .sidebar_width
                .clamp(MIN_SIDEBAR_WIDTH, maximum_sidebar_width(body_area.width));
            self.commit_sidebar_width(sidebar_width);
            let [navigation_area, splitter_area, content_area] = Layout::horizontal([
                Constraint::Length(self.sidebar_width),
                Constraint::Length(SIDEBAR_SPLITTER_WIDTH),
                Constraint::Min(1),
            ])
            .areas(body_area);
            self.draw_navigation(frame, navigation_area);
            self.draw_sidebar_splitter(frame, splitter_area);
            self.draw_content(frame, content_area);
        } else {
            self.geometry.navigation = Rect::default();
            self.geometry.outline_region = Rect::default();
            self.geometry.navigation_scrollbar = None;
            self.geometry.sidebar_splitter = Rect::default();
            self.geometry.navigation_rows.clear();
            if matches!(
                self.pointer.drag(),
                PointerDrag::Sidebar | PointerDrag::NavigationScrollbar(_)
            ) {
                self.pointer.finish_drag();
            }
            self.draw_content(frame, body_area);
        }
        if matches!(self.overlay, crate::app::Overlay::Menu { .. }) {
            Self::draw_menu_hint(frame, status_area);
        } else if self.search_is_open() {
            self.draw_search(frame, status_area);
        } else {
            self.draw_status(frame, status_area);
        }
        self.geometry.status = status_area;
        self.draw_overlay(frame);
        self.draw_copy_toast(frame);
    }

    fn draw_copy_toast(&self, frame: &mut Frame<'_>) {
        let Some(toast) = &self.copy_toast else {
            return;
        };
        let message = sanitize_terminal_text(&toast.message);
        let width = u16::try_from(message.width().saturating_add(4))
            .unwrap_or(u16::MAX)
            .min(frame.area().width);
        if width < 4 || frame.area().height < 4 {
            return;
        }
        let area = Rect::new(
            frame.area().x + frame.area().width.saturating_sub(width) / 2,
            frame.area().y + frame.area().height.saturating_sub(4),
            width,
            3,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::GREEN))
                        .style(Style::default().bg(theme::BASE)),
                )
                .style(Style::default().fg(theme::TEXT).bg(theme::BASE)),
            area,
        );
    }

    fn draw_sidebar_splitter(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.geometry.sidebar_splitter = area;
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(theme::BORDER))
                .style(theme::style(theme::StyleRole::OutlineSurface)),
            area,
        );
    }
}
