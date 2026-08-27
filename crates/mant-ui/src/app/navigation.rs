//! Maintains the invariants between outline selection, folding, and document scrolling.

use std::{collections::HashSet, time::Instant};

use super::{App, NAVIGATION_SYNC_IDLE, NavigationViewportRequest};
use crate::{NavKind, document::LinkTarget, scrollbar::ScrollbarDrag};

impl App {
    pub(super) fn select_relative(&mut self, delta: isize) {
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

    pub(super) fn set_selected_index(&mut self, index: usize) {
        self.selected = index;
        self.navigation_viewport_request =
            Some(NavigationViewportRequest::Reveal { node_index: index });
    }

    pub(super) fn selected_navigation_viewport_row(&self) -> Option<usize> {
        self.geometry
            .navigation_rows
            .iter()
            .position(|index| *index == self.selected)
    }

    pub(super) fn preserve_selected_navigation_row(&mut self, row: Option<usize>) {
        self.navigation_viewport_request = Some(row.map_or(
            NavigationViewportRequest::Reveal {
                node_index: self.selected,
            },
            |row| NavigationViewportRequest::PreserveRow {
                node_index: self.selected,
                row,
            },
        ));
    }

    pub(super) fn select_section_at_row(&mut self, row: usize) {
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

    pub(super) fn scroll_to_selected(&mut self) {
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

    pub(super) fn activate_content_link(&mut self, column: u16, row: u16) {
        let width = self.geometry.content.width.max(1);
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        let document_row = self.content_scroll + usize::from(row - self.geometry.content.y);
        let document_column = usize::from(column - self.geometry.content.x);
        let Some(target) = rendered
            .link_target_at(document_row, document_column)
            .cloned()
        else {
            return;
        };
        match target {
            LinkTarget::Section(target) => {
                let current = self.current_location();
                if self.jump_to_anchor(&target) {
                    super::push_history(&mut self.back_history, current);
                    self.forward_history.clear();
                }
            }
            LinkTarget::Document { address, fragment } => {
                self.request_open(address, fragment);
            }
            LinkTarget::External(uri) => self.pending_external = Some(uri),
        }
    }

    pub(super) fn jump_to_anchor(&mut self, target: &str) -> bool {
        let width = self.geometry.content.width.max(1);
        let rendered = self
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.document.render(width));
        let Some(target_row) = rendered.anchor_row(target) else {
            self.notice = Some(format!("No outline node matches #{target}"));
            return false;
        };
        self.notice = None;
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
        true
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

    pub(super) fn scroll_content(&mut self, delta: isize) {
        self.content_scroll = self.content_scroll.saturating_add_signed(delta);
        self.schedule_navigation_sync();
    }

    pub(super) fn schedule_navigation_sync(&mut self) {
        self.navigation_sync_deadline = Some(Instant::now() + NAVIGATION_SYNC_IDLE);
    }

    pub(super) fn scroll_content_to_pointer(&mut self, row: u16, drag: ScrollbarDrag) {
        if let Some(scrollbar) = self.geometry.content_scrollbar {
            self.content_scroll = scrollbar.position_for_pointer(row, drag);
            self.schedule_navigation_sync();
        }
    }

    pub(super) fn scroll_navigation_to_pointer(&mut self, row: u16, drag: ScrollbarDrag) {
        if let Some(scrollbar) = self.geometry.navigation_scrollbar {
            self.navigation_scroll = scrollbar.position_for_pointer(row, drag);
        }
    }

    pub(super) fn jump_content(&mut self, end: bool) {
        self.content_scroll = if end { usize::MAX } else { 0 };
        self.navigation_sync_deadline = Some(Instant::now() + NAVIGATION_SYNC_IDLE);
    }

    pub(super) fn sync_selection_to_scroll(&mut self) {
        self.select_section_at_row(self.content_scroll);
    }

    pub(super) fn keep_selected_navigation_visible(
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

    pub(super) fn keep_selected_navigation_at_row(
        &mut self,
        selected: std::ops::Range<usize>,
        height: usize,
        preferred_row: usize,
        maximum: usize,
    ) {
        if height == 0 {
            return;
        }
        let selected_height = selected.end.saturating_sub(selected.start);
        let maximum_row = if selected_height < height {
            height - selected_height
        } else {
            height - 1
        };
        let row = preferred_row.min(maximum_row);
        self.navigation_scroll = selected.start.saturating_sub(row).min(maximum);
    }

    pub(super) fn visible_navigation_indices(&self) -> Vec<usize> {
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

    pub(super) fn visible_node_count(&self) -> usize {
        self.visible_navigation_indices().len()
    }

    pub(super) fn select_nearest_visible_ancestor(&mut self) {
        let visible = self.visible_navigation_indices();
        if visible.contains(&self.selected) {
            return;
        }

        let Some(selected) = self.document.navigation().get(self.selected) else {
            return;
        };
        let mut parent = selected.parent_id.as_deref();
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

    pub(super) fn toggle_selected(&mut self) {
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

    pub(super) fn collapse_or_select_parent(&mut self) {
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

    pub(super) fn expand_or_select_child(&mut self) {
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
