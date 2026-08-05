//! Interactive state machine and Ratatui widget composition.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ast::QueryBundle;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};

use crate::{DocumentView, RenderedDocument, theme};

const MIN_SIDEBAR_WIDTH: u16 = 20;
const MAX_SIDEBAR_WIDTH: u16 = 60;

/// All mutable interaction state for one `ManT` document.
pub struct App {
    document: DocumentView,
    selected: usize,
    content_scroll: usize,
    navigation_scroll: usize,
    sidebar_width: u16,
    show_sidebar: bool,
    quit: bool,
    last_content_area: Rect,
    last_navigation_area: Rect,
    rendered_cache: HashMap<u16, RenderedDocument>,
}

impl App {
    #[must_use]
    pub fn new(bundle: &QueryBundle) -> Self {
        Self {
            document: DocumentView::new(bundle),
            selected: 0,
            content_scroll: 0,
            navigation_scroll: 0,
            sidebar_width: 36,
            show_sidebar: true,
            quit: false,
            last_content_area: Rect::default(),
            last_navigation_area: Rect::default(),
            rendered_cache: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.select_relative(1),
            KeyCode::Char('k') | KeyCode::Up => self.select_relative(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_content(10),
            KeyCode::PageUp => self.scroll_content(-10),
            KeyCode::Home => self.content_scroll = 0,
            KeyCode::End => self.content_scroll = usize::MAX,
            KeyCode::Char('b') => self.show_sidebar = !self.show_sidebar,
            KeyCode::Char('<') => {
                self.sidebar_width = self.sidebar_width.saturating_sub(2).max(MIN_SIDEBAR_WIDTH);
            }
            KeyCode::Char('>') => {
                self.sidebar_width = self.sidebar_width.saturating_add(2).min(MAX_SIDEBAR_WIDTH);
            }
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollDown => self.scroll_content(3),
            MouseEventKind::ScrollUp => self.scroll_content(-3),
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .last_navigation_area
                    .contains((mouse.column, mouse.row).into()) =>
            {
                let local_row = usize::from(mouse.row - self.last_navigation_area.y);
                let index = self.navigation_scroll + local_row;
                if index < self.document.navigation().len() {
                    self.selected = index;
                    self.scroll_to_selected();
                }
            }
            _ => {}
        }
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let [menu_area, body_area, status_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        self.draw_menu(frame, menu_area);
        if self.show_sidebar && body_area.width > MIN_SIDEBAR_WIDTH + 20 {
            let [navigation_area, content_area] = Layout::horizontal([
                Constraint::Length(self.sidebar_width.min(body_area.width / 2)),
                Constraint::Min(1),
            ])
            .areas(body_area);
            self.draw_navigation(frame, navigation_area);
            self.draw_content(frame, content_area);
        } else {
            self.last_navigation_area = Rect::default();
            self.draw_content(frame, body_area);
        }
        self.draw_status(frame, status_area);
    }

    fn draw_menu(&self, frame: &mut Frame<'_>, area: Rect) {
        let title = Line::from(vec![
            Span::styled(" File ", Style::default().fg(theme::TEXT)),
            Span::styled(" View ", Style::default().fg(theme::TEXT)),
            Span::styled(" Navigate ", Style::default().fg(theme::TEXT)),
            Span::styled(" Search ", Style::default().fg(theme::TEXT)),
            Span::styled(" Help ", Style::default().fg(theme::TEXT)),
            Span::styled(
                format!("  {}", self.document.label()),
                Style::default().fg(theme::SUBTEXT),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(title).style(Style::default().bg(theme::BASE)),
            area,
        );
    }

    fn draw_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let inner = Block::default()
            .title(" SECTIONS ")
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(theme::OVERLAY))
            .inner(area);
        self.last_navigation_area = inner;

        self.keep_selected_navigation_visible(usize::from(inner.height));
        let lines = self
            .document
            .navigation()
            .iter()
            .enumerate()
            .skip(self.navigation_scroll)
            .take(usize::from(inner.height))
            .map(|(index, item)| {
                let marker = if index == self.selected {
                    "› "
                } else {
                    "· "
                };
                let mut line = Line::from(format!(
                    "{}{}{}",
                    "  ".repeat(item.depth),
                    marker,
                    item.title
                ));
                let style = if index == self.selected {
                    Style::default()
                        .fg(theme::YELLOW)
                        .bg(theme::SELECTED)
                        .add_modifier(Modifier::BOLD)
                } else if item.depth == 0 {
                    Style::default().fg(theme::TEXT)
                } else {
                    Style::default().fg(theme::BLUE)
                };
                line.style = style;
                line
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(Text::from(lines)).block(
                Block::default()
                    .title(" SECTIONS ")
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(theme::OVERLAY)),
            ),
            area,
        );
    }

    fn draw_content(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let inner = area.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        });
        self.last_content_area = inner;
        let rendered = self
            .rendered_cache
            .entry(inner.width)
            .or_insert_with(|| self.document.render(inner.width));
        let viewport_height = usize::from(inner.height);
        let maximum = rendered.row_count.saturating_sub(viewport_height);
        self.content_scroll = self.content_scroll.min(maximum);
        let scroll = u16::try_from(self.content_scroll).unwrap_or(u16::MAX);
        frame.render_widget(
            Paragraph::new(rendered.text.clone()).scroll((scroll, 0)),
            inner,
        );
    }

    fn draw_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let current = self
            .document
            .navigation()
            .get(self.selected)
            .map_or("document", |item| item.title.as_str());
        let status = Line::from(vec![
            Span::styled(
                format!(
                    " {}/{} · {current}",
                    self.selected + 1,
                    self.document.navigation().len()
                ),
                Style::default().fg(theme::TEXT),
            ),
            Span::styled(
                "   j/k navigate · PgUp/PgDn scroll · b sidebar · q quit",
                Style::default().fg(theme::SUBTEXT),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(status).style(Style::default().bg(theme::BASE)),
            area,
        );
    }

    fn select_relative(&mut self, delta: isize) {
        let len = self.document.navigation().len();
        if len == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        self.scroll_to_selected();
    }

    fn scroll_to_selected(&mut self) {
        let Some(item) = self.document.navigation().get(self.selected) else {
            return;
        };
        let width = self.last_content_area.width.max(1);
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        if let Some(row) = rendered.anchor_row(&item.id) {
            self.content_scroll = row;
        }
    }

    fn scroll_content(&mut self, delta: isize) {
        self.content_scroll = self.content_scroll.saturating_add_signed(delta);
        self.sync_selection_to_scroll();
    }

    fn sync_selection_to_scroll(&mut self) {
        let width = self.last_content_area.width.max(1);
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        for (index, item) in self.document.navigation().iter().enumerate() {
            let Some(row) = rendered.anchor_row(&item.id) else {
                continue;
            };
            if row > self.content_scroll {
                break;
            }
            self.selected = index;
        }
    }

    fn keep_selected_navigation_visible(&mut self, height: usize) {
        if self.selected < self.navigation_scroll {
            self.navigation_scroll = self.selected;
        } else if self.selected >= self.navigation_scroll.saturating_add(height) {
            self.navigation_scroll = self.selected.saturating_add(1).saturating_sub(height);
        }
    }
}

#[cfg(test)]
mod tests {
    use mant_ast::{QueryBundle, QuerySchema};
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn empty_bundle() -> QueryBundle {
        QueryBundle {
            schema: QuerySchema::V3,
            label: "demo".to_owned(),
            document: None,
            tldr: None,
        }
    }

    #[test]
    fn renders_the_application_chrome_in_a_test_backend() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&empty_bundle());

        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let screen = terminal.backend().to_string();

        assert!(screen.contains("File"));
        assert!(screen.contains("SECTIONS"));
        assert!(screen.contains("j/k navigate"));
    }

    #[test]
    fn q_requests_a_clean_exit() {
        let mut app = App::new(&empty_bundle());
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit());
    }
}
