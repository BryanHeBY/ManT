//! Defines the classic menu hierarchy independently from input and rendering.

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::{App, Overlay, UpdateOutcome, fit_to_width};
use crate::{CopyFormat, layout::DEFAULT_SIDEBAR_WIDTH, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuId {
    Manual,
    Edit,
    View,
    Navigate,
    Search,
    Help,
}

impl MenuId {
    pub(super) const ALL: [Self; 6] = [
        Self::Manual,
        Self::Edit,
        Self::View,
        Self::Navigate,
        Self::Search,
        Self::Help,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Edit => "Edit",
            Self::View => "View",
            Self::Navigate => "Navigate",
            Self::Search => "Search",
            Self::Help => "Help",
        }
    }

    pub(super) const fn left(self) -> u16 {
        match self {
            Self::Manual => 0,
            Self::Edit => 8,
            Self::View => 14,
            Self::Navigate => 20,
            Self::Search => 30,
            Self::Help => 38,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MenuEntry {
    pub(super) label: &'static str,
    pub(super) shortcut: &'static str,
    pub(super) action: MenuAction,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum MenuAction {
    Quit,
    OpenDocument,
    CopySelection,
    CopyNodeText,
    CopyNodeMarkdown,
    Back,
    Forward,
    ToggleSidebar,
    ToggleFullOutlineLabels,
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

const MANUAL_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Open Document…",
        shortcut: "Ctrl+O",
        action: MenuAction::OpenDocument,
    },
    MenuEntry {
        label: "Quit",
        shortcut: "q",
        action: MenuAction::Quit,
    },
];

const EDIT_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Copy Selection",
        shortcut: "y",
        action: MenuAction::CopySelection,
    },
    MenuEntry {
        label: "Copy Current Node as Text",
        shortcut: "",
        action: MenuAction::CopyNodeText,
    },
    MenuEntry {
        label: "Copy Current Node as Markdown",
        shortcut: "",
        action: MenuAction::CopyNodeMarkdown,
    },
];

const VIEW_MENU: &[MenuEntry] = &[
    MenuEntry {
        label: "Outline Sidebar",
        shortcut: "",
        action: MenuAction::ToggleSidebar,
    },
    MenuEntry {
        label: "Full Outline Labels",
        shortcut: "",
        action: MenuAction::ToggleFullOutlineLabels,
    },
    MenuEntry {
        label: "Reset Outline Width",
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
        label: "Back",
        shortcut: "Alt+←",
        action: MenuAction::Back,
    },
    MenuEntry {
        label: "Forward",
        shortcut: "Alt+→",
        action: MenuAction::Forward,
    },
    MenuEntry {
        label: "Previous Node",
        shortcut: "↑ / k",
        action: MenuAction::Previous,
    },
    MenuEntry {
        label: "Next Node",
        shortcut: "↓ / j",
        action: MenuAction::Next,
    },
    MenuEntry {
        label: "Parent Node",
        shortcut: "← / h",
        action: MenuAction::Parent,
    },
    MenuEntry {
        label: "First Child Node",
        shortcut: "→ / l",
        action: MenuAction::FirstChild,
    },
    MenuEntry {
        label: "First Node",
        shortcut: "",
        action: MenuAction::First,
    },
    MenuEntry {
        label: "Last Node",
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

pub(super) const fn menu_entries(id: MenuId) -> &'static [MenuEntry] {
    match id {
        MenuId::Manual => MANUAL_MENU,
        MenuId::Edit => EDIT_MENU,
        MenuId::View => VIEW_MENU,
        MenuId::Navigate => NAVIGATE_MENU,
        MenuId::Search => SEARCH_MENU,
        MenuId::Help => HELP_MENU,
    }
}

fn menu_overlay_width(id: MenuId) -> u16 {
    const PREFIX_WIDTH: usize = 4;
    const HORIZONTAL_MARGIN: usize = 2;
    let content_width = menu_entries(id)
        .iter()
        .map(|entry| {
            let shortcut_gap = usize::from(!entry.shortcut.is_empty());
            PREFIX_WIDTH + entry.label.width() + shortcut_gap + entry.shortcut.width()
        })
        .max()
        .unwrap_or_default();
    u16::try_from(content_width + HORIZONTAL_MARGIN)
        .unwrap_or(u16::MAX)
        .max(30)
}

impl App {
    pub(super) fn open_menu(&mut self, id: MenuId) {
        self.overlay = Overlay::Menu { id, cursor: 0 };
    }

    pub(super) fn handle_overlay_key(&mut self, key: KeyEvent) {
        match self.overlay {
            Overlay::None => {}
            Overlay::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::DocumentFinder => self.handle_finder_key(key),
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

    pub(super) fn handle_overlay_mouse(&mut self, mouse: MouseEvent) -> Option<UpdateOutcome> {
        if self.overlay == Overlay::DocumentFinder
            && matches!(self.pointer_drag, super::PointerDrag::FinderScrollbar(_))
            && matches!(
                mouse.kind,
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            )
        {
            return Some(self.handle_finder_mouse(mouse));
        }

        let menu_bar_target = (mouse.row == 0)
            .then(|| {
                MenuId::ALL.into_iter().find(|id| {
                    let start = id.left();
                    let end = start + u16::try_from(id.label().len() + 2).unwrap_or_default();
                    mouse.column >= start && mouse.column < end
                })
            })
            .flatten();

        if let Some(id) = menu_bar_target {
            return match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.pointer_drag = super::PointerDrag::None;
                    self.overlay = if matches!(self.overlay, Overlay::Menu { id: open, .. } if open == id)
                    {
                        Overlay::None
                    } else {
                        Overlay::Menu { id, cursor: 0 }
                    };
                    Some(UpdateOutcome::Redraw)
                }
                MouseEventKind::Moved if matches!(self.overlay, Overlay::Menu { .. }) => {
                    let changed =
                        !matches!(self.overlay, Overlay::Menu { id: open, .. } if open == id);
                    if changed {
                        self.overlay = Overlay::Menu { id, cursor: 0 };
                    }
                    Some(if changed {
                        UpdateOutcome::Redraw
                    } else {
                        UpdateOutcome::Unchanged
                    })
                }
                _ if self.overlay != Overlay::None => Some(UpdateOutcome::Unchanged),
                _ => None,
            };
        }

        if let Some(outcome) = self.handle_document_tab_mouse(mouse) {
            return Some(outcome);
        }

        if let Overlay::Menu { id, cursor } = self.overlay {
            let entries = menu_entries(id);
            let row = usize::from(mouse.row.saturating_sub(1));
            let entry = (mouse.row >= 1
                && row < entries.len()
                && mouse.column >= id.left()
                && mouse.column < id.left().saturating_add(menu_overlay_width(id)))
            .then_some(row);

            return match mouse.kind {
                MouseEventKind::Moved => {
                    let changed = entry.is_some_and(|index| index != cursor);
                    if let Some(index) = entry {
                        self.overlay = Overlay::Menu { id, cursor: index };
                    }
                    Some(if changed {
                        UpdateOutcome::Redraw
                    } else {
                        UpdateOutcome::Unchanged
                    })
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(index) = entry {
                        self.activate_menu_action(entries[index].action);
                    } else {
                        self.overlay = Overlay::None;
                    }
                    Some(UpdateOutcome::Redraw)
                }
                _ => Some(UpdateOutcome::Unchanged),
            };
        }

        if self.overlay == Overlay::Help {
            return Some(UpdateOutcome::Unchanged);
        }
        if self.overlay == Overlay::DocumentFinder {
            return Some(self.handle_finder_mouse(mouse));
        }

        None
    }

    pub(super) fn activate_menu_action(&mut self, action: MenuAction) {
        self.overlay = Overlay::None;
        match action {
            MenuAction::Quit => self.quit = true,
            MenuAction::OpenDocument => self.open_document_finder(),
            MenuAction::CopySelection => self.copy_selection(),
            MenuAction::CopyNodeText => self.copy_selected_node(CopyFormat::Text),
            MenuAction::CopyNodeMarkdown => self.copy_selected_node(CopyFormat::Markdown),
            MenuAction::Back => self.navigate_history(true),
            MenuAction::Forward => self.navigate_history(false),
            MenuAction::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            MenuAction::ToggleFullOutlineLabels => {
                self.full_outline_labels = !self.full_outline_labels;
                self.navigation_visibility_target = Some(self.selected);
            }
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

    pub(super) fn draw_menu(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let style = Style::default().bg(theme::MENU);
        frame.render_widget(Block::default().style(style), area);
        let open_menu = match self.overlay {
            Overlay::Menu { id, .. } => Some(id),
            Overlay::None | Overlay::DocumentFinder | Overlay::Help => None,
        };
        let spans = MenuId::ALL
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
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
        self.draw_document_tabs(frame, area, style);
    }

    pub(super) fn draw_overlay(&mut self, frame: &mut Frame<'_>) {
        if self.overlay != Overlay::DocumentFinder {
            self.clear_finder_geometry();
        }
        match self.overlay {
            Overlay::None => {}
            Overlay::Menu { id, cursor } => self.draw_menu_overlay(frame, id, cursor),
            Overlay::DocumentFinder => self.draw_document_finder(frame),
            Overlay::Help => Self::draw_help(frame),
        }
    }

    fn draw_menu_overlay(&self, frame: &mut Frame<'_>, id: MenuId, cursor: usize) {
        let entries = menu_entries(id);
        let height = u16::try_from(entries.len()).unwrap_or_default();
        let area = Rect::new(
            id.left().min(frame.area().width.saturating_sub(1)),
            1,
            menu_overlay_width(id).min(frame.area().width.saturating_sub(id.left())),
            height.min(frame.area().height.saturating_sub(1)),
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::BASE)),
            area,
        );
        for (index, entry) in entries.iter().enumerate().take(usize::from(area.height)) {
            let row = Rect::new(
                area.x,
                area.y + u16::try_from(index).unwrap_or_default(),
                area.width,
                1,
            );
            let content = row.inner(Margin {
                horizontal: 1,
                vertical: 0,
            });
            let active = index == cursor;
            let prefix = match entry.action {
                MenuAction::ToggleSidebar if self.show_sidebar => "[x] ",
                MenuAction::ToggleFullOutlineLabels if self.full_outline_labels => "[x] ",
                MenuAction::ToggleSidebar | MenuAction::ToggleFullOutlineLabels => "[ ] ",
                _ => "    ",
            };
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

    fn draw_help(frame: &mut Frame<'_>) {
        let width = 58.min(frame.area().width.saturating_sub(2));
        let height = 21.min(frame.area().height);
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
                Line::raw("↑/↓ or j/k  select outline node"),
                Line::raw("←/→ or h/l  move through the outline tree"),
                Line::raw("Enter        fold or unfold selected node"),
                Line::raw("Ctrl+O       find and open a document"),
                Line::raw("top tabs     switch opened documents"),
                Line::raw("Alt+←/→      back / forward"),
                Line::raw("Ctrl+F or /  find in current page"),
                Line::raw("n / N        next / previous search match"),
                Line::raw("drag / Shift+click  select+copy / extend"),
                Line::raw("right-click   copy selected plain text"),
                Line::raw("y / Ctrl+Shift+C  copy selected plain text"),
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
}
