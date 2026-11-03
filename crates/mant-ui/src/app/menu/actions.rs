//! Overlay input routing and menu actions over the existing app state.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::super::{App, Overlay, UpdateOutcome};
use super::presentation::{menu_overlay_left, menu_overlay_width};
use super::{MenuAction, MenuId, menu_entries};
use crate::{CopyFormat, layout::DEFAULT_SIDEBAR_WIDTH};

impl App {
    pub(in crate::app) fn open_menu(&mut self, id: MenuId) {
        self.overlay = Overlay::Menu { id, cursor: 0 };
    }

    pub(in crate::app) fn handle_overlay_key(&mut self, key: KeyEvent) {
        match self.overlay {
            Overlay::None => {}
            Overlay::Help => match key.code {
                KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('?') => self.overlay = Overlay::None,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.help_scroll = self.help_scroll.saturating_add(1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                KeyCode::PageDown => self.help_scroll = self.help_scroll.saturating_add(8),
                KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(8),
                KeyCode::Home | KeyCode::Char('g') => self.help_scroll = 0,
                KeyCode::End | KeyCode::Char('G') => self.help_scroll = usize::MAX,
                _ => {}
            },
            Overlay::DocumentFinder => self.handle_finder_key(key),
            Overlay::References(_) => self.handle_reference_key(key),
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
                _ => {
                    if let Some(next) = MenuId::from_mnemonic_key(key, true) {
                        self.open_menu(next);
                    }
                }
            },
        }
    }

    pub(in crate::app) fn handle_overlay_mouse(
        &mut self,
        mouse: MouseEvent,
    ) -> Option<UpdateOutcome> {
        if self.overlay.references().is_some() {
            return Some(self.handle_reference_mouse(mouse));
        }
        if self.overlay == Overlay::DocumentFinder
            && matches!(
                self.pointer.drag(),
                super::super::PointerDrag::FinderScrollbar(_)
            )
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
                    self.pointer.finish_drag();
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
            let left = menu_overlay_left(id, self.geometry.body.width);
            let row = usize::from(mouse.row.saturating_sub(1));
            let entry = (mouse.row >= 1
                && row < entries.len()
                && mouse.column >= left
                && mouse.column < left.saturating_add(menu_overlay_width(id)))
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

    pub(in crate::app) fn activate_menu_action(&mut self, action: MenuAction) {
        self.overlay = Overlay::None;
        match action {
            MenuAction::Quit => self.quit = true,
            MenuAction::OpenDocument => self.open_document_finder(),
            MenuAction::CopySelection => self.copy_selection(),
            MenuAction::CopyNodeText => self.copy_selected_node(CopyFormat::Text),
            MenuAction::CopyNodeMarkdown => self.copy_selected_node(CopyFormat::Markdown),
            MenuAction::CopyReference => self.copy_selected_reference(),
            MenuAction::OpenReference => {
                self.show_reference_chooser(super::super::references::ReferencePurpose::Open);
            }
            MenuAction::Back => self.navigate_history(true),
            MenuAction::Forward => self.navigate_history(false),
            MenuAction::ToggleSidebar => self.toggle_sidebar(),
            MenuAction::ToggleFullOutlineLabels => {
                let row = self.selected_navigation_viewport_row();
                self.full_outline_labels = !self.full_outline_labels;
                self.preserve_selected_navigation_row(row);
            }
            MenuAction::ResetSidebar => {
                self.commit_sidebar_width(DEFAULT_SIDEBAR_WIDTH);
            }
            MenuAction::ExpandAll => {
                let row = self.selected_navigation_viewport_row();
                self.expanded = self
                    .session
                    .document
                    .navigation()
                    .iter()
                    .filter(|item| item.has_children)
                    .map(|item| item.id.clone())
                    .collect();
                self.preserve_selected_navigation_row(row);
            }
            MenuAction::CollapseAll => {
                let row = self.selected_navigation_viewport_row();
                self.expanded.clear();
                self.outline_search.revealed.clear();
                self.select_nearest_visible_ancestor();
                self.preserve_selected_navigation_row(row);
            }
            MenuAction::Previous => self.select_relative(-1),
            MenuAction::Next => self.select_relative(1),
            MenuAction::Parent => self.collapse_or_select_parent(),
            MenuAction::FirstChild => self.expand_or_select_child(),
            MenuAction::First => self.select_edge(false),
            MenuAction::Last => self.select_edge(true),
            MenuAction::Find => {
                self.open_search();
                self.search.direction = 1;
            }
            MenuAction::FindBackward => {
                self.open_search();
                self.search.direction = -1;
            }
            MenuAction::SearchCase | MenuAction::SearchSyntax | MenuAction::SearchWrap => {
                if !self.search_is_open() {
                    if self.last_search_target == super::super::search::SearchTarget::Outline {
                        self.open_outline_search();
                    } else {
                        self.open_search();
                    }
                    let input = self.active_search_input_mut();
                    input.draft.clone_from(&input.query);
                    input.cursor = input.draft.len();
                }
                self.handle_search_key(KeyEvent::new(
                    KeyCode::Char(match action {
                        MenuAction::SearchCase => 'c',
                        MenuAction::SearchSyntax => 'r',
                        _ => 'w',
                    }),
                    if matches!(action, MenuAction::SearchSyntax) {
                        KeyModifiers::CONTROL
                    } else {
                        KeyModifiers::ALT
                    },
                ));
            }
            MenuAction::FindOutline => self.open_outline_search(),
            MenuAction::FindNext => self.select_last_search_relative(1),
            MenuAction::FindPrevious => self.select_last_search_relative(-1),
            MenuAction::Help => {
                self.help_scroll = 0;
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
}
