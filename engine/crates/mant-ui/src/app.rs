//! Interactive state machine and Ratatui widget composition.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ast::QueryBundle;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::UnicodeWidthChar;

use crate::{DocumentView, NavKind, RenderedDocument, theme};

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
        let style = Style::default().bg(theme::MENU);
        let menu_width = " File View Navigate Search Help "
            .chars()
            .map(|character| character.width().unwrap_or(0))
            .sum::<usize>();
        let rule = "─".repeat(usize::from(area.width).saturating_sub(menu_width));
        frame.render_widget(Block::default().style(style), area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" File ", style.fg(theme::SUBTEXT_BRIGHT)),
                Span::styled("View ", style.fg(theme::SUBTEXT_BRIGHT)),
                Span::styled("Navigate ", style.fg(theme::SUBTEXT_BRIGHT)),
                Span::styled("Search ", style.fg(theme::SUBTEXT_BRIGHT)),
                Span::styled("Help ", style.fg(theme::SUBTEXT_BRIGHT)),
                Span::styled(rule, style.fg(theme::BORDER)),
            ]))
            .style(style),
            area,
        );
        frame.render_widget(
            Paragraph::new(format!("{} ", self.document.label()))
                .alignment(Alignment::Right)
                .style(style.fg(theme::SUBTEXT)),
            area,
        );
    }

    fn draw_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let sidebar = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(theme::BORDER))
            .style(Style::default().bg(theme::SIDEBAR));
        let inner = sidebar.inner(area);
        frame.render_widget(sidebar, area);
        let [header_area, section_label_area, navigation_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(inner);

        let mut metadata = format!(
            "{} top-level · {} sections",
            self.document.top_level_count(),
            self.document.section_count()
        );
        if self.document.has_tldr() {
            metadata.push_str(" · TLDR");
        }
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!(
                        " {} · {}",
                        self.document.source_label(),
                        self.document.label()
                    ),
                    Style::default().fg(theme::SUBTEXT_BRIGHT),
                )),
                Line::from(Span::styled(
                    format!(" {metadata}"),
                    Style::default().fg(theme::SUBTEXT),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(theme::BORDER)),
            )
            .style(Style::default().bg(theme::SIDEBAR)),
            header_area,
        );
        frame.render_widget(
            Paragraph::new(" SECTIONS")
                .style(Style::default().fg(theme::SUBTEXT).bg(theme::SIDEBAR)),
            section_label_area,
        );

        self.last_navigation_area = navigation_area;
        self.keep_selected_navigation_visible(usize::from(navigation_area.height));
        let line_width = usize::from(navigation_area.width);
        let lines = self
            .document
            .navigation()
            .iter()
            .enumerate()
            .skip(self.navigation_scroll)
            .take(usize::from(navigation_area.height))
            .map(|(index, item)| navigation_line(item, index == self.selected, line_width))
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(Text::from(lines)).style(Style::default().bg(theme::SIDEBAR)),
            navigation_area,
        );
    }

    fn draw_content(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::CONTENT)),
            area,
        );
        let inner = area.inner(Margin {
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
            Paragraph::new(rendered.text.clone())
                .scroll((scroll, 0))
                .style(Style::default().bg(theme::CONTENT)),
            inner,
        );
        if rendered.row_count > viewport_height {
            let mut state = ScrollbarState::new(rendered.row_count).position(self.content_scroll);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .thumb_symbol("█")
                    .track_symbol(None)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .style(Style::default().fg(theme::OVERLAY)),
                area,
                &mut state,
            );
        }
    }

    fn draw_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let current = self
            .document
            .navigation()
            .get(self.selected)
            .map_or("document", |item| item.title.as_str());
        let style = Style::default().bg(theme::BASE);
        let selected_position =
            usize::from(!self.document.navigation().is_empty()) * (self.selected + 1);
        frame.render_widget(Block::default().style(style), area);
        frame.render_widget(
            Paragraph::new(format!(
                " {}/{} · {current}",
                selected_position,
                self.document.navigation().len()
            ))
            .style(style.fg(theme::TEXT)),
            area,
        );
        let suffix = if self.document.has_tldr() {
            format!("{} visible sections · TLDR ", self.document.section_count())
        } else {
            format!("{} visible sections ", self.document.section_count())
        };
        frame.render_widget(
            Paragraph::new(suffix)
                .alignment(Alignment::Right)
                .style(style.fg(theme::SUBTEXT)),
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

fn navigation_line(item: &crate::NavItem, selected: bool, width: usize) -> Line<'static> {
    let selection = if selected { "› " } else { "  " };
    let tree = if item.kind == NavKind::Tldr {
        "◆ ".to_owned()
    } else if item.depth == 0 {
        if item.has_children {
            "▾ ".to_owned()
        } else {
            "· ".to_owned()
        }
    } else {
        let mut prefix = "│  ".repeat(item.depth.saturating_sub(1));
        prefix.push_str(if item.is_last { "╰─" } else { "├─" });
        prefix.push_str(if item.has_children { "▾ " } else { "· " });
        prefix
    };
    let value = fit_to_width(&format!("{selection}{tree}{}", item.title), width);
    let foreground = if selected {
        if item.kind == NavKind::Tldr {
            theme::MAUVE
        } else {
            theme::SELECTED_TEXT
        }
    } else {
        match item.kind {
            NavKind::Tldr => theme::MAUVE,
            NavKind::Root | NavKind::Section if item.depth == 0 => theme::SUBTEXT_BRIGHT,
            NavKind::Root | NavKind::Section => theme::BLUE,
        }
    };
    let background = if selected {
        if item.kind == NavKind::Tldr {
            theme::TLDR_SELECTED
        } else {
            theme::SELECTED
        }
    } else if item.kind == NavKind::Tldr {
        theme::TLDR_NAV
    } else {
        theme::SIDEBAR
    };
    let mut style = Style::default().fg(foreground).bg(background);
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }
    Line::from(Span::styled(value, style))
}

fn fit_to_width(value: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        used += character_width;
        result.push(character);
    }
    result.push_str(&" ".repeat(width.saturating_sub(used)));
    result
}

#[cfg(test)]
mod tests {
    use mant_ast::{QueryBundle, QuerySchema, TldrDocument, TldrOrigin};
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

    fn tldr_bundle() -> QueryBundle {
        QueryBundle {
            schema: QuerySchema::V3,
            label: "demo".to_owned(),
            document: None,
            tldr: Some(TldrDocument {
                title: "demo".to_owned(),
                description: vec!["A polished quick reference".to_owned()],
                more_information: None,
                examples: Vec::new(),
                platform: "common".to_owned(),
                language: "en".to_owned(),
                source_path: "demo.md".to_owned(),
                origin: TldrOrigin::TldrPages,
            }),
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
        assert!(screen.contains("MANUAL · demo"));
        assert!(screen.contains("0 visible sections"));
    }

    #[test]
    fn q_requests_a_clean_exit() {
        let mut app = App::new(&empty_bundle());
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit());
    }

    #[test]
    fn preserves_the_established_menu_sidebar_and_tldr_surfaces() {
        let backend = TestBackend::new(80, 14);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&tldr_bundle());

        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer.cell((0, 0)).expect("menu cell").bg, theme::MENU);
        assert_eq!(
            buffer.cell((0, 1)).expect("sidebar cell").bg,
            theme::SIDEBAR
        );
        assert_eq!(
            buffer.cell((0, 5)).expect("selected tldr navigation").bg,
            theme::TLDR_SELECTED
        );
        assert_eq!(
            buffer.cell((37, 2)).expect("tldr panel border").bg,
            theme::TLDR_SURFACE
        );
    }
}
