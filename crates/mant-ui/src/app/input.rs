//! Routes keyboard and pointer events into the application state machine.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{App, Overlay, PointerDrag, UpdateOutcome, menu::MenuId};
use crate::layout::{MIN_SIDEBAR_WIDTH, maximum_sidebar_width};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> UpdateOutcome {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return UpdateOutcome::Unchanged;
        }
        if self.overlay != Overlay::None {
            self.handle_overlay_key(key);
            return UpdateOutcome::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.open_document_finder();
            return UpdateOutcome::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Left {
            self.navigate_history(true);
            return UpdateOutcome::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Right {
            self.navigate_history(false);
            return UpdateOutcome::Redraw;
        }
        if self.search.is_open() && key.code == KeyCode::F(10) {
            self.open_menu(MenuId::Manual);
            return UpdateOutcome::Redraw;
        }
        if self.search.is_open() {
            self.handle_search_key(key);
            return UpdateOutcome::Redraw;
        }
        if key.code == KeyCode::F(10) {
            self.open_menu(MenuId::Manual);
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
        if let Some(outcome) = self.handle_overlay_mouse(mouse) {
            return outcome;
        }
        if let Some(outcome) = self.handle_pointer_control(mouse) {
            return outcome;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if self.search.is_open()
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

    pub(super) fn handle_pointer_control_at(
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
                self.sidebar_resize.begin();
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
                PointerDrag::Sidebar => self.request_sidebar_resize(mouse.column, now),
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

    pub(super) fn is_sidebar_boundary(&self, column: u16, row: u16) -> bool {
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

    pub(super) fn commit_sidebar_at(&mut self, column: u16) -> bool {
        let width = self.sidebar_width_at(column);
        if self.sidebar_width == width {
            return false;
        }
        self.sidebar_width = width;
        true
    }

    fn request_sidebar_resize(&mut self, column: u16, now: Instant) -> UpdateOutcome {
        let Some(column) = self.sidebar_resize.request(column, now) else {
            return UpdateOutcome::Unchanged;
        };
        if self.commit_sidebar_at(column) {
            UpdateOutcome::Redraw
        } else {
            UpdateOutcome::Unchanged
        }
    }

    fn finish_sidebar_resize(&mut self, column: u16) {
        let column = self.sidebar_resize.finish(column);
        self.commit_sidebar_at(column);
    }
}
