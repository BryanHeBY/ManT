//! Routes keyboard and pointer events into the application state machine.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{
    App, NAVIGATION_SYNC_IDLE, Overlay, PointerDrag, SELECTION_AUTO_SCROLL_INTERVAL,
    SelectionAutoScroll, UpdateOutcome, menu::MenuId,
};
use crate::layout::{MIN_SIDEBAR_WIDTH, maximum_sidebar_width};

impl App {
    /// Apply one keyboard event and report whether the terminal needs repainting.
    pub fn handle_key(&mut self, key: KeyEvent) -> UpdateOutcome {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && key.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            self.copy_selection();
            return UpdateOutcome::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return UpdateOutcome::Unchanged;
        }
        if self.overlay != Overlay::None {
            self.handle_overlay_key(key);
            return UpdateOutcome::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('o') {
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
        if key.code == KeyCode::Esc && self.selection.take().is_some() {
            self.selection_auto_scroll = None;
            return UpdateOutcome::Redraw;
        }
        match key.code {
            KeyCode::Char('q' | 'Q') => self.quit = true,
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.select_search_relative(-1);
            }
            KeyCode::Char('n') => self.select_search_relative(1),
            KeyCode::Char('N') => self.select_search_relative(-1),
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
            KeyCode::Char('y') => self.copy_selection(),
            KeyCode::Char('<') => {
                self.commit_sidebar_width(
                    self.sidebar_width.saturating_sub(2).max(MIN_SIDEBAR_WIDTH),
                );
            }
            KeyCode::Char('>') => {
                self.commit_sidebar_width(
                    self.sidebar_width
                        .saturating_add(2)
                        .min(maximum_sidebar_width(self.geometry.body.width)),
                );
            }
            _ => return UpdateOutcome::Unchanged,
        }
        UpdateOutcome::Redraw
    }

    /// Apply one mouse event using geometry retained from the last frame.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> UpdateOutcome {
        if let Some(outcome) = self.handle_overlay_mouse(mouse) {
            return outcome;
        }
        if let Some(outcome) = self.handle_pointer_control(mouse) {
            return outcome;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Right)
                if self.pointer_drag == PointerDrag::None
                    && self
                        .selection
                        .is_some_and(|selection| !selection.is_empty())
                    && self
                        .geometry
                        .content
                        .contains((mouse.column, mouse.row).into()) =>
            {
                self.copy_selection();
                UpdateOutcome::Redraw
            }
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
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.selection_auto_scroll = None;
        }
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
            MouseEventKind::Down(MouseButton::Left)
                if self
                    .geometry
                    .content
                    .contains((mouse.column, mouse.row).into()) =>
            {
                self.begin_content_selection(mouse)
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
                PointerDrag::ContentSelection { moved } => {
                    self.update_content_selection(mouse, moved, now)
                }
                PointerDrag::FinderScrollbar(_) | PointerDrag::None => return None,
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
                    PointerDrag::ContentSelection { moved } => {
                        self.selection_auto_scroll = None;
                        self.finish_content_selection(mouse, moved);
                    }
                    PointerDrag::FinderScrollbar(_) | PointerDrag::None => {}
                }
                self.pointer_drag = PointerDrag::None;
                UpdateOutcome::Redraw
            }
            _ => return None,
        };
        Some(outcome)
    }

    fn begin_content_selection(&mut self, mouse: MouseEvent) -> UpdateOutcome {
        let position = self
            .content_text_position(mouse.column, mouse.row, false)
            .expect("content containment guarantees a text position");
        let extend = mouse.modifiers.contains(KeyModifiers::SHIFT) && self.selection.is_some();
        if extend {
            let retained_anchor = self
                .selection
                .expect("extension requires a retained selection")
                .anchor;
            self.selection = Some(crate::RenderedSelection {
                anchor: retained_anchor,
                focus: position,
            });
        } else {
            self.selection = Some(crate::RenderedSelection::new(position));
        }
        self.pointer_drag = PointerDrag::ContentSelection { moved: extend };
        UpdateOutcome::Redraw
    }

    fn update_content_selection(
        &mut self,
        mouse: MouseEvent,
        moved: bool,
        now: Instant,
    ) -> UpdateOutcome {
        let direction = self.selection_scroll_direction(mouse.row);
        let scrolling = direction
            .is_some_and(|direction| self.advance_selection_scroll(direction, mouse.column, now));
        self.selection_auto_scroll = if scrolling {
            direction.map(|direction| SelectionAutoScroll {
                direction,
                column: mouse.column,
                deadline: now + SELECTION_AUTO_SCROLL_INTERVAL,
            })
        } else {
            None
        };
        self.update_selection_focus(mouse.column, mouse.row);
        if let Some(selection) = self.selection {
            self.pointer_drag = PointerDrag::ContentSelection {
                moved: moved || selection.focus != selection.anchor,
            };
        }
        UpdateOutcome::Redraw
    }

    fn selection_scroll_direction(&self, row: u16) -> Option<isize> {
        let area = self.geometry.content;
        if area.height == 0 {
            None
        } else if row <= area.y {
            Some(-1)
        } else if row >= area.bottom().saturating_sub(1) {
            Some(1)
        } else {
            None
        }
    }

    fn advance_selection_scroll(&mut self, direction: isize, column: u16, now: Instant) -> bool {
        let Some(maximum) = self
            .geometry
            .content_scrollbar
            .map(crate::scrollbar::VerticalScrollbar::maximum)
        else {
            return false;
        };
        let next = self
            .content_scroll
            .saturating_add_signed(direction)
            .min(maximum);
        if next == self.content_scroll {
            return false;
        }
        self.content_scroll = next;
        self.navigation_sync_deadline = Some(now + NAVIGATION_SYNC_IDLE);
        let row = if direction < 0 {
            self.geometry.content.y
        } else {
            self.geometry.content.bottom().saturating_sub(1)
        };
        self.update_selection_focus(column, row);
        true
    }

    fn update_selection_focus(&mut self, column: u16, row: u16) {
        if let Some(position) = self.content_text_position(column, row, true)
            && let Some(selection) = &mut self.selection
        {
            selection.focus = position;
        }
    }

    pub(super) fn tick_selection_auto_scroll(&mut self, now: Instant) -> bool {
        let Some(scroll) = self
            .selection_auto_scroll
            .filter(|scroll| scroll.deadline <= now)
        else {
            return false;
        };
        if !matches!(self.pointer_drag, PointerDrag::ContentSelection { .. })
            || !self.advance_selection_scroll(scroll.direction, scroll.column, now)
        {
            self.selection_auto_scroll = None;
            return false;
        }
        self.selection_auto_scroll = Some(SelectionAutoScroll {
            deadline: now + SELECTION_AUTO_SCROLL_INTERVAL,
            ..scroll
        });
        if let Some(selection) = self.selection {
            self.pointer_drag = PointerDrag::ContentSelection {
                moved: selection.focus != selection.anchor,
            };
        }
        true
    }

    fn finish_content_selection(&mut self, mouse: MouseEvent, moved: bool) {
        if let Some(position) = self.content_text_position(mouse.column, mouse.row, true)
            && let Some(selection) = &mut self.selection
        {
            selection.focus = position;
        }
        let activate_link = !moved
            && self
                .selection
                .is_some_and(crate::RenderedSelection::is_empty)
            && self
                .geometry
                .content
                .contains((mouse.column, mouse.row).into());
        if activate_link {
            self.selection = None;
            self.activate_content_link(mouse.column, mouse.row);
        } else if moved {
            self.copy_selection();
        }
    }

    fn content_text_position(
        &self,
        column: u16,
        row: u16,
        clamp: bool,
    ) -> Option<crate::TextPosition> {
        let area = self.geometry.content;
        if area.width == 0 || area.height == 0 {
            return None;
        }
        if !clamp && !area.contains((column, row).into()) {
            return None;
        }
        let column = column.clamp(area.x, area.x.saturating_add(area.width - 1));
        let row = row.clamp(area.y, area.y.saturating_add(area.height - 1));
        Some(crate::TextPosition {
            row: self.content_scroll + usize::from(row - area.y),
            column: usize::from(column - area.x),
        })
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
        self.commit_sidebar_width(width)
    }

    pub(super) fn commit_sidebar_width(&mut self, width: u16) -> bool {
        if self.sidebar_width == width {
            return false;
        }
        let row = self.selected_navigation_viewport_row();
        self.sidebar_width = width;
        self.preserve_selected_navigation_row(row);
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
