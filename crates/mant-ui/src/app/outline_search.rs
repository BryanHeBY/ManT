//! Current-snapshot outline finding and temporary ancestor visibility.

use crossterm::event::KeyEvent;
use std::{borrow::Cow, collections::HashSet, time::Instant};

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

#[derive(Debug, Default, PartialEq, Eq)]
enum ScanStatus {
    #[default]
    Idle,
    Running,
    Complete,
    Cancelled,
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
    pub(super) deadline: Option<Instant>,
    pub(super) job: Option<crate::navigation::search::OutlineScan>,
    pending_move: Option<isize>,
    matching_query: String,
    status: ScanStatus,
    confirmed_target: Option<(String, usize)>,
    restart_target: Option<usize>,
    repaint: super::search::SearchRepaint,
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
        let value = if self.outline_search.input.is_editing() {
            self.outline_search.input.draft.clone()
        } else {
            self.outline_search.input.query.clone()
        };
        if self.outline_search.matching_query != value
            || self.outline_search.status != ScanStatus::Complete
        {
            self.start_outline_search(&value, Instant::now() + super::search::SEARCH_DEBOUNCE);
        }
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
            if self.outline_search.job.take().is_some() {
                self.outline_search.status = ScanStatus::Cancelled;
            }
            self.outline_search.deadline = None;
            self.outline_search.pending_move = None;
            if self.outline_search.matching_query != self.outline_search.input.query {
                self.outline_search.matches.clear();
            }
            self.navigation_viewport_request = Some(super::NavigationViewportRequest::Reveal {
                node_index: self.selected,
            });
        }
    }
    pub(super) fn handle_outline_search_key(&mut self, key: KeyEvent) {
        let has_matches = !self.outline_search.matches.is_empty();
        match self.outline_search.input.handle_key(key, has_matches) {
            SearchCommand::None => {}
            SearchCommand::Changed => {
                let query = self.outline_search.input.draft.clone();
                self.start_outline_search(&query, Instant::now() + super::search::SEARCH_DEBOUNCE);
            }
            SearchCommand::Close => self.close_outline_search(),
            SearchCommand::Confirm => {
                self.last_search_target = SearchTarget::Outline;
                if self.outline_search.job.is_some() {
                    self.outline_search.deadline = Some(Instant::now());
                }
                self.outline_search.pending_move = Some(0);
                self.advance_outline_search(Instant::now());
                self.fulfill_outline_search_move();
            }
            SearchCommand::Next => {
                self.outline_search.input.confirm_navigation();
                self.last_search_target = SearchTarget::Outline;
                self.select_outline_relative(1);
            }
            SearchCommand::Previous => {
                self.outline_search.input.confirm_navigation();
                self.last_search_target = SearchTarget::Outline;
                self.select_outline_relative(-1);
            }
        }
    }
    pub(super) fn select_last_search_relative(&mut self, delta: isize) {
        if self.search_is_open() && self.search_target == self.last_search_target {
            self.active_search_input_mut().confirm_navigation();
        }
        match self.last_search_target {
            SearchTarget::Page => self.select_search_relative(delta),
            SearchTarget::Outline => self.select_outline_relative(delta),
        }
    }
    fn select_outline_relative(&mut self, delta: isize) {
        if !self.outline_search.input.is_open()
            && self.outline_search.job.is_none()
            && (self.outline_search.status != ScanStatus::Complete
                || self.outline_search.matching_query != self.outline_search.input.query)
        {
            let query = self.outline_search.input.query.clone();
            self.start_outline_search(&query, Instant::now());
        }
        if self.outline_search.job.is_some() {
            self.outline_search.deadline = Some(Instant::now());
            self.advance_outline_search(Instant::now());
        }
        if self.outline_search.job.is_some() {
            self.outline_search.deadline = Some(Instant::now());
            if self.outline_search.restart_target.is_some()
                || self.outline_search.matches.is_empty()
                || (delta > 0
                    && self.outline_search.active_match + 1 >= self.outline_search.matches.len())
                || (delta < 0 && self.outline_search.active_match == 0)
            {
                self.outline_search.pending_move = Some(delta);
                return;
            }
        }
        self.move_outline_result(delta);
    }

    fn move_outline_result(&mut self, delta: isize) {
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
        if !self.outline_search.input.is_editing()
            && self.outline_search.matching_query == self.outline_search.input.query
        {
            self.outline_search.confirmed_target =
                Some((self.outline_search.input.query.clone(), index));
        }
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

    fn start_outline_search(&mut self, query: &str, deadline: Instant) {
        self.outline_search.repaint = super::search::SearchRepaint::default();
        self.outline_search.restart_target = self
            .outline_search
            .confirmed_target
            .as_ref()
            .filter(|(confirmed, _)| confirmed == query)
            .map(|(_, node)| *node);
        self.outline_search.matches.clear();
        self.outline_search.active_match = 0;
        self.outline_search.limited = self.outline_search.index_limited;
        self.outline_search.pending_move = None;
        // Previewing matches reveals no new ancestors and moves no selection.
        self.outline_search.revealed = self
            .navigation_ancestors(self.selected)
            .into_iter()
            .collect();
        query.clone_into(&mut self.outline_search.matching_query);
        self.outline_search.status = if query.is_empty() {
            ScanStatus::Complete
        } else {
            ScanStatus::Running
        };
        self.outline_search.job =
            (!query.is_empty()).then(|| crate::navigation::search::OutlineScan::new(query));
        self.outline_search.deadline = self.outline_search.job.as_ref().map(|_| deadline);
    }

    pub(super) fn advance_outline_search(&mut self, now: Instant) -> bool {
        if self
            .outline_search
            .deadline
            .is_none_or(|deadline| deadline > now)
        {
            return false;
        }
        let Some(mut job) = self.outline_search.job.take() else {
            return false;
        };
        let previous_count = self.outline_search.matches.len();
        let wanted_navigation = self.outline_search.pending_move.is_some();
        let (done, limited) = job.advance(
            &self.outline_search.records,
            &mut self.outline_search.matches,
            super::search::SEARCH_BYTES_PER_SLICE,
        );
        self.outline_search.limited |= limited;
        if let Some(node) = self.outline_search.restart_target
            && let Some(index) = self
                .outline_search
                .matches
                .iter()
                .position(|hit| hit.node_index == node)
        {
            self.outline_search.active_match = index;
            self.outline_search.restart_target = None;
        }
        if done {
            self.outline_search.status = ScanStatus::Complete;
            self.outline_search.deadline = None;
        } else {
            self.outline_search.job = Some(job);
            self.outline_search.deadline = Some(now + super::search::SEARCH_SLICE_DELAY);
        }
        self.fulfill_outline_search_move();
        self.outline_search.repaint.update(
            now,
            previous_count != self.outline_search.matches.len(),
            done,
            wanted_navigation && self.outline_search.pending_move.is_none(),
        )
    }

    fn fulfill_outline_search_move(&mut self) {
        if self.overlay != super::Overlay::None {
            self.outline_search.pending_move = None;
            return;
        }
        if self.outline_search.restart_target.is_some() && self.outline_search.job.is_some() {
            return;
        }
        let Some(delta) = self.outline_search.pending_move else {
            return;
        };
        let count = self.outline_search.matches.len();
        if delta == 0 && count > 0 {
            self.outline_search.pending_move = None;
            self.select_active_outline_match();
        } else if count > 0
            && (self.outline_search.job.is_none()
                || (delta > 0 && self.outline_search.active_match + 1 < count)
                || (delta < 0 && self.outline_search.active_match > 0))
        {
            self.outline_search.pending_move = None;
            self.move_outline_result(delta);
        } else if self.outline_search.job.is_none() {
            self.outline_search.pending_move = None;
        }
    }
    pub(super) fn navigation_is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
            || (self.outline_search.input.is_open() && self.outline_search.revealed.contains(id))
    }
}
