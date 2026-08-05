//! Interactive state machine and Ratatui widget composition.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ast::QueryBundle;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{DocumentView, NavKind, RenderedDocument, RenderedSearchMatch, theme};

const MIN_SIDEBAR_WIDTH: u16 = 20;
const MAX_SIDEBAR_WIDTH: u16 = 60;
const NAVIGATION_SYNC_IDLE: Duration = Duration::from_millis(140);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Closed,
    Open { editing: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuId {
    File,
    View,
    Navigate,
    Search,
    Help,
}

impl MenuId {
    const ALL: [Self; 5] = [
        Self::File,
        Self::View,
        Self::Navigate,
        Self::Search,
        Self::Help,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::View => "View",
            Self::Navigate => "Navigate",
            Self::Search => "Search",
            Self::Help => "Help",
        }
    }

    const fn left(self) -> u16 {
        match self {
            Self::File => 0,
            Self::View => 6,
            Self::Navigate => 12,
            Self::Search => 22,
            Self::Help => 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    Menu { id: MenuId, cursor: usize },
    Help,
}

#[derive(Debug, Clone, Copy)]
struct MenuEntry {
    label: &'static str,
    shortcut: &'static str,
    action: MenuAction,
}

#[derive(Debug, Clone, Copy)]
enum MenuAction {
    Quit,
    ToggleSidebar,
    ResetSidebar,
    ExpandAll,
    CollapseAll,
    Previous,
    Next,
    Parent,
    FirstChild,
    First,
    Last,
    Find,
    FindNext,
    FindPrevious,
    Help,
}

impl SearchMode {
    const fn is_open(self) -> bool {
        matches!(self, Self::Open { .. })
    }

    const fn is_editing(self) -> bool {
        matches!(self, Self::Open { editing: true })
    }
}

/// All mutable interaction state for one `ManT` document.
pub struct App {
    document: DocumentView,
    selected: usize,
    expanded: HashSet<String>,
    content_scroll: usize,
    navigation_scroll: usize,
    navigation_visibility_target: Option<usize>,
    sidebar_width: u16,
    show_sidebar: bool,
    quit: bool,
    search_mode: SearchMode,
    search_draft: String,
    search_cursor: usize,
    search_query: String,
    search_matches: Vec<RenderedSearchMatch>,
    active_search_match: usize,
    search_width: u16,
    overlay: Overlay,
    resizing_sidebar: bool,
    last_body_area: Rect,
    last_content_area: Rect,
    last_navigation_area: Rect,
    last_navigation_rows: Vec<usize>,
    navigation_sync_deadline: Option<Instant>,
    rendered_cache: HashMap<u16, RenderedDocument>,
}

impl App {
    #[must_use]
    pub fn new(bundle: &QueryBundle) -> Self {
        let document = DocumentView::new(bundle);
        let expanded = document
            .navigation()
            .iter()
            .filter(|item| item.kind == NavKind::Section && item.depth == 0)
            .map(|item| item.id.clone())
            .collect();
        Self {
            document,
            selected: 0,
            expanded,
            content_scroll: 0,
            navigation_scroll: 0,
            navigation_visibility_target: Some(0),
            sidebar_width: 36,
            show_sidebar: true,
            quit: false,
            search_mode: SearchMode::Closed,
            search_draft: String::new(),
            search_cursor: 0,
            search_query: String::new(),
            search_matches: Vec::new(),
            active_search_match: 0,
            search_width: 0,
            overlay: Overlay::None,
            resizing_sidebar: false,
            last_body_area: Rect::default(),
            last_content_area: Rect::default(),
            last_navigation_area: Rect::default(),
            last_navigation_rows: Vec::new(),
            navigation_sync_deadline: None,
            rendered_cache: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        if self
            .navigation_sync_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.navigation_sync_deadline = None;
            self.sync_selection_to_scroll();
        }
    }

    pub(crate) fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        self.navigation_sync_deadline
            .map(|deadline| deadline.saturating_duration_since(now))
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.overlay != Overlay::None {
            self.handle_overlay_key(key);
            return;
        }
        if self.search_mode.is_open() {
            self.handle_search_key(key);
            return;
        }
        if key.code == KeyCode::F(10) {
            self.open_menu(MenuId::File);
            return;
        }
        if (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('f'))
            || key.code == KeyCode::Char('/')
        {
            self.open_search();
            return;
        }
        if key.code == KeyCode::Char('?') {
            self.overlay = Overlay::Help;
            return;
        }
        if key.code == KeyCode::Char('n') && !self.search_matches.is_empty() {
            self.select_search_relative(if key.modifiers.contains(KeyModifiers::SHIFT) {
                -1
            } else {
                1
            });
            return;
        }
        match key.code {
            KeyCode::Char('q' | 'Q') => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.select_relative(1),
            KeyCode::Char('k') | KeyCode::Up => self.select_relative(-1),
            KeyCode::Char('h') | KeyCode::Left => self.collapse_or_select_parent(),
            KeyCode::Char('l') | KeyCode::Right => self.expand_or_select_child(),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_selected(),
            KeyCode::PageDown | KeyCode::Char('d') => self.scroll_content(10),
            KeyCode::PageUp | KeyCode::Char('u') => self.scroll_content(-10),
            KeyCode::Home => self.jump_content(false),
            KeyCode::End => self.jump_content(true),
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
        if self.handle_overlay_mouse(mouse) {
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if self.is_sidebar_boundary(mouse.column) => {
                self.resizing_sidebar = true;
            }
            MouseEventKind::Drag(MouseButton::Left) if self.resizing_sidebar => {
                self.resize_sidebar_to(mouse.column);
            }
            MouseEventKind::Up(MouseButton::Left) if self.resizing_sidebar => {
                self.resize_sidebar_to(mouse.column);
                self.resizing_sidebar = false;
            }
            MouseEventKind::ScrollDown
                if self
                    .last_navigation_area
                    .contains((mouse.column, mouse.row).into()) =>
            {
                self.navigation_scroll = self.navigation_scroll.saturating_add(3);
            }
            MouseEventKind::ScrollUp
                if self
                    .last_navigation_area
                    .contains((mouse.column, mouse.row).into()) =>
            {
                self.navigation_scroll = self.navigation_scroll.saturating_sub(3);
            }
            MouseEventKind::ScrollDown => self.scroll_content(3),
            MouseEventKind::ScrollUp => self.scroll_content(-3),
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .last_navigation_area
                    .contains((mouse.column, mouse.row).into()) =>
            {
                let local_row = usize::from(mouse.row - self.last_navigation_area.y);
                if let Some(index) = self.last_navigation_rows.get(local_row).copied() {
                    if self.selected == index && self.document.navigation()[index].has_children {
                        self.toggle_selected();
                    } else {
                        self.set_selected_index(index);
                        if self.document.navigation()[index].has_children {
                            self.expanded
                                .insert(self.document.navigation()[index].id.clone());
                        }
                        self.scroll_to_selected();
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .last_content_area
                    .contains((mouse.column, mouse.row).into()) =>
            {
                self.activate_content_link(mouse.column, mouse.row);
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
        self.last_body_area = body_area;
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
            self.last_navigation_rows.clear();
            self.resizing_sidebar = false;
            self.draw_content(frame, body_area);
        }
        if self.search_mode.is_open() {
            self.draw_search(frame, status_area);
        } else {
            self.draw_status(frame, status_area);
        }
        self.draw_overlay(frame);
    }

    fn open_menu(&mut self, id: MenuId) {
        self.close_search();
        self.overlay = Overlay::Menu { id, cursor: 0 };
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) {
        match self.overlay {
            Overlay::None => {}
            Overlay::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::Menu { id, cursor } => match key.code {
                KeyCode::Esc | KeyCode::F(10) => self.overlay = Overlay::None,
                KeyCode::Left | KeyCode::Right => {
                    let current = MenuId::ALL
                        .iter()
                        .position(|candidate| *candidate == id)
                        .unwrap_or_default();
                    let delta = if key.code == KeyCode::Left { -1 } else { 1 };
                    let length = isize::try_from(MenuId::ALL.len()).unwrap_or_default();
                    let next = usize::try_from(
                        (isize::try_from(current).unwrap_or_default() + delta).rem_euclid(length),
                    )
                    .unwrap_or_default();
                    self.overlay = Overlay::Menu {
                        id: MenuId::ALL[next],
                        cursor: 0,
                    };
                }
                KeyCode::Down | KeyCode::Up => {
                    let length = isize::try_from(menu_entries(id).len()).unwrap_or_default();
                    let delta = if key.code == KeyCode::Up { -1 } else { 1 };
                    let next = usize::try_from(
                        (isize::try_from(cursor).unwrap_or_default() + delta).rem_euclid(length),
                    )
                    .unwrap_or_default();
                    self.overlay = Overlay::Menu { id, cursor: next };
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if let Some(entry) = menu_entries(id).get(cursor) {
                        self.activate_menu_action(entry.action);
                    }
                }
                _ => {}
            },
        }
    }

    fn handle_overlay_mouse(&mut self, mouse: MouseEvent) -> bool {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return self.overlay != Overlay::None;
        }
        if mouse.row == 0
            && let Some(id) = MenuId::ALL.into_iter().find(|id| {
                let start = id.left();
                let end = start + u16::try_from(id.label().len() + 2).unwrap_or_default();
                mouse.column >= start && mouse.column < end
            })
        {
            self.close_search();
            self.overlay = if matches!(self.overlay, Overlay::Menu { id: open, .. } if open == id) {
                Overlay::None
            } else {
                Overlay::Menu { id, cursor: 0 }
            };
            return true;
        }
        if let Overlay::Menu { id, .. } = self.overlay {
            let entries = menu_entries(id);
            let row = usize::from(mouse.row.saturating_sub(1));
            if mouse.row >= 1
                && row < entries.len()
                && mouse.column >= id.left()
                && mouse.column < id.left().saturating_add(30)
            {
                self.activate_menu_action(entries[row].action);
            } else {
                self.overlay = Overlay::None;
            }
            return true;
        }
        self.overlay == Overlay::Help
    }

    fn activate_menu_action(&mut self, action: MenuAction) {
        self.overlay = Overlay::None;
        match action {
            MenuAction::Quit => self.quit = true,
            MenuAction::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            MenuAction::ResetSidebar => self.sidebar_width = 36,
            MenuAction::ExpandAll => {
                self.expanded = self
                    .document
                    .navigation()
                    .iter()
                    .filter(|item| item.has_children)
                    .map(|item| item.id.clone())
                    .collect();
            }
            MenuAction::CollapseAll => {
                self.expanded.clear();
                self.select_nearest_visible_ancestor();
            }
            MenuAction::Previous => self.select_relative(-1),
            MenuAction::Next => self.select_relative(1),
            MenuAction::Parent => self.collapse_or_select_parent(),
            MenuAction::FirstChild => self.expand_or_select_child(),
            MenuAction::First => self.select_edge(false),
            MenuAction::Last => self.select_edge(true),
            MenuAction::Find => self.open_search(),
            MenuAction::FindNext => self.select_search_relative(1),
            MenuAction::FindPrevious => self.select_search_relative(-1),
            MenuAction::Help => self.overlay = Overlay::Help,
        }
    }

    fn select_edge(&mut self, last: bool) {
        let visible = self.visible_navigation_indices();
        let selected = if last {
            visible.last()
        } else {
            visible.first()
        };
        if let Some(index) = selected.copied() {
            self.set_selected_index(index);
            self.scroll_to_selected();
        }
    }

    fn draw_menu(&self, frame: &mut Frame<'_>, area: Rect) {
        let style = Style::default().bg(theme::MENU);
        let menu_width = 36;
        let rule = "─".repeat(usize::from(area.width).saturating_sub(menu_width));
        frame.render_widget(Block::default().style(style), area);
        let open_menu = match self.overlay {
            Overlay::Menu { id, .. } => Some(id),
            Overlay::None | Overlay::Help => None,
        };
        let mut spans = MenuId::ALL
            .into_iter()
            .map(|id| {
                let active = open_menu == Some(id);
                Span::styled(
                    format!(" {} ", id.label()),
                    if active {
                        style.fg(theme::SELECTED_TEXT).bg(theme::SELECTED)
                    } else {
                        style.fg(theme::SUBTEXT_BRIGHT)
                    },
                )
            })
            .collect::<Vec<_>>();
        spans.push(Span::styled(rule, style.fg(theme::BORDER)));
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
        frame.render_widget(
            Paragraph::new(format!("{} ", self.document.label()))
                .alignment(Alignment::Right)
                .style(style.fg(theme::SUBTEXT)),
            area,
        );
    }

    fn draw_overlay(&self, frame: &mut Frame<'_>) {
        match self.overlay {
            Overlay::None => {}
            Overlay::Menu { id, cursor } => {
                let entries = menu_entries(id);
                let height = u16::try_from(entries.len())
                    .unwrap_or_default()
                    .saturating_add(1);
                let area = Rect::new(
                    id.left().min(frame.area().width.saturating_sub(1)),
                    1,
                    30.min(frame.area().width.saturating_sub(id.left())),
                    height.min(frame.area().height.saturating_sub(1)),
                );
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Block::default()
                        .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                        .border_style(Style::default().fg(theme::OVERLAY))
                        .style(Style::default().bg(theme::BASE)),
                    area,
                );
                let inner = Rect::new(
                    area.x.saturating_add(1),
                    area.y,
                    area.width.saturating_sub(2),
                    area.height.saturating_sub(1),
                );
                for (index, entry) in entries.iter().enumerate().take(usize::from(inner.height)) {
                    let row = Rect::new(
                        inner.x,
                        inner.y + u16::try_from(index).unwrap_or_default(),
                        inner.width,
                        1,
                    );
                    let active = index == cursor;
                    let checked =
                        matches!(entry.action, MenuAction::ToggleSidebar) && self.show_sidebar;
                    let prefix = if checked { "✓ " } else { "  " };
                    let label = format!("{prefix}{}", entry.label);
                    let gap = usize::from(row.width)
                        .saturating_sub(label.width())
                        .saturating_sub(entry.shortcut.width());
                    let value = fit_to_width(
                        &format!(
                            "{label}{}{shortcut}",
                            " ".repeat(gap),
                            shortcut = entry.shortcut
                        ),
                        usize::from(row.width),
                    );
                    let style = if active {
                        Style::default()
                            .fg(theme::SELECTED_TEXT)
                            .bg(theme::SELECTED)
                    } else {
                        Style::default().fg(theme::TEXT).bg(theme::BASE)
                    };
                    frame.render_widget(Paragraph::new(Span::styled(value, style)), row);
                }
            }
            Overlay::Help => Self::draw_help(frame),
        }
    }

    fn draw_help(frame: &mut Frame<'_>) {
        let width = 58.min(frame.area().width.saturating_sub(2));
        let height = 13.min(frame.area().height);
        if width < 4 || height < 3 {
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
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BLUE))
            .style(Style::default().bg(theme::BASE));
        let inner = block.inner(area).inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    "Keyboard Shortcuts",
                    Style::default()
                        .fg(theme::BLUE)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::raw("↑/↓ or j/k  select section"),
                Line::raw("←/→ or h/l  move through the section tree"),
                Line::raw("Enter        fold or unfold selected section"),
                Line::raw("Ctrl+F or /  find in current page"),
                Line::raw("n / N        next / previous search match"),
                Line::raw("d/u          scroll content by ten rows"),
                Line::raw("b            toggle sidebar"),
                Line::raw("F10          open menu bar"),
                Line::raw("q            quit"),
                Line::raw(""),
                Line::styled(
                    "Esc or ? closes this window",
                    Style::default().fg(theme::SUBTEXT),
                ),
            ])
            .style(Style::default().fg(theme::TEXT).bg(theme::BASE)),
            inner,
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
        let visible = self.visible_navigation_indices();
        let line_width = usize::from(navigation_area.width);
        let rows = navigation_rows(
            self.document.navigation(),
            &visible,
            self.selected,
            &self.expanded,
            line_width,
        );
        let height = usize::from(navigation_area.height);
        self.navigation_scroll = self
            .navigation_scroll
            .min(rows.len().saturating_sub(height));
        if self.navigation_visibility_target.take() == Some(self.selected) {
            let selected_row = rows
                .iter()
                .position(|row| row.item_index == self.selected)
                .unwrap_or_default();
            self.keep_selected_navigation_visible(selected_row, height);
        }
        let visible_rows = rows
            .into_iter()
            .skip(self.navigation_scroll)
            .take(height)
            .collect::<Vec<_>>();
        self.last_navigation_rows = visible_rows.iter().map(|row| row.item_index).collect();
        let lines = visible_rows
            .into_iter()
            .map(|row| row.line)
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
        self.rendered_cache
            .entry(inner.width)
            .or_insert_with(|| self.document.render(inner.width));
        if !self.search_query.is_empty() && self.search_width != inner.width {
            self.refresh_search(inner.width);
        }
        let rendered = &self.rendered_cache[&inner.width];
        let viewport_height = usize::from(inner.height);
        let maximum = rendered.row_count.saturating_sub(viewport_height);
        self.content_scroll = self.content_scroll.min(maximum);
        let scroll = u16::try_from(self.content_scroll).unwrap_or(u16::MAX);
        let text = if self.search_query.is_empty() {
            rendered.text.clone()
        } else {
            rendered.highlighted_text(
                &self.search_matches,
                (!self.search_matches.is_empty()).then_some(self.active_search_match),
            )
        };
        frame.render_widget(
            Paragraph::new(text)
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
        let visible = self.visible_navigation_indices();
        let selected_position = visible
            .iter()
            .position(|index| *index == self.selected)
            .map_or(0, |index| index + 1);
        let style = Style::default().bg(theme::BASE);
        frame.render_widget(Block::default().style(style), area);
        frame.render_widget(
            Paragraph::new(format!(
                " {}/{} · {current}",
                selected_position,
                visible.len()
            ))
            .style(style.fg(theme::TEXT)),
            area,
        );
        let suffix = if !self.search_query.is_empty() && !self.search_matches.is_empty() {
            format!(
                "Find “{}” · {} matches ",
                self.search_query,
                self.search_matches.len()
            )
        } else if self.document.has_tldr() {
            format!("{} visible sections · TLDR ", self.visible_section_count())
        } else {
            format!("{} visible sections ", self.visible_section_count())
        };
        frame.render_widget(
            Paragraph::new(suffix)
                .alignment(Alignment::Right)
                .style(style.fg(theme::SUBTEXT)),
            area,
        );
    }

    fn draw_search(&self, frame: &mut Frame<'_>, area: Rect) {
        let style = Style::default().bg(theme::MENU);
        frame.render_widget(Block::default().style(style), area);
        let (before_cursor, after_cursor) = self.search_draft.split_at(self.search_cursor);
        let cursor_character = after_cursor.chars().next();
        let cursor_bytes = cursor_character.map_or(0, char::len_utf8);
        let after_cursor = &after_cursor[cursor_bytes..];
        let prompt = format!(" Find: {}", self.search_draft);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Find: ", style.fg(theme::YELLOW)),
                Span::styled(
                    before_cursor.to_owned(),
                    style.fg(theme::TEXT).bg(theme::SURFACE),
                ),
                Span::styled(
                    cursor_character.map_or_else(|| " ".to_owned(), |value| value.to_string()),
                    style.fg(theme::BASE).bg(theme::TEXT),
                ),
                Span::styled(
                    after_cursor.to_owned(),
                    style.fg(theme::TEXT).bg(theme::SURFACE),
                ),
            ])),
            area,
        );
        let suffix = if !self.search_mode.is_editing() && !self.search_query.is_empty() {
            if self.search_matches.is_empty() {
                " No matches · Edit query · Esc close ".to_owned()
            } else {
                format!(
                    " {}/{} · Enter next · Esc close ",
                    self.active_search_match + 1,
                    self.search_matches.len()
                )
            }
        } else {
            " Enter search · Esc cancel ".to_owned()
        };
        let suffix_style = if self.search_matches.is_empty()
            && !self.search_mode.is_editing()
            && !self.search_query.is_empty()
        {
            style
                .fg(theme::BASE)
                .bg(theme::PEACH)
                .add_modifier(Modifier::BOLD)
        } else {
            style.fg(theme::SUBTEXT)
        };
        let prompt_width = prompt.width();
        if prompt_width + suffix.width() < usize::from(area.width) {
            frame.render_widget(
                Paragraph::new(Span::styled(suffix, suffix_style)).alignment(Alignment::Right),
                area,
            );
        }
    }

    fn select_relative(&mut self, delta: isize) {
        let visible = self.visible_navigation_indices();
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|index| *index == self.selected)
            .unwrap_or_default();
        let next = current.saturating_add_signed(delta).min(visible.len() - 1);
        self.set_selected_index(visible[next]);
        self.scroll_to_selected();
    }

    fn set_selected_index(&mut self, index: usize) {
        self.selected = index;
        self.navigation_visibility_target = Some(index);
    }

    fn open_search(&mut self) {
        self.search_mode = SearchMode::Open { editing: false };
        self.search_draft.clone_from(&self.search_query);
        self.search_cursor = self.search_draft.len();
    }

    fn close_search(&mut self) {
        self.search_mode = SearchMode::Closed;
        self.search_draft.clear();
        self.search_cursor = 0;
        self.search_query.clear();
        self.search_matches.clear();
        self.active_search_match = 0;
        self.search_width = 0;
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_search(),
            KeyCode::Enter => {
                if !self.search_mode.is_editing() && self.search_draft == self.search_query {
                    self.select_search_relative(1);
                } else {
                    self.search_query.clone_from(&self.search_draft);
                    self.search_mode = SearchMode::Open { editing: false };
                    self.refresh_search(self.last_content_area.width.max(1));
                    self.select_active_search_match();
                }
            }
            KeyCode::Backspace => {
                if let Some(previous) =
                    previous_char_boundary(&self.search_draft, self.search_cursor)
                {
                    self.search_draft.drain(previous..self.search_cursor);
                    self.search_cursor = previous;
                    self.search_mode = SearchMode::Open { editing: true };
                }
            }
            KeyCode::Delete => {
                if let Some(next) = next_char_boundary(&self.search_draft, self.search_cursor) {
                    self.search_draft.drain(self.search_cursor..next);
                    self.search_mode = SearchMode::Open { editing: true };
                }
            }
            KeyCode::Left => {
                self.search_cursor = previous_char_boundary(&self.search_draft, self.search_cursor)
                    .unwrap_or_default();
            }
            KeyCode::Right => {
                self.search_cursor = next_char_boundary(&self.search_draft, self.search_cursor)
                    .unwrap_or(self.search_draft.len());
            }
            KeyCode::Home => self.search_cursor = 0,
            KeyCode::End => self.search_cursor = self.search_draft.len(),
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_cursor = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_cursor = self.search_draft.len();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_draft.clear();
                self.search_cursor = 0;
                self.search_mode = SearchMode::Open { editing: true };
            }
            KeyCode::Down if !self.search_mode.is_editing() => self.select_search_relative(1),
            KeyCode::Up if !self.search_mode.is_editing() => self.select_search_relative(-1),
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_draft.insert(self.search_cursor, character);
                self.search_cursor += character.len_utf8();
                self.search_mode = SearchMode::Open { editing: true };
            }
            _ => {}
        }
    }

    fn refresh_search(&mut self, width: u16) {
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        self.search_matches = rendered.search(&self.search_query);
        self.active_search_match = self
            .active_search_match
            .min(self.search_matches.len().saturating_sub(1));
        self.search_width = width;
    }

    fn select_search_relative(&mut self, delta: isize) {
        if self.search_matches.is_empty() {
            return;
        }
        let length = isize::try_from(self.search_matches.len()).unwrap_or(isize::MAX);
        let current = isize::try_from(self.active_search_match).unwrap_or_default();
        self.active_search_match =
            usize::try_from((current + delta).rem_euclid(length)).unwrap_or_default();
        self.select_active_search_match();
    }

    fn select_active_search_match(&mut self) {
        let Some(search_match) = self.search_matches.get(self.active_search_match).cloned() else {
            return;
        };
        self.content_scroll = search_match.row;
        self.select_section_at_row(search_match.row);
    }

    fn select_section_at_row(&mut self, row: usize) {
        let width = self.last_content_area.width.max(1);
        let visible = self.visible_navigation_indices();
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        let mut selected = None;
        for index in visible {
            let item = &self.document.navigation()[index];
            if !matches!(item.kind, NavKind::Tldr | NavKind::Root | NavKind::Section) {
                continue;
            }
            let Some(anchor_row) = rendered.anchor_row(&item.target_id) else {
                continue;
            };
            if anchor_row > row {
                break;
            }
            selected = Some(index);
        }
        if let Some(index) = selected {
            self.set_selected_index(index);
        }
    }

    fn scroll_to_selected(&mut self) {
        self.navigation_sync_deadline = None;
        let Some(item) = self.document.navigation().get(self.selected) else {
            return;
        };
        let width = self.last_content_area.width.max(1);
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        if let Some(row) = rendered.anchor_row(&item.target_id) {
            self.content_scroll = row;
        }
    }

    fn activate_content_link(&mut self, column: u16, row: u16) {
        let width = self.last_content_area.width.max(1);
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        let document_row = self.content_scroll + usize::from(row - self.last_content_area.y);
        let document_column = usize::from(column - self.last_content_area.x);
        let Some(target) = rendered
            .link_target_at(document_row, document_column)
            .map(str::to_owned)
        else {
            return;
        };
        let Some(target_row) = rendered.anchor_row(&target) else {
            return;
        };
        self.content_scroll = target_row;
        if let Some(index) = self
            .document
            .navigation()
            .iter()
            .position(|item| item.id == target)
        {
            self.expand_navigation_ancestors(index);
            self.set_selected_index(index);
        } else {
            self.select_section_at_row(target_row);
        }
    }

    fn expand_navigation_ancestors(&mut self, index: usize) {
        let mut parent = self.document.navigation()[index].parent_id.as_deref();
        while let Some(parent_id) = parent {
            self.expanded.insert(parent_id.to_owned());
            parent = self
                .document
                .navigation()
                .iter()
                .find(|item| item.id == parent_id)
                .and_then(|item| item.parent_id.as_deref());
        }
    }

    fn scroll_content(&mut self, delta: isize) {
        self.content_scroll = self.content_scroll.saturating_add_signed(delta);
        self.navigation_sync_deadline = Some(Instant::now() + NAVIGATION_SYNC_IDLE);
    }

    fn jump_content(&mut self, end: bool) {
        self.content_scroll = if end { usize::MAX } else { 0 };
        self.navigation_sync_deadline = Some(Instant::now() + NAVIGATION_SYNC_IDLE);
    }

    fn sync_selection_to_scroll(&mut self) {
        self.select_section_at_row(self.content_scroll);
    }

    fn keep_selected_navigation_visible(&mut self, selected: usize, height: usize) {
        if selected < self.navigation_scroll {
            self.navigation_scroll = selected;
        } else if selected >= self.navigation_scroll.saturating_add(height) {
            self.navigation_scroll = selected.saturating_add(1).saturating_sub(height);
        }
    }

    fn is_sidebar_boundary(&self, column: u16) -> bool {
        self.show_sidebar
            && self.last_navigation_area.width > 0
            && column.abs_diff(self.last_navigation_area.right()) <= 1
    }

    fn resize_sidebar_to(&mut self, column: u16) {
        let maximum = MAX_SIDEBAR_WIDTH.min(self.last_body_area.width.saturating_sub(20));
        let width = column
            .saturating_sub(self.last_body_area.x)
            .saturating_add(1)
            .clamp(MIN_SIDEBAR_WIDTH, maximum.max(MIN_SIDEBAR_WIDTH));
        self.sidebar_width = width;
    }

    fn visible_navigation_indices(&self) -> Vec<usize> {
        let mut visible_ids = HashSet::new();
        let mut indices = Vec::new();
        for (index, item) in self.document.navigation().iter().enumerate() {
            let visible = item.parent_id.as_ref().is_none_or(|parent| {
                visible_ids.contains(parent) && self.expanded.contains(parent)
            });
            if visible {
                visible_ids.insert(item.id.clone());
                indices.push(index);
            }
        }
        indices
    }

    fn visible_section_count(&self) -> usize {
        self.visible_navigation_indices()
            .into_iter()
            .filter(|index| self.document.navigation()[*index].kind == NavKind::Section)
            .count()
    }

    fn select_nearest_visible_ancestor(&mut self) {
        let visible = self.visible_navigation_indices();
        if visible.contains(&self.selected) {
            return;
        }

        let mut parent = self.document.navigation()[self.selected]
            .parent_id
            .as_deref();
        while let Some(parent_id) = parent {
            if let Some(index) = self
                .document
                .navigation()
                .iter()
                .position(|item| item.id == parent_id)
            {
                if visible.contains(&index) {
                    self.set_selected_index(index);
                    return;
                }
                parent = self.document.navigation()[index].parent_id.as_deref();
            } else {
                break;
            }
        }
        if let Some(index) = visible.first().copied() {
            self.set_selected_index(index);
        }
    }

    fn toggle_selected(&mut self) {
        let Some(item) = self.document.navigation().get(self.selected) else {
            return;
        };
        if !item.has_children {
            return;
        }
        if !self.expanded.remove(&item.id) {
            self.expanded.insert(item.id.clone());
        }
    }

    fn collapse_or_select_parent(&mut self) {
        let Some(item) = self.document.navigation().get(self.selected) else {
            return;
        };
        if item.has_children && self.expanded.remove(&item.id) {
            return;
        }
        let Some(parent_id) = item.parent_id.as_deref() else {
            return;
        };
        if let Some(index) = self
            .document
            .navigation()
            .iter()
            .position(|candidate| candidate.id == parent_id)
        {
            self.set_selected_index(index);
            self.scroll_to_selected();
        }
    }

    fn expand_or_select_child(&mut self) {
        let Some(item) = self.document.navigation().get(self.selected) else {
            return;
        };
        if !item.has_children {
            return;
        }
        if self.expanded.insert(item.id.clone()) {
            return;
        }
        let parent_id = item.id.clone();
        if let Some(index) = self
            .document
            .navigation()
            .iter()
            .position(|candidate| candidate.parent_id.as_deref() == Some(parent_id.as_str()))
        {
            self.set_selected_index(index);
            self.scroll_to_selected();
        }
    }
}

const FILE_MENU: &[MenuEntry] = &[MenuEntry {
    label: "Quit",
    shortcut: "q",
    action: MenuAction::Quit,
}];

const VIEW_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Sidebar",
        shortcut: "",
        action: MenuAction::ToggleSidebar,
    },
    MenuEntry {
        label: "Reset Sidebar Width",
        shortcut: "",
        action: MenuAction::ResetSidebar,
    },
    MenuEntry {
        label: "Expand All",
        shortcut: "",
        action: MenuAction::ExpandAll,
    },
    MenuEntry {
        label: "Collapse All",
        shortcut: "",
        action: MenuAction::CollapseAll,
    },
];

const NAVIGATE_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Previous Section",
        shortcut: "↑ / k",
        action: MenuAction::Previous,
    },
    MenuEntry {
        label: "Next Section",
        shortcut: "↓ / j",
        action: MenuAction::Next,
    },
    MenuEntry {
        label: "Parent Section",
        shortcut: "← / h",
        action: MenuAction::Parent,
    },
    MenuEntry {
        label: "First Child",
        shortcut: "→ / l",
        action: MenuAction::FirstChild,
    },
    MenuEntry {
        label: "First Section",
        shortcut: "",
        action: MenuAction::First,
    },
    MenuEntry {
        label: "Last Section",
        shortcut: "",
        action: MenuAction::Last,
    },
];

const SEARCH_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Find in Page…",
        shortcut: "Ctrl+F / /",
        action: MenuAction::Find,
    },
    MenuEntry {
        label: "Find Next",
        shortcut: "n",
        action: MenuAction::FindNext,
    },
    MenuEntry {
        label: "Find Previous",
        shortcut: "N",
        action: MenuAction::FindPrevious,
    },
];

const HELP_MENU: &[MenuEntry] = &[MenuEntry {
    label: "Keyboard Shortcuts",
    shortcut: "?",
    action: MenuAction::Help,
}];

const fn menu_entries(id: MenuId) -> &'static [MenuEntry] {
    match id {
        MenuId::File => FILE_MENU,
        MenuId::View => VIEW_MENU,
        MenuId::Navigate => NAVIGATE_MENU,
        MenuId::Search => SEARCH_MENU,
        MenuId::Help => HELP_MENU,
    }
}

fn previous_char_boundary(value: &str, cursor: usize) -> Option<usize> {
    value[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn next_char_boundary(value: &str, cursor: usize) -> Option<usize> {
    value[cursor..]
        .chars()
        .next()
        .map(|character| cursor + character.len_utf8())
}

struct NavigationRow {
    item_index: usize,
    line: Line<'static>,
}

fn navigation_rows(
    items: &[crate::NavItem],
    visible: &[usize],
    selected: usize,
    expanded: &HashSet<String>,
    width: usize,
) -> Vec<NavigationRow> {
    visible
        .iter()
        .flat_map(|index| {
            let item = &items[*index];
            navigation_lines(
                item,
                *index,
                *index == selected,
                expanded.contains(&item.id),
                width,
            )
        })
        .collect()
}

fn navigation_lines(
    item: &crate::NavItem,
    item_index: usize,
    selected: bool,
    expanded: bool,
    width: usize,
) -> Vec<NavigationRow> {
    let selection = if selected { "› " } else { "  " };
    let prefix = format!("{selection}{}", navigation_tree_prefix(item, expanded));
    let continuation_prefix = format!("  {}", navigation_continuation_prefix(item, expanded));
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
            NavKind::EntryGroup => theme::YELLOW,
            NavKind::Option => theme::GREEN,
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
    let title_width = width
        .saturating_sub(prefix.width().max(continuation_prefix.width()))
        .max(1);
    let titles = if selected {
        wrap_to_width(&item.title, title_width)
    } else {
        vec![item.title.clone()]
    };
    titles
        .into_iter()
        .enumerate()
        .map(|(line_index, title)| {
            let line_prefix = if line_index == 0 {
                prefix.clone()
            } else {
                continuation_prefix.clone()
            };
            let used = line_prefix.width() + title.width();
            let prefix_color = if selected {
                if line_index == 0 {
                    theme::PEACH
                } else {
                    theme::PINK
                }
            } else {
                theme::OVERLAY
            };
            NavigationRow {
                item_index,
                line: Line::from(vec![
                    Span::styled(
                        line_prefix,
                        Style::default().fg(prefix_color).bg(background),
                    ),
                    Span::styled(title, style),
                    Span::styled(
                        " ".repeat(width.saturating_sub(used)),
                        Style::default().bg(background),
                    ),
                ]),
            }
        })
        .collect()
}

fn navigation_tree_prefix(item: &crate::NavItem, expanded: bool) -> String {
    if item.kind == NavKind::Tldr {
        return "◆ ".to_owned();
    }
    let mut prefix = "│ ".repeat(item.depth);
    if item.depth == 0 {
        if item.has_children {
            prefix.push_str("│ ");
        }
    } else {
        prefix.push_str(if item.is_last && !expanded {
            "╰─"
        } else {
            "├─"
        });
    }
    prefix.push_str(if item.has_children {
        if expanded { "▾ " } else { "▸ " }
    } else if item.kind == NavKind::Option {
        "◇ "
    } else {
        "· "
    });
    prefix
}

fn navigation_continuation_prefix(item: &crate::NavItem, expanded: bool) -> String {
    if item.kind == NavKind::Tldr {
        return "  ".to_owned();
    }
    let mut prefix = "│ ".repeat(item.depth);
    if item.depth > 0 || item.has_children {
        prefix.push_str("│ ");
    }
    if item.has_children && expanded {
        prefix.push_str("│ ");
    }
    prefix.push_str("  ");
    prefix
}

fn wrap_to_width(value: &str, width: usize) -> Vec<String> {
    if value.width() <= width {
        return vec![value.to_owned()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        let separator = usize::from(!current.is_empty());
        if current.width() + separator + word.width() <= width {
            if separator == 1 {
                current.push(' ');
            }
            current.push_str(word);
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        let mut remaining = word;
        while remaining.width() > width {
            let split = byte_index_at_width(remaining, width);
            lines.push(remaining[..split].to_owned());
            remaining = &remaining[split..];
        }
        current.push_str(remaining);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn byte_index_at_width(value: &str, width: usize) -> usize {
    let mut used = 0;
    for (index, character) in value.char_indices() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            return if index == 0 {
                character.len_utf8()
            } else {
                index
            };
        }
        used += character_width;
    }
    value.len()
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
    use mant_ast::{
        Block as AstBlock, DefinitionIdentity, DefinitionItem, DefinitionRole, DocumentMeta,
        DocumentSchema, DocumentSource, Inline, LayoutHint, MantDocument, Producer, QueryBundle,
        QuerySchema, Section, SourceFormat, TldrDocument, TldrOrigin,
    };
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

    fn navigation_bundle() -> QueryBundle {
        let paragraph = |value: &str| AstBlock::Paragraph {
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        QueryBundle {
            schema: QuerySchema::V3,
            label: "demo".to_owned(),
            document: Some(MantDocument {
                schema: DocumentSchema::V3,
                producer: Producer {
                    name: "mant".to_owned(),
                    version: "test".to_owned(),
                    engine: None,
                },
                source: DocumentSource {
                    format: SourceFormat::Man,
                    path: None,
                    renderer: None,
                },
                meta: DocumentMeta::default(),
                diagnostics: Vec::new(),
                blocks: Vec::new(),
                sections: vec![Section {
                    id: "options".to_owned(),
                    title: "OPTIONS".to_owned(),
                    spacing_before_lines: 0,
                    blocks: vec![AstBlock::DefinitionList {
                        items: vec![DefinitionItem {
                            identity: Some(DefinitionIdentity {
                                id: "help-option".to_owned(),
                                role: DefinitionRole::Option,
                                names: vec!["-h".to_owned(), "--help".to_owned()],
                            }),
                            terms: vec![vec![Inline::Text {
                                value: "-h, --help".to_owned(),
                            }]],
                            description: vec![paragraph("Show help")],
                            inline_term: false,
                            spacing_before_lines: None,
                        }],
                        compact: true,
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    children: vec![Section {
                        id: "details".to_owned(),
                        title: "Details".to_owned(),
                        spacing_before_lines: 0,
                        blocks: vec![paragraph("Nested details")],
                        children: Vec::new(),
                        source: None,
                    }],
                    source: None,
                }],
            }),
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
        assert!(screen.contains("MANUAL · demo"));
        assert!(screen.contains("0 visible sections"));
    }

    #[test]
    fn status_counts_only_sections_visible_in_the_folded_tree() {
        let backend = TestBackend::new(80, 14);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());

        terminal
            .draw(|frame| app.draw(frame))
            .expect("initial draw");
        assert!(
            terminal
                .backend()
                .to_string()
                .contains("2 visible sections")
        );

        app.set_selected_index(1);
        app.activate_menu_action(MenuAction::CollapseAll);
        terminal.draw(|frame| app.draw(frame)).expect("folded draw");
        let screen = terminal.backend().to_string();
        assert!(screen.contains("1 visible sections"));
        assert_eq!(app.selected, 0, "hidden child selects its visible parent");
    }

    #[test]
    fn help_overlay_is_safe_on_a_tiny_terminal() {
        let backend = TestBackend::new(12, 4);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&empty_bundle());
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

        terminal
            .draw(|frame| app.draw(frame))
            .expect("tiny help draw");
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

    #[test]
    fn semantic_options_are_revealed_only_after_their_group_expands() {
        let mut app = App::new(&navigation_bundle());

        assert_eq!(app.visible_navigation_indices(), vec![0, 1, 3]);
        app.selected = 1;
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.visible_navigation_indices(), vec![0, 1, 2, 3]);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.selected, 2);
        assert_eq!(app.document.navigation()[2].target_id, "help-option");
    }

    #[test]
    fn clicking_the_sidebar_selects_and_reclicking_a_branch_collapses_it() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 7,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.document.navigation()[app.selected].id, "details");
        let width = app.last_content_area.width;
        assert_eq!(
            app.content_scroll,
            app.rendered_cache[&width]
                .anchor_row("details")
                .expect("details anchor")
        );

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.document.navigation()[app.selected].id, "options");
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.visible_navigation_indices(), vec![0]);
    }

    #[test]
    fn selected_navigation_titles_wrap_with_a_continuous_background() {
        let mut bundle = navigation_bundle();
        bundle.document.as_mut().expect("manual").sections[0].children[0].title =
            "A deliberately long nested section title".to_owned();
        let backend = TestBackend::new(64, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&bundle);
        app.selected = 3;

        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let buffer = terminal.backend().buffer();

        assert_eq!(app.last_navigation_rows[2], 3);
        assert_eq!(app.last_navigation_rows[3], 3);
        assert_eq!(
            buffer.cell((5, 7)).expect("first selected row").bg,
            theme::SELECTED
        );
        assert_eq!(
            buffer.cell((5, 8)).expect("wrapped selected row").bg,
            theme::SELECTED
        );
    }

    #[test]
    fn dragging_the_sidebar_boundary_changes_its_width() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 36,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 44,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 44,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(app.sidebar_width, 45);
        assert!(!app.resizing_sidebar);
    }

    #[test]
    fn search_runs_only_on_confirmation_and_escape_removes_highlights() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        for character in "show".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert!(app.search_matches.is_empty());
        assert!(app.search_mode.is_editing());

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.search_query, "show");
        assert_eq!(app.search_matches.len(), 1);
        assert!(!app.search_mode.is_editing());

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.search_mode, SearchMode::Closed);
        assert!(app.search_query.is_empty());
        assert!(app.search_matches.is_empty());
    }

    #[test]
    fn confirmed_search_reports_no_matches_in_the_bottom_bar() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "missing".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        terminal.draw(|frame| app.draw(frame)).expect("draw app");

        assert!(terminal.backend().to_string().contains("No matches"));
    }

    #[test]
    fn view_menu_is_clickable_and_toggles_the_sidebar() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 7,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        terminal.draw(|frame| app.draw(frame)).expect("draw menu");
        assert!(
            terminal
                .backend()
                .to_string()
                .contains("Reset Sidebar Width")
        );

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 8,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.show_sidebar);
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn question_mark_opens_and_closes_keyboard_help() {
        let backend = TestBackend::new(100, 22);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());

        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        terminal.draw(|frame| app.draw(frame)).expect("draw help");
        assert!(
            terminal
                .backend()
                .to_string()
                .contains("Keyboard Shortcuts")
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn content_scrolling_updates_navigation_only_after_the_idle_deadline() {
        let mut app = App::new(&navigation_bundle());
        app.last_content_area = Rect::new(0, 0, 80, 10);
        app.content_scroll = 100;
        let deadline = Instant::now() + NAVIGATION_SYNC_IDLE;
        app.navigation_sync_deadline = Some(deadline);

        app.tick(
            deadline
                .checked_sub(Duration::from_millis(1))
                .expect("deadline is in the future"),
        );
        assert_eq!(app.selected, 0);

        app.tick(deadline);
        assert_eq!(app.document.navigation()[app.selected].id, "details");
        assert!(app.navigation_sync_deadline.is_none());
    }

    #[test]
    fn clicking_a_wrapped_section_reference_opens_its_target() {
        let mut bundle = navigation_bundle();
        bundle.document.as_mut().expect("manual").sections[0]
            .blocks
            .insert(
                0,
                AstBlock::Paragraph {
                    children: vec![
                        Inline::Text {
                            value: "Continue with ".to_owned(),
                        },
                        Inline::SectionReference {
                            target: "details".to_owned(),
                            children: vec![Inline::Text {
                                value: "the nested details section".to_owned(),
                            }],
                        },
                    ],
                    layout: LayoutHint::default(),
                    source: None,
                },
            );
        let backend = TestBackend::new(72, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&bundle);
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        app.expanded.clear();
        let width = app.last_content_area.width;
        let region = app.rendered_cache[&width]
            .search("nested")
            .into_iter()
            .next()
            .expect("visible reference text");

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: app.last_content_area.x
                + u16::try_from(region.start_column).expect("link column"),
            row: app.last_content_area.y + u16::try_from(region.row).expect("link row"),
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(app.document.navigation()[app.selected].id, "details");
        assert!(app.expanded.contains("options"));
        assert_eq!(
            app.content_scroll,
            app.rendered_cache[&width]
                .anchor_row("details")
                .expect("details anchor")
        );
    }

    #[test]
    fn keyboard_navigation_moves_from_tldr_and_markdown_overview_to_manual_sections() {
        let mut with_tldr = navigation_bundle();
        with_tldr.tldr = tldr_bundle().tldr;
        let mut app = App::new(&with_tldr);
        assert_eq!(app.document.navigation()[app.selected].kind, NavKind::Tldr);
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.document.navigation()[app.selected].id, "options");

        let mut with_overview = navigation_bundle();
        with_overview.document.as_mut().expect("document").blocks = vec![AstBlock::Paragraph {
            children: vec![Inline::Text {
                value: "Document overview".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }];
        let mut app = App::new(&with_overview);
        assert_eq!(app.document.navigation()[app.selected].kind, NavKind::Root);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.document.navigation()[app.selected].id, "options");
    }

    #[test]
    fn mouse_wheel_over_sidebar_does_not_scroll_the_document() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let content_scroll = app.content_scroll;

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 7,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(app.navigation_scroll, 3);
        assert_eq!(app.content_scroll, content_scroll);
        assert!(app.navigation_sync_deadline.is_none());
    }

    #[test]
    fn search_input_edits_at_unicode_character_boundaries() {
        let mut app = App::new(&navigation_bundle());
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "ab界".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        assert_eq!(app.search_draft, "ac界");
        assert_eq!(app.search_cursor, 2);
    }

    #[test]
    fn arrows_cycle_confirmed_search_results_without_requerying() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "help".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.search_matches.len() >= 2);

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.active_search_match, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.active_search_match, 0);
    }

    #[test]
    fn wrapped_expanded_navigation_titles_keep_their_subtree_guides() {
        let item = crate::NavItem {
            id: "parent".to_owned(),
            target_id: "parent".to_owned(),
            title: "A deliberately long expanded parent title".to_owned(),
            depth: 0,
            kind: NavKind::Section,
            has_children: true,
            is_last: true,
            parent_id: None,
        };

        let rows = navigation_lines(&item, 0, true, true, 22);
        let text = rows
            .iter()
            .map(|row| row.line.to_string())
            .collect::<Vec<_>>();

        assert!(text.len() > 1);
        assert!(text[0].starts_with("› │ ▾ "));
        assert!(text[1].starts_with("  │ │   "));
        assert!(
            rows[1]
                .line
                .spans
                .iter()
                .all(|span| span.style.bg == Some(theme::SELECTED))
        );
    }

    #[test]
    fn nested_navigation_rows_use_two_column_tree_guides() {
        let item = crate::NavItem {
            id: "leaf".to_owned(),
            target_id: "leaf".to_owned(),
            title: "Leaf".to_owned(),
            depth: 1,
            kind: NavKind::Section,
            has_children: false,
            is_last: false,
            parent_id: Some("parent".to_owned()),
        };

        let row = navigation_lines(&item, 0, false, false, 24)
            .remove(0)
            .line
            .to_string();

        assert!(row.starts_with("  │ ├─· Leaf"));
    }
}
