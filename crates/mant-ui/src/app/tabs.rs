//! Maintains the bounded, first-opened document tab stack.

use std::sync::Arc;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use mant_protocol::DocumentAddress;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::{
    App, DocumentTab, HISTORY_LIMIT, HistoryDirection, NavigationRequest, Overlay, PointerDrag,
    UpdateOutcome, menu::MenuId,
};
use crate::theme;

const MAX_DOCUMENT_TAB_WIDTH: u16 = 28;

#[derive(Debug, Clone, Copy)]
struct VisibleDocumentTab {
    index: usize,
    width: u16,
}

impl App {
    pub(super) fn sync_current_document_tab(&mut self) {
        let existing = self.document_tabs.iter().position(|tab| {
            document_identity_matches(tab.address.as_ref(), self.current_address.as_ref())
        });
        let fallback = self
            .current_address
            .is_none()
            .then(|| Arc::clone(&self.current_bundle));
        let label = self.document.terminal_label().to_owned();
        let index = if let Some(index) = existing {
            let tab = &mut self.document_tabs[index];
            tab.label = label;
            tab.fallback = fallback;
            index
        } else {
            if self.document_tabs.len() == HISTORY_LIMIT {
                self.document_tabs.remove(0);
                self.active_document_tab = self.active_document_tab.saturating_sub(1);
                self.document_tab_scroll = self.document_tab_scroll.saturating_sub(1);
            }
            self.document_tabs.push(DocumentTab {
                address: self.current_address.clone(),
                fallback,
                label,
                target: None,
            });
            self.document_tabs.len() - 1
        };
        self.active_document_tab = index;
        self.document_tab_visibility_target = Some(index);
    }

    pub(super) fn remember_current_document_tab(&mut self) {
        let target = self
            .document
            .navigation()
            .get(self.selected)
            .map(|item| item.target_id.clone());
        if let Some(tab) = self.document_tabs.get_mut(self.active_document_tab) {
            tab.target = target;
        }
    }

    pub(super) fn activate_document_tab(&mut self, index: usize) -> UpdateOutcome {
        if index == self.active_document_tab || index >= self.document_tabs.len() {
            return UpdateOutcome::Unchanged;
        }
        self.remember_current_document_tab();
        let tab = self.document_tabs[index].clone();
        if let Some(address) = tab.address {
            self.pending_open = Some(NavigationRequest {
                address,
                target: tab.target,
                direction: HistoryDirection::New,
            });
        } else if let Some(bundle) = tab.fallback.as_deref() {
            let bundle = bundle.clone();
            self.complete_local_bundle(&bundle, tab.target, HistoryDirection::New);
        } else {
            return UpdateOutcome::Unchanged;
        }
        UpdateOutcome::Redraw
    }

    pub(super) fn scroll_document_tabs(&mut self, direction: isize) -> UpdateOutcome {
        let maximum = self.document_tabs.len().saturating_sub(1);
        let next = self
            .document_tab_scroll
            .saturating_add_signed(direction)
            .min(maximum);
        if next == self.document_tab_scroll {
            return UpdateOutcome::Unchanged;
        }
        self.document_tab_scroll = next;
        self.document_tab_visibility_target = None;
        UpdateOutcome::Redraw
    }

    pub(super) fn handle_document_tab_mouse(&mut self, mouse: MouseEvent) -> Option<UpdateOutcome> {
        if mouse.row != 0 || !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return None;
        }
        let outcome = if self
            .geometry
            .previous_document_tabs
            .contains((mouse.column, mouse.row).into())
        {
            Some(self.scroll_document_tabs(-1))
        } else if self
            .geometry
            .next_document_tabs
            .contains((mouse.column, mouse.row).into())
        {
            Some(self.scroll_document_tabs(1))
        } else {
            self.geometry
                .document_tabs
                .iter()
                .find(|tab| tab.area.contains((mouse.column, mouse.row).into()))
                .map(|tab| tab.index)
                .map(|index| self.activate_document_tab(index))
        }?;
        self.pointer_drag = PointerDrag::None;
        self.overlay = Overlay::None;
        Some(outcome)
    }

    pub(super) fn draw_document_tabs(&mut self, frame: &mut Frame<'_>, area: Rect, style: Style) {
        self.clear_document_tab_geometry();
        let left = area.x.saturating_add(menu_bar_width().min(area.width));
        let available = area.right().saturating_sub(left);
        if available == 0 || self.document_tabs.is_empty() {
            return;
        }
        if self.document_tab_view_width != available {
            self.document_tab_view_width = available;
            self.document_tab_visibility_target = Some(self.active_document_tab);
        }

        let widths = self
            .document_tabs
            .iter()
            .map(|tab| document_tab_width(&tab.label))
            .collect::<Vec<_>>();
        let total_width = widths.iter().copied().fold(0_u16, u16::saturating_add);
        let overflowing = self.document_tabs.len() > 1 && total_width > available;
        let show_controls = overflowing && available >= 5;
        let viewport_width = available.saturating_sub(u16::from(show_controls) * 2);
        if viewport_width == 0 {
            return;
        }

        let mut start = if overflowing {
            self.document_tab_scroll
                .min(self.document_tabs.len().saturating_sub(1))
        } else {
            0
        };
        if let Some(target) = self.document_tab_visibility_target.take() {
            start = start_for_visible_target(&widths, start, target, viewport_width);
        }
        let visible = visible_document_tabs(&widths, start, viewport_width);
        self.document_tab_scroll = start;
        let end = visible.last().map_or(start, |tab| tab.index + 1);
        let used_width = visible
            .iter()
            .map(|tab| tab.width)
            .fold(0_u16, u16::saturating_add);
        let tabs_left = if show_controls {
            left.saturating_add(1)
        } else if overflowing {
            left
        } else {
            area.right().saturating_sub(used_width)
        };
        Self::draw_document_tab_rule(frame, area.y, style, left, tabs_left);
        self.draw_document_tab_controls(frame, area, style, show_controls, start, end);
        self.draw_visible_document_tabs(frame, area.y, style, tabs_left, visible);
    }

    fn clear_document_tab_geometry(&mut self) {
        self.geometry.document_tabs.clear();
        self.geometry.previous_document_tabs = Rect::default();
        self.geometry.next_document_tabs = Rect::default();
    }

    fn draw_document_tab_rule(
        frame: &mut Frame<'_>,
        row: u16,
        style: Style,
        left: u16,
        tabs_left: u16,
    ) {
        let width = tabs_left.saturating_sub(left);
        if width > 0 {
            frame.render_widget(
                Paragraph::new("─".repeat(usize::from(width))).style(style.fg(theme::BORDER)),
                Rect::new(left, row, width, 1),
            );
        }
    }

    fn draw_document_tab_controls(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        style: Style,
        overflowing: bool,
        start: usize,
        end: usize,
    ) {
        if !overflowing {
            return;
        }
        let previous = Rect::new(area.x + menu_bar_width(), area.y, 1, 1);
        let next = Rect::new(area.right().saturating_sub(1), area.y, 1, 1);
        let has_previous = start > 0;
        let has_next = end < self.document_tabs.len();
        for (control, symbol, enabled) in [(previous, "‹", has_previous), (next, "›", has_next)]
        {
            frame.render_widget(
                Paragraph::new(if enabled { symbol } else { " " })
                    .alignment(Alignment::Center)
                    .style(style.fg(if enabled {
                        theme::SUBTEXT_BRIGHT
                    } else {
                        theme::BORDER
                    })),
                control,
            );
        }
        if has_previous {
            self.geometry.previous_document_tabs = previous;
        }
        if has_next {
            self.geometry.next_document_tabs = next;
        }
    }

    fn draw_visible_document_tabs(
        &mut self,
        frame: &mut Frame<'_>,
        row: u16,
        style: Style,
        mut cursor: u16,
        visible: Vec<VisibleDocumentTab>,
    ) {
        for tab in visible {
            let area = Rect::new(cursor, row, tab.width, 1);
            frame.render_widget(
                Paragraph::new("│").style(style.fg(theme::BORDER)),
                Rect::new(cursor, row, 1.min(tab.width), 1),
            );
            if tab.width > 1 {
                self.draw_document_tab_label(frame, area, style, tab.index);
            }
            self.geometry.document_tabs.push(super::DocumentTabHit {
                area,
                index: tab.index,
            });
            cursor = cursor.saturating_add(tab.width);
        }
    }

    fn draw_document_tab_label(
        &self,
        frame: &mut Frame<'_>,
        tab_area: Rect,
        style: Style,
        index: usize,
    ) {
        let area = Rect::new(tab_area.x + 1, tab_area.y, tab_area.width - 1, 1);
        let style = if index == self.active_document_tab {
            Style::default()
                .fg(theme::SELECTED_TEXT)
                .bg(theme::SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            style.fg(theme::SUBTEXT)
        };
        frame.render_widget(
            Paragraph::new(document_tab_label(
                &self.document_tabs[index].label,
                area.width,
            ))
            .style(style),
            area,
        );
    }
}

fn menu_bar_width() -> u16 {
    MenuId::ALL
        .into_iter()
        .map(|id| {
            id.left().saturating_add(
                u16::try_from(id.label().width().saturating_add(2)).unwrap_or(u16::MAX),
            )
        })
        .max()
        .unwrap_or_default()
}

fn document_tab_width(label: &str) -> u16 {
    let label = crate::text::sanitize_terminal_text(label);
    u16::try_from(label.width().saturating_add(3))
        .unwrap_or(u16::MAX)
        .clamp(3, MAX_DOCUMENT_TAB_WIDTH)
}

fn document_tab_label(label: &str, width: u16) -> String {
    let label = crate::text::sanitize_terminal_text(label);
    let width = usize::from(width);
    if width < 2 {
        return crate::navigation::truncate_middle(&label, width);
    }
    let content_width = width - 2;
    let label = crate::navigation::truncate_middle(&label, content_width);
    let padding = content_width.saturating_sub(label.width());
    format!(" {label}{} ", " ".repeat(padding))
}

fn visible_document_tabs(
    widths: &[u16],
    start: usize,
    viewport_width: u16,
) -> Vec<VisibleDocumentTab> {
    let mut remaining = viewport_width;
    let mut visible = Vec::new();
    for (index, width) in widths.iter().copied().enumerate().skip(start) {
        if remaining == 0 {
            break;
        }
        let width = width.min(viewport_width);
        if width > remaining && !visible.is_empty() {
            break;
        }
        let width = width.min(remaining);
        visible.push(VisibleDocumentTab { index, width });
        remaining -= width;
    }
    visible
}

fn start_for_visible_target(
    widths: &[u16],
    current_start: usize,
    target: usize,
    viewport_width: u16,
) -> usize {
    let target = target.min(widths.len().saturating_sub(1));
    let mut start = current_start.min(target);
    while !visible_document_tabs(widths, start, viewport_width)
        .iter()
        .any(|tab| tab.index == target)
        && start < target
    {
        start += 1;
    }
    while start > 0
        && visible_document_tabs(widths, start - 1, viewport_width)
            .iter()
            .any(|tab| tab.index == target)
    {
        start -= 1;
    }
    start
}

fn document_identity_matches(
    left: Option<&DocumentAddress>,
    right: Option<&DocumentAddress>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}
