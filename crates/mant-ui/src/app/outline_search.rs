//! Current-snapshot outline finding and temporary ancestor visibility.

use crossterm::event::KeyEvent;
use std::{borrow::Cow, collections::HashSet};

use super::{
    App,
    search::{SearchCommand, SearchInput},
};
use crate::{document::OutlineRecord, navigation::search::OutlineHit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchTarget {
    Page,
    Outline,
}

#[derive(Debug, Default)]
pub(super) struct OutlineSearchState {
    pub(super) input: SearchInput,
    pub(super) records: Vec<OutlineRecord>,
    pub(super) matches: Vec<OutlineHit>,
    pub(super) active_match: usize,
    pub(super) limited: bool,
    index_limited: bool,
    indexed: bool,
    pub(super) revealed: HashSet<String>,
}

impl App {
    pub(super) fn active_search_input(&self) -> &SearchInput {
        match self.search_target {
            SearchTarget::Page => &self.search,
            SearchTarget::Outline => &self.outline_search.input,
        }
    }
    pub(super) fn active_search_input_mut(&mut self) -> &mut SearchInput {
        match self.search_target {
            SearchTarget::Page => &mut self.search,
            SearchTarget::Outline => &mut self.outline_search.input,
        }
    }
    pub(super) fn search_is_open(&self) -> bool {
        self.active_search_input().is_open()
    }
    pub(super) fn search_prompt(&self) -> &'static str {
        match self.search_target {
            SearchTarget::Page => " Find Page: ",
            SearchTarget::Outline => " Find Outline: ",
        }
    }
    pub(super) fn open_outline_search(&mut self) {
        if self.search_target == SearchTarget::Outline && self.outline_search.input.is_open() {
            return;
        }
        self.search.suspend();
        self.search_target = SearchTarget::Outline;
        self.show_sidebar = true;
        if !self.outline_search.indexed {
            let (records, limited) = self.session.document.outline_search_records();
            self.outline_search.records = records;
            self.outline_search.index_limited = limited;
            self.outline_search.indexed = true;
        }
        self.outline_search.input.open();
    }
    pub(super) fn close_outline_search(&mut self) {
        self.finish_outline_search(false);
    }

    pub(super) fn finish_outline_search(&mut self, suspend: bool) {
        if self.outline_search.input.is_open() {
            // Only the current selection's path becomes a lasting expansion.
            // User-owned folds were never overwritten by search exploration.
            if !self.outline_search.revealed.is_empty() {
                self.expand_navigation_ancestors(self.selected);
            }
            if suspend {
                self.outline_search.input.suspend();
            } else {
                self.outline_search.input.close();
            }
            self.outline_search.revealed.clear();
            self.navigation_viewport_request = Some(super::NavigationViewportRequest::Reveal {
                node_index: self.selected,
            });
        }
    }
    pub(super) fn handle_outline_search_key(&mut self, key: KeyEvent) {
        let has_matches = !self.outline_search.matches.is_empty();
        match self.outline_search.input.handle_key(key, has_matches) {
            SearchCommand::None => {}
            SearchCommand::Close => self.close_outline_search(),
            SearchCommand::Confirm => {
                self.last_search_target = SearchTarget::Outline;
                self.outline_search.revealed = self
                    .navigation_ancestors(self.selected)
                    .into_iter()
                    .collect();
                let (matches, limited) = crate::navigation::search::find(
                    &self.outline_search.records,
                    &self.outline_search.input.query,
                );
                self.outline_search.matches = matches;
                self.outline_search.limited = self.outline_search.index_limited || limited;
                self.outline_search.active_match = 0;
                self.select_active_outline_match();
            }
            SearchCommand::Next => {
                self.last_search_target = SearchTarget::Outline;
                self.select_outline_relative(1);
            }
            SearchCommand::Previous => {
                self.last_search_target = SearchTarget::Outline;
                self.select_outline_relative(-1);
            }
        }
    }
    pub(super) fn select_last_search_relative(&mut self, delta: isize) {
        match self.last_search_target {
            SearchTarget::Page => self.select_search_relative(delta),
            SearchTarget::Outline => self.select_outline_relative(delta),
        }
    }
    fn select_outline_relative(&mut self, delta: isize) {
        let count = self.outline_search.matches.len();
        if count == 0 {
            return;
        }
        let current = isize::try_from(self.outline_search.active_match).unwrap_or_default();
        let count = isize::try_from(count).unwrap_or(isize::MAX);
        self.outline_search.active_match =
            usize::try_from((current + delta).rem_euclid(count)).unwrap_or_default();
        self.select_active_outline_match();
    }
    fn select_active_outline_match(&mut self) {
        let Some(hit) = self
            .outline_search
            .matches
            .get(self.outline_search.active_match)
        else {
            return;
        };
        let index = hit.node_index;
        let anchor = hit.fields.first().map(|field| field.field.anchor.clone());
        let ancestors = self.navigation_ancestors(index);
        if self.outline_search.input.is_open() {
            self.outline_search.revealed.extend(ancestors);
        } else {
            self.expanded.extend(ancestors);
        }
        self.set_selected_index(index);
        self.scroll_to_selected();
        if let Some(anchor) = anchor {
            let width = self.geometry.content.width.max(1);
            if let Some(row) = self
                .session
                .rendered_cache
                .get(&width)
                .and_then(|rendered| rendered.anchor_row(&anchor))
            {
                self.session.content_scroll = row;
            }
        }
    }
    pub(super) fn effective_expanded(&self) -> Cow<'_, HashSet<String>> {
        if self.outline_search.input.is_open() && !self.outline_search.revealed.is_empty() {
            let mut expanded = self.expanded.clone();
            expanded.extend(self.outline_search.revealed.iter().cloned());
            Cow::Owned(expanded)
        } else {
            Cow::Borrowed(&self.expanded)
        }
    }
    pub(super) fn navigation_is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
            || (self.outline_search.input.is_open() && self.outline_search.revealed.contains(id))
    }
}
