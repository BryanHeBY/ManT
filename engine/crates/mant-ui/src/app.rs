//! Interactive state machine and Ratatui widget composition.

use std::collections::{HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ast::QueryBundle;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{DocumentView, NavKind, RenderedDocument, RenderedSearchMatch, theme};

const MIN_SIDEBAR_WIDTH: u16 = 20;
const MAX_SIDEBAR_WIDTH: u16 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Closed,
    Open { editing: bool },
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
    sidebar_width: u16,
    show_sidebar: bool,
    quit: bool,
    search_mode: SearchMode,
    search_draft: String,
    search_query: String,
    search_matches: Vec<RenderedSearchMatch>,
    active_search_match: usize,
    search_width: u16,
    resizing_sidebar: bool,
    last_body_area: Rect,
    last_content_area: Rect,
    last_navigation_area: Rect,
    last_navigation_rows: Vec<usize>,
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
            sidebar_width: 36,
            show_sidebar: true,
            quit: false,
            search_mode: SearchMode::Closed,
            search_draft: String::new(),
            search_query: String::new(),
            search_matches: Vec::new(),
            active_search_match: 0,
            search_width: 0,
            resizing_sidebar: false,
            last_body_area: Rect::default(),
            last_content_area: Rect::default(),
            last_navigation_area: Rect::default(),
            last_navigation_rows: Vec::new(),
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
        if self.search_mode.is_open() {
            self.handle_search_key(key);
            return;
        }
        if (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('f'))
            || key.code == KeyCode::Char('/')
        {
            self.open_search();
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.select_relative(1),
            KeyCode::Char('k') | KeyCode::Up => self.select_relative(-1),
            KeyCode::Char('h') | KeyCode::Left => self.collapse_or_select_parent(),
            KeyCode::Char('l') | KeyCode::Right => self.expand_or_select_child(),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_selected(),
            KeyCode::PageDown | KeyCode::Char('d') => self.scroll_content(10),
            KeyCode::PageUp | KeyCode::Char('u') => self.scroll_content(-10),
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
                        self.selected = index;
                        if self.document.navigation()[index].has_children {
                            self.expanded
                                .insert(self.document.navigation()[index].id.clone());
                        }
                        self.scroll_to_selected();
                    }
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
        let visible = self.visible_navigation_indices();
        let line_width = usize::from(navigation_area.width);
        let rows = navigation_rows(
            self.document.navigation(),
            &visible,
            self.selected,
            &self.expanded,
            line_width,
        );
        let selected_row = rows
            .iter()
            .position(|row| row.item_index == self.selected)
            .unwrap_or_default();
        self.keep_selected_navigation_visible(selected_row, usize::from(navigation_area.height));
        let visible_rows = rows
            .into_iter()
            .skip(self.navigation_scroll)
            .take(usize::from(navigation_area.height))
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

    fn draw_search(&self, frame: &mut Frame<'_>, area: Rect) {
        let style = Style::default().bg(theme::MENU);
        frame.render_widget(Block::default().style(style), area);
        let prompt = format!(" Find: {}", self.search_draft);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Find: ", style.fg(theme::YELLOW)),
                Span::styled(
                    self.search_draft.clone(),
                    style.fg(theme::TEXT).bg(theme::SURFACE),
                ),
                Span::styled(" ", style.bg(theme::TEXT)),
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
        self.selected = visible[next];
        self.scroll_to_selected();
    }

    fn open_search(&mut self) {
        self.search_mode = SearchMode::Open { editing: false };
        self.search_draft.clone_from(&self.search_query);
    }

    fn close_search(&mut self) {
        self.search_mode = SearchMode::Closed;
        self.search_draft.clear();
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
                self.search_draft.pop();
                self.search_mode = SearchMode::Open { editing: true };
            }
            KeyCode::Delete => {
                self.search_draft.clear();
                self.search_mode = SearchMode::Open { editing: true };
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_draft.push(character);
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
        let Some(search_match) = self.search_matches.get(self.active_search_match).copied() else {
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
            self.selected = index;
        }
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
        if let Some(row) = rendered.anchor_row(&item.target_id) {
            self.content_scroll = row;
        }
    }

    fn scroll_content(&mut self, delta: isize) {
        self.content_scroll = self.content_scroll.saturating_add_signed(delta);
        self.sync_selection_to_scroll();
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
            self.selected = index;
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
            self.selected = index;
            self.scroll_to_selected();
        }
    }
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
    let tree = if item.kind == NavKind::Tldr {
        "◆ ".to_owned()
    } else if item.depth == 0 {
        if item.has_children {
            if expanded { "▾ " } else { "▸ " }.to_owned()
        } else {
            "· ".to_owned()
        }
    } else {
        let mut prefix = "│  ".repeat(item.depth.saturating_sub(1));
        prefix.push_str(if item.is_last && !expanded {
            "╰─"
        } else {
            "├─"
        });
        prefix.push_str(if item.has_children {
            if expanded { "▾ " } else { "▸ " }
        } else if item.kind == NavKind::Option {
            "◇ "
        } else {
            "· "
        });
        prefix
    };
    let prefix = format!("{selection}{tree}");
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
    let prefix_width = prefix.width();
    let title_width = width.saturating_sub(prefix_width).max(1);
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
                " ".repeat(prefix_width)
            };
            let value = fit_to_width(&format!("{line_prefix}{title}"), width);
            NavigationRow {
                item_index,
                line: Line::from(Span::styled(value, style)),
            }
        })
        .collect()
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
}
