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
            .session
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.session.document.render(width));
        let mut selected = None;
        for index in visible {
            let item = &self.session.document.navigation()[index];
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
        let Some(item) = self.session.document.navigation().get(self.selected) else {
            return;
        };
        let width = self.geometry.content.width.max(1);
        let rendered = self
            .session
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.session.document.render(width));
        if let Some(row) = rendered.anchor_row(&item.target_id) {
            self.session.content_scroll = row;
        }
    }

    pub(super) fn activate_content_link(&mut self, column: u16, row: u16) {
        let width = self.geometry.content.width.max(1);
        let rendered = self
            .session
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.session.document.render(width));
        let document_row = self.session.content_scroll + usize::from(row - self.geometry.content.y);
        let document_column = usize::from(column - self.geometry.content.x);
        let Some(target) = rendered
            .link_target_at(document_row, document_column)
            .cloned()
        else {
            return;
        };
        self.activate_link_target(target);
    }

    pub(super) fn open_selected_reference(&mut self) {
        let Some(node) = self.session.document.navigation().get(self.selected) else {
            return;
        };
        let Some(target) = self.session.document.reference_target(&node.id) else {
            self.toggle_selected();
            return;
        };
        let Some(target) = self.session.document.activation_target(target) else {
            self.report_notice(
                "This reference has no registered document context; its target was not opened"
                    .into(),
            );
            return;
        };
        self.activate_link_target(target);
    }

    pub(super) fn activate_link_target(&mut self, target: LinkTarget) {
        match target {
            LinkTarget::Section(target) => {
                let current = self.current_location();
                if self.jump_to_anchor(&target) {
                    self.navigation
                        .commit(super::HistoryDirection::New, current);
                }
            }
            LinkTarget::Document { address, fragment } => {
                self.request_open(address, fragment);
            }
            LinkTarget::Manual {
                name,
                manual_section,
            } => {
                self.pending_open = Some(super::NavigationRequest {
                    document: mant_protocol::DocumentOpenTarget::Manual {
                        name,
                        manual_section,
                    },
                    target: super::LocalTarget::Default,
                    direction: super::HistoryDirection::New,
                });
            }
            LinkTarget::External(uri) => self.pending_external = Some(uri),
        }
    }

    pub(super) fn jump_to_anchor(&mut self, target: &str) -> bool {
        if let Err(message) = super::validate_fragment(&self.session.current_bundle, target) {
            self.report_open_error(message);
            return false;
        }
        self.reveal_anchor(target)
    }

    pub(super) fn reveal_anchor(&mut self, target: &str) -> bool {
        let width = self.geometry.content.width.max(1);
        let rendered = self
            .session
            .rendered_cache
            .entry(width)
            .or_insert_with(|| self.session.document.render(width));
        let Some(target_row) = rendered.anchor_row(target) else {
            self.notice = Some(format!("No outline node matches #{target}"));
            return false;
        };
        self.notice = None;
        self.session.content_scroll = target_row;
        if let Some(index) = self
            .session
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
        let mut parent = self.session.document.navigation()[index]
            .parent_id
            .as_deref();
        while let Some(parent_id) = parent {
            self.expanded.insert(parent_id.to_owned());
            parent = self
                .session
                .document
                .navigation()
                .iter()
                .find(|item| item.id == parent_id)
                .and_then(|item| item.parent_id.as_deref());
        }
    }

    pub(super) fn scroll_content(&mut self, delta: isize) {
        self.session.content_scroll = self.session.content_scroll.saturating_add_signed(delta);
        self.schedule_navigation_sync();
    }

    pub(super) fn schedule_navigation_sync(&mut self) {
        self.navigation_sync_deadline = Some(Instant::now() + NAVIGATION_SYNC_IDLE);
    }

    pub(super) fn scroll_content_to_pointer(&mut self, row: u16, drag: ScrollbarDrag) {
        if let Some(scrollbar) = self.geometry.content_scrollbar {
            self.session.content_scroll = scrollbar.position_for_pointer(row, drag);
            self.schedule_navigation_sync();
        }
    }

    pub(super) fn scroll_navigation_to_pointer(&mut self, row: u16, drag: ScrollbarDrag) {
        if let Some(scrollbar) = self.geometry.navigation_scrollbar {
            self.navigation_scroll = scrollbar.position_for_pointer(row, drag);
        }
    }

    pub(super) fn jump_content(&mut self, end: bool) {
        self.session.content_scroll = if end { usize::MAX } else { 0 };
        self.navigation_sync_deadline = Some(Instant::now() + NAVIGATION_SYNC_IDLE);
    }

    pub(super) fn sync_selection_to_scroll(&mut self) {
        self.select_section_at_row(self.session.content_scroll);
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
        for (index, item) in self.session.document.navigation().iter().enumerate() {
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

        let Some(selected) = self.session.document.navigation().get(self.selected) else {
            return;
        };
        let mut parent = selected.parent_id.as_deref();
        while let Some(parent_id) = parent {
            if let Some(index) = self
                .session
                .document
                .navigation()
                .iter()
                .position(|item| item.id == parent_id)
            {
                if visible.contains(&index) {
                    self.set_selected_index(index);
                    return;
                }
                parent = self.session.document.navigation()[index]
                    .parent_id
                    .as_deref();
            } else {
                break;
            }
        }
        if let Some(index) = visible.first().copied() {
            self.set_selected_index(index);
        }
    }

    pub(super) fn toggle_selected(&mut self) {
        let Some(item) = self.session.document.navigation().get(self.selected) else {
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
        let Some(item) = self.session.document.navigation().get(self.selected) else {
            return;
        };
        if item.has_children && self.expanded.remove(&item.id) {
            return;
        }
        let Some(parent_id) = item.parent_id.as_deref() else {
            return;
        };
        if let Some(index) = self
            .session
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
        let Some(item) = self.session.document.navigation().get(self.selected) else {
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
            .session
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
