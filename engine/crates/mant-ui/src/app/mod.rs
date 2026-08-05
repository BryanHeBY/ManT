//! Interactive state machine and Ratatui widget composition.

mod menu;

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
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use self::menu::{MenuAction, MenuId, menu_entries};

use crate::{
    DocumentView, NavKind, RenderedDocument, RenderedSearchMatch,
    layout::{
        CONTENT_MARGIN, CONTENT_SCROLLBAR_GAP, DEFAULT_SIDEBAR_WIDTH, MIN_CONTENT_WIDTH,
        MIN_SIDEBAR_WIDTH, SIDEBAR_SPLITTER_WIDTH, maximum_sidebar_width,
    },
    navigation,
    scrollbar::{ScrollbarDrag, VerticalScrollbar},
    theme,
};

const NAVIGATION_SYNC_IDLE: Duration = Duration::from_millis(140);
/// Defers expensive width-dependent document reflow until the splitter settles.
///
/// Pointer events can arrive substantially faster than a large manual can be
/// lowered into visual rows. Keeping only the newest coordinate and restarting
/// this short idle window avoids replaying every intermediate width.
const SIDEBAR_RESIZE_IDLE: Duration = Duration::from_millis(60);

/// Whether an input or timer update changed visible application state.
///
/// The terminal loop uses this result to avoid rebuilding large documents for
/// bookkeeping-only events, notably intermediate sidebar drag coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    Unchanged,
    Redraw,
}

impl UpdateOutcome {
    #[must_use]
    pub const fn needs_redraw(self) -> bool {
        matches!(self, Self::Redraw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Closed,
    Open { editing: bool },
}

#[derive(Debug)]
struct SearchState {
    mode: SearchMode,
    draft: String,
    cursor: usize,
    query: String,
    matches: Vec<RenderedSearchMatch>,
    active_match: usize,
    render_width: u16,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            mode: SearchMode::Closed,
            draft: String::new(),
            cursor: 0,
            query: String::new(),
            matches: Vec::new(),
            active_match: 0,
            render_width: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    Menu { id: MenuId, cursor: usize },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerDrag {
    None,
    Sidebar,
    NavigationScrollbar(ScrollbarDrag),
    ContentScrollbar(ScrollbarDrag),
}

#[derive(Debug, Clone, Copy)]
struct PendingSidebarResize {
    column: u16,
    deadline: Instant,
}

/// Geometry retained from the previous frame for pointer hit testing.
///
/// Keeping these values together makes the boundary between layout/rendering
/// and event handling explicit: input code may inspect the last complete
/// frame, but it does not partially recompute layout on its own.
#[derive(Debug, Default)]
struct FrameGeometry {
    body: Rect,
    content: Rect,
    content_scrollbar: Option<VerticalScrollbar>,
    navigation: Rect,
    navigation_scrollbar: Option<VerticalScrollbar>,
    sidebar_splitter: Rect,
    status: Rect,
    navigation_rows: Vec<usize>,
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
    search: SearchState,
    overlay: Overlay,
    pointer_drag: PointerDrag,
    geometry: FrameGeometry,
    navigation_sync_deadline: Option<Instant>,
    pending_sidebar_resize: Option<PendingSidebarResize>,
    content_render_width: u16,
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
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            show_sidebar: true,
            quit: false,
            search: SearchState::default(),
            overlay: Overlay::None,
            pointer_drag: PointerDrag::None,
            geometry: FrameGeometry::default(),
            navigation_sync_deadline: None,
            pending_sidebar_resize: None,
            content_render_width: 0,
            rendered_cache: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    pub(crate) fn tick(&mut self, now: Instant) -> UpdateOutcome {
        let mut outcome = UpdateOutcome::Unchanged;
        if let Some(pending) = self
            .pending_sidebar_resize
            .filter(|pending| pending.deadline <= now)
        {
            self.pending_sidebar_resize = None;
            self.commit_sidebar_at(pending.column);
            outcome = UpdateOutcome::Redraw;
        }
        if self
            .navigation_sync_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.navigation_sync_deadline = None;
            self.sync_selection_to_scroll();
            outcome = UpdateOutcome::Redraw;
        }
        outcome
    }

    pub(crate) fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        [
            self.navigation_sync_deadline,
            self.pending_sidebar_resize.map(|pending| pending.deadline),
        ]
        .into_iter()
        .flatten()
        .map(|deadline| deadline.saturating_duration_since(now))
        .min()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> UpdateOutcome {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return UpdateOutcome::Unchanged;
        }
        if self.overlay != Overlay::None {
            self.handle_overlay_key(key);
            return UpdateOutcome::Redraw;
        }
        if self.search.mode.is_open() && key.code == KeyCode::F(10) {
            self.open_menu(MenuId::File);
            return UpdateOutcome::Redraw;
        }
        if self.search.mode.is_open() {
            self.handle_search_key(key);
            return UpdateOutcome::Redraw;
        }
        if key.code == KeyCode::F(10) {
            self.open_menu(MenuId::File);
            return UpdateOutcome::Redraw;
        }
        if (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('f'))
            || key.code == KeyCode::Char('/')
        {
            self.open_search();
            return UpdateOutcome::Redraw;
        }
        if key.code == KeyCode::Char('?') {
            self.overlay = Overlay::Help;
            return UpdateOutcome::Redraw;
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
                self.sidebar_width = self
                    .sidebar_width
                    .saturating_add(2)
                    .min(maximum_sidebar_width(self.geometry.body.width));
            }
            _ => return UpdateOutcome::Unchanged,
        }
        UpdateOutcome::Redraw
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> UpdateOutcome {
        if self.handle_overlay_mouse(mouse) {
            return if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                UpdateOutcome::Redraw
            } else {
                UpdateOutcome::Unchanged
            };
        }
        if let Some(outcome) = self.handle_pointer_control(mouse) {
            return outcome;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if self.search.mode.is_open()
                    && self
                        .geometry
                        .status
                        .contains((mouse.column, mouse.row).into()) =>
            {
                self.move_search_cursor_to(mouse.column);
                UpdateOutcome::Redraw
            }
            MouseEventKind::ScrollDown
                if self
                    .geometry
                    .navigation
                    .contains((mouse.column, mouse.row).into()) =>
            {
                self.navigation_scroll = self.navigation_scroll.saturating_add(3);
                UpdateOutcome::Redraw
            }
            MouseEventKind::ScrollUp
                if self
                    .geometry
                    .navigation
                    .contains((mouse.column, mouse.row).into()) =>
            {
                self.navigation_scroll = self.navigation_scroll.saturating_sub(3);
                UpdateOutcome::Redraw
            }
            MouseEventKind::ScrollDown => {
                self.scroll_content(3);
                UpdateOutcome::Redraw
            }
            MouseEventKind::ScrollUp => {
                self.scroll_content(-3);
                UpdateOutcome::Redraw
            }
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .geometry
                    .navigation
                    .contains((mouse.column, mouse.row).into()) =>
            {
                let local_row = usize::from(mouse.row - self.geometry.navigation.y);
                if let Some(index) = self.geometry.navigation_rows.get(local_row).copied() {
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
                UpdateOutcome::Redraw
            }
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .geometry
                    .content
                    .contains((mouse.column, mouse.row).into()) =>
            {
                self.activate_content_link(mouse.column, mouse.row);
                UpdateOutcome::Redraw
            }
            _ => UpdateOutcome::Unchanged,
        }
    }

    fn handle_pointer_control(&mut self, mouse: MouseEvent) -> Option<UpdateOutcome> {
        self.handle_pointer_control_at(mouse, Instant::now())
    }

    fn handle_pointer_control_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
    ) -> Option<UpdateOutcome> {
        let outcome = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .geometry
                    .navigation_scrollbar
                    .is_some_and(|scrollbar| scrollbar.contains(mouse.column, mouse.row)) =>
            {
                let scrollbar = self
                    .geometry
                    .navigation_scrollbar
                    .expect("guarded scrollbar");
                let (drag, position) = scrollbar.begin_drag(mouse.row);
                self.navigation_scroll = position;
                self.pointer_drag = PointerDrag::NavigationScrollbar(drag);
                UpdateOutcome::Redraw
            }
            MouseEventKind::Down(MouseButton::Left)
                if self.is_sidebar_boundary(mouse.column, mouse.row) =>
            {
                self.pointer_drag = PointerDrag::Sidebar;
                self.pending_sidebar_resize = None;
                UpdateOutcome::Unchanged
            }
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .geometry
                    .content_scrollbar
                    .is_some_and(|scrollbar| scrollbar.contains(mouse.column, mouse.row)) =>
            {
                let scrollbar = self.geometry.content_scrollbar.expect("guarded scrollbar");
                let (drag, position) = scrollbar.begin_drag(mouse.row);
                self.content_scroll = position;
                self.schedule_navigation_sync();
                self.pointer_drag = PointerDrag::ContentScrollbar(drag);
                UpdateOutcome::Redraw
            }
            MouseEventKind::Drag(MouseButton::Left) => match self.pointer_drag {
                PointerDrag::Sidebar => {
                    self.request_sidebar_resize(mouse.column, now);
                    UpdateOutcome::Unchanged
                }
                PointerDrag::NavigationScrollbar(drag) => {
                    self.scroll_navigation_to_pointer(mouse.row, drag);
                    UpdateOutcome::Redraw
                }
                PointerDrag::ContentScrollbar(drag) => {
                    self.scroll_content_to_pointer(mouse.row, drag);
                    UpdateOutcome::Redraw
                }
                PointerDrag::None => return None,
            },
            MouseEventKind::Up(MouseButton::Left) if self.pointer_drag != PointerDrag::None => {
                match self.pointer_drag {
                    PointerDrag::Sidebar => self.finish_sidebar_resize(mouse.column),
                    PointerDrag::NavigationScrollbar(drag) => {
                        self.scroll_navigation_to_pointer(mouse.row, drag);
                    }
                    PointerDrag::ContentScrollbar(drag) => {
                        self.scroll_content_to_pointer(mouse.row, drag);
                    }
                    PointerDrag::None => {}
                }
                self.pointer_drag = PointerDrag::None;
                UpdateOutcome::Redraw
            }
            _ => return None,
        };
        Some(outcome)
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let [menu_area, body_area, status_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        self.draw_menu(frame, menu_area);
        self.geometry.body = body_area;
        if self.show_sidebar
            && body_area.width > MIN_SIDEBAR_WIDTH + SIDEBAR_SPLITTER_WIDTH + MIN_CONTENT_WIDTH
        {
            self.sidebar_width = self
                .sidebar_width
                .clamp(MIN_SIDEBAR_WIDTH, maximum_sidebar_width(body_area.width));
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
            self.geometry.navigation_scrollbar = None;
            self.geometry.sidebar_splitter = Rect::default();
            self.geometry.navigation_rows.clear();
            if matches!(
                self.pointer_drag,
                PointerDrag::Sidebar | PointerDrag::NavigationScrollbar(_)
            ) {
                self.pending_sidebar_resize = None;
                self.pointer_drag = PointerDrag::None;
            }
            self.draw_content(frame, body_area);
        }
        if self.search.mode.is_open() {
            self.draw_search(frame, status_area);
        } else {
            self.draw_status(frame, status_area);
        }
        self.geometry.status = status_area;
        self.draw_overlay(frame);
    }

    fn open_menu(&mut self, id: MenuId) {
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
            MenuAction::ResetSidebar => self.sidebar_width = DEFAULT_SIDEBAR_WIDTH,
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
            MenuAction::Help => {
                self.close_search();
                self.overlay = Overlay::Help;
            }
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
            Paragraph::new(format!("{} ", self.document.terminal_label()))
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
                let height = u16::try_from(entries.len()).unwrap_or_default();
                let area = Rect::new(
                    id.left().min(frame.area().width.saturating_sub(1)),
                    1,
                    30.min(frame.area().width.saturating_sub(id.left())),
                    height.min(frame.area().height.saturating_sub(1)),
                );
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Block::default().style(Style::default().bg(theme::BASE)),
                    area,
                );
                let inner = area;
                for (index, entry) in entries.iter().enumerate().take(usize::from(inner.height)) {
                    let row = Rect::new(
                        inner.x,
                        inner.y + u16::try_from(index).unwrap_or_default(),
                        inner.width,
                        1,
                    );
                    let content = row.inner(Margin {
                        horizontal: 1,
                        vertical: 0,
                    });
                    let active = index == cursor;
                    let checked =
                        matches!(entry.action, MenuAction::ToggleSidebar) && self.show_sidebar;
                    let prefix = if checked { "✓ " } else { "  " };
                    let label = format!("{prefix}{}", entry.label);
                    let gap = usize::from(content.width)
                        .saturating_sub(label.width())
                        .saturating_sub(entry.shortcut.width());
                    let value = fit_to_width(
                        &format!(
                            "{label}{}{shortcut}",
                            " ".repeat(gap),
                            shortcut = entry.shortcut
                        ),
                        usize::from(content.width),
                    );
                    let style = if active {
                        Style::default()
                            .fg(theme::SELECTED_TEXT)
                            .bg(theme::SELECTED)
                    } else {
                        Style::default().fg(theme::TEXT).bg(theme::BASE)
                    };
                    frame.render_widget(Block::default().style(style), row);
                    frame.render_widget(Paragraph::new(Span::styled(value, style)), content);
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

    fn draw_sidebar_splitter(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.geometry.sidebar_splitter = area;
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(theme::BORDER))
                .style(Style::default().bg(theme::SIDEBAR)),
            area,
        );
    }

    fn draw_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::SIDEBAR)),
            area,
        );
        let [header_area, section_label_area, navigation_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(area);

        let metadata = sidebar_metadata(
            self.document.top_level_count(),
            self.document.section_count(),
            self.document.has_tldr(),
            navigation_area.width,
        );
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
                Line::from(Span::styled(metadata, Style::default().fg(theme::SUBTEXT))),
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

        self.geometry.navigation = navigation_area;
        let visible = self.visible_navigation_indices();
        let line_width = usize::from(navigation_area.width);
        let rows = navigation::rows(
            self.document.navigation(),
            &visible,
            self.selected,
            &self.expanded,
            line_width,
        );
        let row_count = rows.len();
        let height = usize::from(navigation_area.height);
        let maximum = row_count.saturating_sub(height);
        self.navigation_scroll = self.navigation_scroll.min(maximum);
        let selected_range = (self.navigation_visibility_target.take() == Some(self.selected))
            .then(|| navigation::item_row_range(&rows, self.selected))
            .flatten();
        if let Some(range) = selected_range {
            self.keep_selected_navigation_visible(range, height);
        }
        let visible_rows = rows
            .into_iter()
            .skip(self.navigation_scroll)
            .take(height)
            .collect::<Vec<_>>();
        self.geometry.navigation_rows = visible_rows.iter().map(|row| row.item_index).collect();
        let lines = visible_rows
            .into_iter()
            .map(|row| row.line)
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(Text::from(lines)).style(Style::default().bg(theme::SIDEBAR)),
            navigation_area,
        );
        self.draw_navigation_scrollbar(frame, navigation_area, row_count, height);
    }

    fn draw_navigation_scrollbar(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        row_count: usize,
        viewport_height: usize,
    ) {
        self.geometry.navigation_scrollbar =
            VerticalScrollbar::new(area, row_count, viewport_height, self.navigation_scroll);
        if let Some(scrollbar) = self.geometry.navigation_scrollbar {
            scrollbar.render(frame);
        } else if matches!(self.pointer_drag, PointerDrag::NavigationScrollbar(_)) {
            self.pointer_drag = PointerDrag::None;
        }
    }

    fn draw_content(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::CONTENT)),
            area,
        );
        let inner = area.inner(CONTENT_MARGIN);
        let viewport_height = usize::from(inner.height);
        // The scrollbar owns a gutter beside the document instead of
        // overwriting its final column. This is especially visible on the
        // right border of full-width TLDR panels.
        let scrollbar_gutter = CONTENT_SCROLLBAR_GAP.saturating_add(1);
        // Test the narrower, scrollbar-bearing layout first. Almost every
        // manual needs its virtual trailing rows, so this avoids rendering a
        // second full-width document merely to discover that fact.
        let sizing_width = inner.width.saturating_sub(scrollbar_gutter).max(1);
        let sizing_rows = self
            .rendered_cache
            .entry(sizing_width)
            .or_insert_with(|| self.document.render(sizing_width))
            .row_count;
        let needs_scrollbar = virtual_content_rows(sizing_rows, viewport_height) > viewport_height;
        let document_area = if needs_scrollbar && inner.width > scrollbar_gutter {
            Rect::new(
                inner.x,
                inner.y,
                inner.width.saturating_sub(scrollbar_gutter),
                inner.height,
            )
        } else {
            inner
        };
        self.geometry.content = document_area;
        let render_width = document_area.width.max(1);
        let viewport_anchor = (self.content_render_width != 0
            && self.content_render_width != render_width)
            .then(|| {
                self.rendered_cache
                    .get(&self.content_render_width)
                    .and_then(|rendered| rendered.viewport_anchor(self.content_scroll))
            })
            .flatten();
        self.rendered_cache
            .entry(render_width)
            .or_insert_with(|| self.document.render(render_width));
        if !self.search.query.is_empty() && self.search.render_width != render_width {
            self.refresh_search(render_width);
        }
        let rendered = &self.rendered_cache[&render_width];
        if let Some(anchor) = viewport_anchor
            && let Some(row) = rendered.row_for_viewport_anchor(anchor)
        {
            self.content_scroll = row;
        }
        self.content_render_width = render_width;
        // Keep enough virtual trailing space for every addressable row,
        // including the final section heading, to become the viewport's first
        // line. The previous OpenTUI frontend achieved this with a terminal-
        // height spacer after the document.
        let virtual_rows = virtual_content_rows(rendered.row_count, viewport_height);
        let maximum = virtual_rows.saturating_sub(viewport_height);
        self.content_scroll = self.content_scroll.min(maximum);
        let matches = if self.search.query.is_empty() {
            &[]
        } else {
            self.search.matches.as_slice()
        };
        let text = rendered.viewport_text(
            self.content_scroll,
            viewport_height,
            matches,
            (!matches.is_empty()).then_some(self.search.active_match),
        );
        frame.render_widget(
            Paragraph::new(text).style(Style::default().bg(theme::CONTENT)),
            document_area,
        );
        self.geometry.content_scrollbar =
            VerticalScrollbar::new(inner, virtual_rows, viewport_height, self.content_scroll);
        if let Some(scrollbar) = self.geometry.content_scrollbar {
            scrollbar.render(frame);
        } else if matches!(self.pointer_drag, PointerDrag::ContentScrollbar(_)) {
            self.pointer_drag = PointerDrag::None;
        }
        // A width-dependent rendering can be large (notably for GCC). Keeping
        // the current width hot is useful; retaining every prior terminal
        // width turns repeated resizing into unbounded growth.
        self.rendered_cache
            .retain(|width, _| *width == render_width);
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
        let suffix = if !self.search.query.is_empty() && !self.search.matches.is_empty() {
            format!(
                "Find “{}” · {} matches ",
                self.search.query,
                self.search.matches.len()
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
        let (before_cursor, after_cursor) = self.search.draft.split_at(self.search.cursor);
        let cursor_character = after_cursor.chars().next();
        let cursor_bytes = cursor_character.map_or(0, char::len_utf8);
        let after_cursor = &after_cursor[cursor_bytes..];
        let prompt = format!(" Find: {}", self.search.draft);
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
        let suffix = if !self.search.mode.is_editing() && !self.search.query.is_empty() {
            if self.search.matches.is_empty() {
                " No matches · Edit query · Esc close ".to_owned()
            } else {
                format!(
                    " {}/{} · Enter next · Esc close ",
                    self.search.active_match + 1,
                    self.search.matches.len()
                )
            }
        } else {
            " Enter search · Esc cancel ".to_owned()
        };
        let suffix_style = if self.search.matches.is_empty()
            && !self.search.mode.is_editing()
            && !self.search.query.is_empty()
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
        self.search.mode = SearchMode::Open { editing: false };
        self.search.draft.clone_from(&self.search.query);
        self.search.cursor = self.search.draft.len();
    }

    fn close_search(&mut self) {
        self.search = SearchState::default();
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_search(),
            KeyCode::Enter => {
                if !self.search.mode.is_editing() && self.search.draft == self.search.query {
                    self.select_search_relative(1);
                } else {
                    self.search.query.clone_from(&self.search.draft);
                    self.search.mode = SearchMode::Open { editing: false };
                    self.refresh_search(self.geometry.content.width.max(1));
                    self.select_active_search_match();
                }
            }
            KeyCode::Char('n' | 'N')
                if !self.search.mode.is_editing()
                    && self.search.draft == self.search.query
                    && !self.search.matches.is_empty() =>
            {
                self.select_search_relative(
                    if key.code == KeyCode::Char('N') || key.modifiers.contains(KeyModifiers::SHIFT)
                    {
                        -1
                    } else {
                        1
                    },
                );
            }
            KeyCode::Backspace => {
                if let Some(previous) =
                    previous_char_boundary(&self.search.draft, self.search.cursor)
                {
                    self.search.draft.drain(previous..self.search.cursor);
                    self.search.cursor = previous;
                    self.search.mode = SearchMode::Open { editing: true };
                }
            }
            KeyCode::Delete => {
                if let Some(next) = next_char_boundary(&self.search.draft, self.search.cursor) {
                    self.search.draft.drain(self.search.cursor..next);
                    self.search.mode = SearchMode::Open { editing: true };
                }
            }
            KeyCode::Left => {
                self.search.cursor = previous_char_boundary(&self.search.draft, self.search.cursor)
                    .unwrap_or_default();
            }
            KeyCode::Right => {
                self.search.cursor = next_char_boundary(&self.search.draft, self.search.cursor)
                    .unwrap_or(self.search.draft.len());
            }
            KeyCode::Home => self.search.cursor = 0,
            KeyCode::End => self.search.cursor = self.search.draft.len(),
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search.cursor = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search.cursor = self.search.draft.len();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search.draft.clear();
                self.search.cursor = 0;
                self.search.mode = SearchMode::Open { editing: true };
            }
            KeyCode::Down if !self.search.mode.is_editing() => self.select_search_relative(1),
            KeyCode::Up if !self.search.mode.is_editing() => self.select_search_relative(-1),
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search.draft.insert(self.search.cursor, character);
                self.search.cursor += character.len_utf8();
                self.search.mode = SearchMode::Open { editing: true };
            }
            _ => {}
        }
    }

    fn refresh_search(&mut self, width: u16) {
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        self.search.matches = rendered.search(&self.search.query);
        self.search.active_match = self
            .search
            .active_match
            .min(self.search.matches.len().saturating_sub(1));
        self.search.render_width = width;
    }

    fn select_search_relative(&mut self, delta: isize) {
        if self.search.matches.is_empty() {
            return;
        }
        let length = isize::try_from(self.search.matches.len()).unwrap_or(isize::MAX);
        let current = isize::try_from(self.search.active_match).unwrap_or_default();
        self.search.active_match =
            usize::try_from((current + delta).rem_euclid(length)).unwrap_or_default();
        self.select_active_search_match();
    }

    fn select_active_search_match(&mut self) {
        let Some(search_match) = self.search.matches.get(self.search.active_match).cloned() else {
            return;
        };
        self.content_scroll = search_match.row;
        self.select_section_at_row(search_match.row);
    }

    fn select_section_at_row(&mut self, row: usize) {
        let width = self.geometry.content.width.max(1);
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
        let width = self.geometry.content.width.max(1);
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        if let Some(row) = rendered.anchor_row(&item.target_id) {
            self.content_scroll = row;
        }
    }

    fn activate_content_link(&mut self, column: u16, row: u16) {
        let width = self.geometry.content.width.max(1);
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        let document_row = self.content_scroll + usize::from(row - self.geometry.content.y);
        let document_column = usize::from(column - self.geometry.content.x);
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
        self.schedule_navigation_sync();
    }

    fn schedule_navigation_sync(&mut self) {
        self.navigation_sync_deadline = Some(Instant::now() + NAVIGATION_SYNC_IDLE);
    }

    fn scroll_content_to_pointer(&mut self, row: u16, drag: ScrollbarDrag) {
        if let Some(scrollbar) = self.geometry.content_scrollbar {
            self.content_scroll = scrollbar.position_for_pointer(row, drag);
            self.schedule_navigation_sync();
        }
    }

    fn scroll_navigation_to_pointer(&mut self, row: u16, drag: ScrollbarDrag) {
        if let Some(scrollbar) = self.geometry.navigation_scrollbar {
            self.navigation_scroll = scrollbar.position_for_pointer(row, drag);
        }
    }

    fn move_search_cursor_to(&mut self, column: u16) {
        const SEARCH_PREFIX_WIDTH: u16 = 7;
        let text_column = usize::from(
            column.saturating_sub(self.geometry.status.x.saturating_add(SEARCH_PREFIX_WIDTH)),
        );
        self.search.cursor = cursor_byte_at_column(&self.search.draft, text_column);
    }

    fn jump_content(&mut self, end: bool) {
        self.content_scroll = if end { usize::MAX } else { 0 };
        self.navigation_sync_deadline = Some(Instant::now() + NAVIGATION_SYNC_IDLE);
    }

    fn sync_selection_to_scroll(&mut self) {
        self.select_section_at_row(self.content_scroll);
    }

    fn keep_selected_navigation_visible(
        &mut self,
        selected: std::ops::Range<usize>,
        height: usize,
    ) {
        let selected_height = selected.end.saturating_sub(selected.start);
        if selected_height >= height || selected.start < self.navigation_scroll {
            self.navigation_scroll = selected.start;
        } else if selected.end > self.navigation_scroll.saturating_add(height) {
            self.navigation_scroll = selected.end.saturating_sub(height);
        }
    }

    fn is_sidebar_boundary(&self, column: u16, row: u16) -> bool {
        self.show_sidebar
            && self
                .geometry
                .sidebar_splitter
                .contains((column, row).into())
    }

    fn sidebar_width_at(&self, column: u16) -> u16 {
        let maximum = maximum_sidebar_width(self.geometry.body.width);
        column
            .saturating_sub(self.geometry.body.x)
            .clamp(MIN_SIDEBAR_WIDTH, maximum.max(MIN_SIDEBAR_WIDTH))
    }

    fn commit_sidebar_at(&mut self, column: u16) {
        self.sidebar_width = self.sidebar_width_at(column);
    }

    fn request_sidebar_resize(&mut self, column: u16, now: Instant) {
        // This is a trailing-edge debounce: every new coordinate replaces the
        // previous one and postpones reflow until pointer input becomes idle.
        self.pending_sidebar_resize = Some(PendingSidebarResize {
            column,
            deadline: now + SIDEBAR_RESIZE_IDLE,
        });
    }

    fn finish_sidebar_resize(&mut self, column: u16) {
        self.commit_sidebar_at(column);
        self.pending_sidebar_resize = None;
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

fn cursor_byte_at_column(value: &str, column: usize) -> usize {
    let mut used = 0;
    for (index, character) in value.char_indices() {
        let next = used + character.width().unwrap_or(0);
        if column < next {
            return index;
        }
        if column == next {
            return index + character.len_utf8();
        }
        used = next;
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

fn sidebar_metadata(
    top_level_count: usize,
    section_count: usize,
    has_tldr: bool,
    width: u16,
) -> String {
    let suffix = if has_tldr { " · TLDR" } else { "" };
    let candidates = [
        format!(" {top_level_count} top-level · {section_count} sections{suffix}"),
        format!(" {top_level_count} top · {section_count} sections{suffix}"),
        format!(" {top_level_count} · {section_count} sections{suffix}"),
        format!(" {top_level_count} · {section_count}{suffix}"),
    ];
    let available = usize::from(width);
    candidates
        .into_iter()
        .find(|candidate| candidate.width() <= available)
        .unwrap_or_else(|| {
            if has_tldr && available >= " TLDR".width() {
                " TLDR".to_owned()
            } else {
                fit_to_width(&format!(" {top_level_count} · {section_count}"), available)
            }
        })
}

const fn virtual_content_rows(row_count: usize, viewport_height: usize) -> usize {
    row_count.saturating_add(viewport_height.saturating_sub(1))
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
    fn terminal_title_includes_the_manual_section_but_the_sidebar_does_not() {
        let mut bundle = navigation_bundle();
        bundle.document.as_mut().expect("document").meta.section = Some("1".to_owned());
        let backend = TestBackend::new(80, 14);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&bundle);

        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let screen = terminal.backend().to_string();

        assert!(screen.lines().next().expect("menu row").contains("demo(1)"));
        assert!(screen.contains("MANUAL · demo"));
        assert!(!screen.contains("MANUAL · demo(1)"));
    }

    #[test]
    fn the_final_section_heading_can_become_the_first_content_row() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal
            .draw(|frame| app.draw(frame))
            .expect("initial draw");
        app.set_selected_index(3);
        app.scroll_to_selected();
        let width = app.geometry.content.width;
        let expected = app.rendered_cache[&width]
            .anchor_row("details")
            .expect("details anchor");

        terminal
            .draw(|frame| app.draw(frame))
            .expect("scrolled draw");

        assert_eq!(app.content_scroll, expected);
        let row = app.geometry.content.y;
        let content = (app.geometry.content.x..app.geometry.content.right())
            .filter_map(|column| terminal.backend().buffer().cell((column, row)))
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(content.trim_start().starts_with("Details"));
    }

    #[test]
    fn overflowing_navigation_exposes_a_scrollbar() {
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());

        terminal.draw(|frame| app.draw(frame)).expect("draw app");

        let scrollbar_column = app.geometry.navigation.right().saturating_sub(1);
        assert!(
            (app.geometry.navigation.y..app.geometry.navigation.bottom()).any(|row| {
                terminal
                    .backend()
                    .buffer()
                    .cell((scrollbar_column, row))
                    .is_some_and(|cell| cell.bg == theme::SCROLLBAR_THUMB)
            })
        );
        assert_eq!(app.geometry.navigation.right(), app.sidebar_width);
        assert!(
            (app.geometry.navigation.y..app.geometry.navigation.bottom()).any(|row| {
                terminal
                    .backend()
                    .buffer()
                    .cell((scrollbar_column, row))
                    .is_some_and(|cell| cell.bg == theme::SCROLLBAR_TRACK)
            })
        );
    }

    #[test]
    fn navigation_scrollbar_click_and_drag_do_not_resize_the_sidebar() {
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let scrollbar = app
            .geometry
            .navigation_scrollbar
            .expect("navigation scrollbar");
        let area = scrollbar.area();
        let maximum = scrollbar.maximum();
        let sidebar_width = app.sidebar_width;
        assert!(area.height > 1);
        assert!(maximum > 0);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.bottom() - 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.navigation_scroll, maximum);
        assert!(matches!(
            app.pointer_drag,
            PointerDrag::NavigationScrollbar(_)
        ));
        assert_eq!(app.sidebar_width, sidebar_width);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.navigation_scroll, 0);
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.pointer_drag, PointerDrag::None);
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
            buffer
                .cell((DEFAULT_SIDEBAR_WIDTH - 1, 1))
                .expect("borderless sidebar edge")
                .bg,
            theme::SIDEBAR
        );
        assert_eq!(
            buffer
                .cell((DEFAULT_SIDEBAR_WIDTH, 1))
                .expect("sidebar splitter")
                .symbol(),
            "│"
        );
        assert_eq!(
            buffer
                .cell((DEFAULT_SIDEBAR_WIDTH, 1))
                .expect("sidebar splitter background")
                .bg,
            theme::SIDEBAR
        );
        assert_eq!(
            buffer.cell((0, 5)).expect("selected tldr navigation").bg,
            theme::TLDR_SELECTED
        );
        assert_eq!(
            buffer
                .cell((DEFAULT_SIDEBAR_WIDTH + SIDEBAR_SPLITTER_WIDTH + 1, 2,))
                .expect("tldr panel border")
                .bg,
            theme::TLDR_SURFACE
        );
        let panel_right = app.geometry.content.right().saturating_sub(1);
        assert_eq!(
            buffer
                .cell((panel_right, 2))
                .expect("tldr right border")
                .symbol(),
            "┐"
        );
        assert_eq!(
            app.geometry
                .content_scrollbar
                .expect("content scrollbar")
                .area()
                .x,
            app.geometry.content.right() + CONTENT_SCROLLBAR_GAP
        );
        assert_eq!(
            buffer
                .cell((app.geometry.content.right(), 2))
                .expect("content-scrollbar gap")
                .bg,
            theme::CONTENT
        );
    }

    #[test]
    fn default_geometry_keeps_the_established_sidebar_and_content_padding() {
        let backend = TestBackend::new(100, 14);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&tldr_bundle());

        terminal.draw(|frame| app.draw(frame)).expect("draw app");

        assert_eq!(app.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
        assert_eq!(app.geometry.navigation.right(), DEFAULT_SIDEBAR_WIDTH);
        assert_eq!(
            app.geometry.sidebar_splitter,
            Rect::new(DEFAULT_SIDEBAR_WIDTH, 1, SIDEBAR_SPLITTER_WIDTH, 12)
        );
        assert_eq!(
            app.geometry.content.x,
            DEFAULT_SIDEBAR_WIDTH + SIDEBAR_SPLITTER_WIDTH + 1
        );
        assert_eq!(app.geometry.content.y, 2);
        let scrollbar = app.geometry.content_scrollbar.expect("content scrollbar");
        assert_eq!(
            scrollbar.area().x,
            app.geometry.content.right() + CONTENT_SCROLLBAR_GAP
        );
        assert_eq!(scrollbar.area().y, app.geometry.content.y);
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
        let width = app.geometry.content.width;
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

        assert_eq!(app.geometry.navigation_rows[2], 3);
        assert_eq!(app.geometry.navigation_rows[3], 3);
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
    fn navigation_visibility_keeps_the_complete_selected_title_on_screen() {
        let mut app = App::new(&navigation_bundle());
        app.navigation_scroll = 4;

        app.keep_selected_navigation_visible(8..11, 5);
        assert_eq!(app.navigation_scroll, 6);

        app.keep_selected_navigation_visible(2..5, 5);
        assert_eq!(app.navigation_scroll, 2);

        app.keep_selected_navigation_visible(7..14, 5);
        assert_eq!(app.navigation_scroll, 7);
    }

    #[test]
    fn dragging_the_sidebar_boundary_debounces_width_updates() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let initial_render_width = app.geometry.content.width;
        let boundary = app.geometry.sidebar_splitter.x;
        let splitter_row = app.geometry.sidebar_splitter.y;
        assert_eq!(boundary, DEFAULT_SIDEBAR_WIDTH);
        assert!(!app.is_sidebar_boundary(boundary.saturating_sub(1), splitter_row));
        assert!(app.is_sidebar_boundary(boundary, splitter_row));
        assert!(!app.is_sidebar_boundary(boundary, 0));
        let started = Instant::now();

        app.handle_pointer_control_at(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: boundary,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            started,
        );
        assert_eq!(app.pointer_drag, PointerDrag::Sidebar);
        app.handle_pointer_control_at(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 40,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            started,
        );
        terminal
            .draw(|frame| app.draw(frame))
            .expect("draw while resize is pending");
        assert_eq!(app.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
        assert_eq!(
            app.pending_sidebar_resize.map(|pending| pending.column),
            Some(40)
        );
        assert_eq!(app.pointer_drag, PointerDrag::Sidebar);
        assert_eq!(app.geometry.content.width, initial_render_width);
        assert_eq!(
            app.rendered_cache.keys().copied().collect::<HashSet<_>>(),
            HashSet::from([app.geometry.content.width])
        );

        app.handle_pointer_control_at(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 44,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            started + Duration::from_millis(1),
        );
        assert_eq!(app.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
        assert_eq!(
            app.pending_sidebar_resize.map(|pending| pending.column),
            Some(44)
        );
        app.tick(started + SIDEBAR_RESIZE_IDLE);
        assert_eq!(app.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
        app.tick(started + SIDEBAR_RESIZE_IDLE + Duration::from_millis(1));
        terminal
            .draw(|frame| app.draw(frame))
            .expect("draw final live width");
        assert_eq!(app.sidebar_width, 44);
        assert_eq!(app.pointer_drag, PointerDrag::Sidebar);
        assert_eq!(
            app.rendered_cache.keys().copied().collect::<HashSet<_>>(),
            HashSet::from([app.geometry.content.width])
        );

        app.handle_pointer_control_at(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 46,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            started + SIDEBAR_RESIZE_IDLE + Duration::from_millis(2),
        );

        assert_eq!(app.sidebar_width, 46);
        assert_eq!(app.pointer_drag, PointerDrag::None);
        assert!(app.pending_sidebar_resize.is_none());
    }

    #[test]
    fn pending_sidebar_drag_requests_redraw_only_after_the_idle_deadline() {
        let mut app = App::new(&navigation_bundle());
        app.geometry.body = Rect::new(0, 1, 100, 18);
        app.geometry.sidebar_splitter = Rect::new(DEFAULT_SIDEBAR_WIDTH, 1, 1, 18);
        let started = Instant::now();
        let pointer = |kind, column| MouseEvent {
            kind,
            column,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };

        assert_eq!(
            app.handle_pointer_control_at(
                pointer(
                    MouseEventKind::Down(MouseButton::Left),
                    DEFAULT_SIDEBAR_WIDTH,
                ),
                started,
            ),
            Some(UpdateOutcome::Unchanged)
        );
        assert_eq!(
            app.handle_pointer_control_at(
                pointer(MouseEventKind::Drag(MouseButton::Left), 44),
                started,
            ),
            Some(UpdateOutcome::Unchanged)
        );
        assert_eq!(
            app.tick(
                (started + SIDEBAR_RESIZE_IDLE)
                    .checked_sub(Duration::from_millis(1))
                    .expect("idle deadline is later than the start"),
            ),
            UpdateOutcome::Unchanged
        );
        assert_eq!(
            app.tick(started + SIDEBAR_RESIZE_IDLE),
            UpdateOutcome::Redraw
        );
        assert_eq!(app.sidebar_width, 44);
    }

    #[test]
    fn settled_sidebar_resize_keeps_the_visible_code_logically_anchored() {
        let mut bundle = navigation_bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks = vec![
            AstBlock::Paragraph {
                children: vec![Inline::Text {
                    value: "A long paragraph before the example repeats enough words to wrap very differently when the content pane changes width. ".repeat(8),
                }],
                layout: LayoutHint::default(),
                source: None,
            },
            AstBlock::Preformatted {
                children: vec![Inline::Text {
                    value: "sentinel_code_block();".to_owned(),
                }],
                language: None,
                layout: LayoutHint::default(),
                source: None,
            },
        ];
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&bundle);
        terminal
            .draw(|frame| app.draw(frame))
            .expect("initial draw");

        let initial_width = app.geometry.content.width;
        let initial_rendered = &app.rendered_cache[&initial_width];
        let code_row = initial_rendered.search("sentinel_code_block")[0].row;
        let logical_anchor = initial_rendered
            .viewport_anchor(code_row)
            .expect("code viewport anchor");
        app.content_scroll = code_row;

        let boundary = app.geometry.sidebar_splitter.x;
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: boundary,
            row: 6,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 50,
            row: 6,
            modifiers: KeyModifiers::NONE,
        });
        let deadline = app
            .pending_sidebar_resize
            .expect("pending sidebar resize")
            .deadline;
        app.tick(deadline);
        terminal
            .draw(|frame| app.draw(frame))
            .expect("resized draw");

        let resized = &app.rendered_cache[&app.geometry.content.width];
        assert_eq!(
            app.content_scroll,
            resized
                .row_for_viewport_anchor(logical_anchor)
                .expect("resized code anchor")
        );
        assert!(
            resized
                .viewport_text(app.content_scroll, 1, &[], None)
                .lines[0]
                .to_string()
                .contains("sentinel_code_block")
        );
    }

    #[test]
    fn sidebar_metadata_never_clips_the_tldr_label_mid_word() {
        assert_eq!(
            sidebar_metadata(10, 93, true, DEFAULT_SIDEBAR_WIDTH),
            " 10 top · 93 sections · TLDR"
        );
        assert_eq!(sidebar_metadata(10, 93, true, 8), " TLDR");
    }

    #[test]
    fn clicking_and_dragging_the_content_scrollbar_moves_the_document() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let scrollbar = app.geometry.content_scrollbar.expect("content scrollbar");
        let area = scrollbar.area();
        let maximum = scrollbar.maximum();
        assert!(area.height > 1);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.bottom() - 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.content_scroll, maximum);
        assert!(matches!(app.pointer_drag, PointerDrag::ContentScrollbar(_)));

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.content_scroll, 0);
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.pointer_drag, PointerDrag::None);
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
        assert!(app.search.matches.is_empty());
        assert!(app.search.mode.is_editing());

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.search.query, "show");
        assert_eq!(app.search.matches.len(), 1);
        assert!(!app.search.mode.is_editing());

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.search.mode, SearchMode::Closed);
        assert!(app.search.query.is_empty());
        assert!(app.search.matches.is_empty());
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
        let buffer = terminal.backend().buffer();
        let menu_left = MenuId::View.left();
        let menu_right = menu_left + 29;
        assert_eq!(
            buffer.cell((menu_left, 1)).expect("menu left edge").bg,
            theme::SELECTED
        );
        assert_eq!(
            buffer
                .cell((menu_left, 1))
                .expect("menu left padding")
                .symbol(),
            " "
        );
        assert_eq!(
            buffer.cell((menu_right, 1)).expect("menu right edge").bg,
            theme::SELECTED
        );
        assert_eq!(
            buffer
                .cell((menu_right, 1))
                .expect("menu right padding")
                .symbol(),
            " "
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
        app.geometry.content = Rect::new(0, 0, 80, 10);
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
        let width = app.geometry.content.width;
        let region = app.rendered_cache[&width]
            .search("nested")
            .into_iter()
            .next()
            .expect("visible reference text");

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: app.geometry.content.x
                + u16::try_from(region.start_column).expect("link column"),
            row: app.geometry.content.y + u16::try_from(region.row).expect("link row"),
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

        assert_eq!(app.search.draft, "ac界");
        assert_eq!(app.search.cursor, 2);
    }

    #[test]
    fn clicking_the_search_field_moves_its_unicode_aware_cursor() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "ab界".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        terminal.draw(|frame| app.draw(frame)).expect("draw search");

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: app.geometry.status.x + 8,
            row: app.geometry.status.y,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));

        assert_eq!(app.search.draft, "aXb界");
        assert_eq!(app.search.cursor, 2);
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
        assert!(app.search.matches.len() >= 2);

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.search.active_match, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.search.active_match, 0);

        app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
        assert_eq!(app.search.active_match, app.search.matches.len() - 1);
    }

    #[test]
    fn search_menu_actions_keep_confirmed_results_available() {
        let backend = TestBackend::new(100, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&navigation_bundle());
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "help".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.search.matches.len() >= 2);

        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.overlay, Overlay::None);
        assert!(app.search.mode.is_open());
        assert_eq!(app.search.active_match, app.search.matches.len() - 1);
    }
}
