//! Owns search-field editing and translates keys into document-level commands.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use std::{
    ops::Range,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::{Duration, Instant},
};

use super::App;
use crate::RenderedSearchMatch;
use mant_protocol::{SearchCase, SearchSyntax};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SearchOptions {
    pub(super) syntax: SearchSyntax,
    pub(super) case: SearchCase,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            syntax: SearchSyntax::Regex,
            case: SearchCase::Sensitive,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ScopedRenderedSearchMatch {
    pub(super) document_index: usize,
    record: (usize, usize),
    range: Range<usize>,
    pub(super) rendered: RenderedSearchMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchMode {
    Closed,
    Open { editing: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchCommand {
    None,
    Changed,
    Close,
    Confirm,
    Next,
    Previous,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SearchState {
    input: SearchInput,
    pub(super) matches: Vec<RenderedSearchMatch>,
    pub(super) scope_matches: Vec<ScopedRenderedSearchMatch>,
    pub(super) active_match: usize,
    pub(super) render_width: u16,
    pub(super) matching_query: String,
    matching_options: SearchOptions,
    pub(super) job: Option<PageSearchJob>,
    pub(super) deadline: Option<Instant>,
    pub(super) limited: bool,
    repaint: SearchRepaint,
    restart_needed: bool,
    pending_move: Option<isize>,
    prepared: Option<(usize, u16, Arc<crate::RenderedDocument>)>,
    reflow_target: Option<SearchAnchor>,
    confirmed_target: Option<(String, SearchOptions, SearchAnchor)>,
    initial_position: Option<(usize, usize)>,
    initial_direction: isize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchAnchor {
    document: usize,
    record: (usize, usize),
    range: Range<usize>,
}

pub(super) const SEARCH_DEBOUNCE: Duration = Duration::from_millis(100);
pub(super) const SEARCH_SLICE_DELAY: Duration = Duration::from_millis(8);
pub(super) const SEARCH_BYTES_PER_SLICE: usize = 8192;
const SEARCH_REPAINT_INTERVAL: Duration = Duration::from_millis(50);
const MAX_PAGE_MATCHES: usize = 4096;
const MAX_PAGE_FRAGMENTS: usize = 32768;

/// Match inventories advance every slice; passive previews repaint at most
/// every 50 ms. Input and explicit navigation still redraw immediately.
#[derive(Debug, Clone, Default)]
pub(super) struct SearchRepaint {
    deadline: Option<Instant>,
    dirty: bool,
}

impl SearchRepaint {
    pub(super) fn update(
        &mut self,
        now: Instant,
        changed: bool,
        complete: bool,
        navigated: bool,
    ) -> bool {
        self.dirty |= changed;
        if complete
            || navigated
            || (self.dirty && self.deadline.is_none_or(|deadline| deadline <= now))
        {
            self.dirty = false;
            self.deadline = Some(now + SEARCH_REPAINT_INTERVAL);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PageSearchJob {
    document: usize,
    record: usize,
    fragments: usize,
    stream: crate::document::PatternStream,
}

/// Common input editor; each search surface owns its own query and cursor.
#[derive(Debug, Clone)]
pub(super) struct SearchInput {
    resume_mode: Option<SearchMode>,
    pub(super) mode: SearchMode,
    pub(super) draft: String,
    pub(super) cursor: usize,
    pub(super) query: String,
    pub(super) input_limited: bool,
    pub(super) options: SearchOptions,
    confirmed_options: SearchOptions,
    pub(super) direction: isize,
    confirmed_direction: isize,
    pub(super) error: Option<String>,
    pub(super) wrap: bool,
    pub(super) highlighted: bool,
}

impl Default for SearchInput {
    fn default() -> Self {
        Self {
            resume_mode: None,
            mode: SearchMode::Closed,
            draft: String::new(),
            cursor: 0,
            query: String::new(),
            input_limited: false,
            options: SearchOptions::default(),
            confirmed_options: SearchOptions::default(),
            direction: 1,
            confirmed_direction: 1,
            error: None,
            wrap: false,
            highlighted: false,
        }
    }
}

impl SearchInput {
    pub(super) const fn is_open(&self) -> bool {
        self.mode.is_open()
    }

    pub(super) const fn is_editing(&self) -> bool {
        self.mode.is_editing()
    }

    pub(super) fn effective_query(&self) -> &str {
        if self.is_editing() {
            &self.draft
        } else {
            &self.query
        }
    }
    pub(super) fn effective_options(&self) -> SearchOptions {
        if self.is_editing() {
            self.options
        } else {
            self.confirmed_options
        }
    }
    pub(super) fn repeat_direction(&self) -> isize {
        self.confirmed_direction
    }
    pub(super) fn has_changed_draft(&self) -> bool {
        self.is_editing() && (self.draft != self.query || self.options != self.confirmed_options)
    }

    pub(super) fn open(&mut self) {
        self.highlighted = true;
        if let Some(mode) = self.resume_mode.take() {
            self.mode = mode;
            return;
        }
        self.mode = SearchMode::Open { editing: true };
        self.draft.clear();
        self.cursor = 0;
    }

    pub(super) fn close(&mut self) {
        self.resume_mode = None;
        self.mode = SearchMode::Closed;
        self.draft.clear();
        self.cursor = 0;
        self.input_limited = false;
        self.options = self.confirmed_options;
        self.direction = self.confirmed_direction;
        self.error = None;
        self.highlighted = false;
    }

    pub(super) fn confirm_navigation(&mut self) {
        if self.is_editing() {
            self.query.clone_from(&self.draft);
            self.confirmed_options = self.options;
            self.confirmed_direction = self.direction;
            self.mode = SearchMode::Open { editing: false };
        }
    }

    pub(super) fn suspend(&mut self) {
        self.highlighted = false;
        if self.is_open() {
            self.resume_mode = Some(self.mode);
            self.mode = SearchMode::Closed;
            self.highlighted = false;
        }
    }

    pub(super) fn move_cursor_to_column(&mut self, column: usize) {
        self.cursor = cursor_byte_at_column(&self.draft, column);
    }

    fn reuse_empty_pattern(&mut self) -> bool {
        if !self.draft.is_empty() {
            return true;
        }
        self.draft.clone_from(&self.query);
        self.cursor = self.draft.len();
        if !self.draft.is_empty()
            && let Err(error) = crate::document::PatternStream::compile(
                &self.draft,
                self.options.syntax,
                self.options.case,
            )
        {
            self.error = Some(error);
            self.mode = SearchMode::Open { editing: true };
            return false;
        }
        true
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent, _has_matches: bool) -> SearchCommand {
        let previous = self.draft.clone();
        match key.code {
            KeyCode::Esc => return SearchCommand::Close,
            KeyCode::Enter => {
                if self.error.is_some() {
                    return SearchCommand::None;
                }
                if !self.reuse_empty_pattern() {
                    return SearchCommand::None;
                }
                self.query.clone_from(&self.draft);
                self.confirmed_options = self.options;
                self.confirmed_direction = self.direction;
                self.mode = SearchMode::Open { editing: false };
                return SearchCommand::Confirm;
            }
            KeyCode::Char('r' | 'R') if key.modifiers == KeyModifiers::CONTROL => {
                self.options.syntax = if self.options.syntax == SearchSyntax::Regex {
                    SearchSyntax::Literal
                } else {
                    SearchSyntax::Regex
                };
                self.mode = SearchMode::Open { editing: true };
                return SearchCommand::Changed;
            }
            KeyCode::Char('c' | 'C') if key.modifiers == KeyModifiers::ALT => {
                self.options.case = match self.options.case {
                    SearchCase::Sensitive => SearchCase::Insensitive,
                    SearchCase::Insensitive => SearchCase::Smart,
                    SearchCase::Smart => SearchCase::Sensitive,
                };
                self.mode = SearchMode::Open { editing: true };
                return SearchCommand::Changed;
            }
            KeyCode::Char('w' | 'W') if key.modifiers == KeyModifiers::ALT => {
                self.wrap = !self.wrap;
                return SearchCommand::None;
            }
            KeyCode::Backspace => {
                if let Some(previous) = previous_char_boundary(&self.draft, self.cursor) {
                    self.draft.drain(previous..self.cursor);
                    self.cursor = previous;
                    self.mode = SearchMode::Open { editing: true };
                }
            }
            KeyCode::Delete => {
                if let Some(next) = next_char_boundary(&self.draft, self.cursor) {
                    self.draft.drain(self.cursor..next);
                    self.mode = SearchMode::Open { editing: true };
                }
            }
            KeyCode::Left => {
                self.cursor = previous_char_boundary(&self.draft, self.cursor).unwrap_or_default();
            }
            KeyCode::Right => {
                self.cursor =
                    next_char_boundary(&self.draft, self.cursor).unwrap_or(self.draft.len());
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.draft.len(),
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => self.cursor = 0,
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor = self.draft.len();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.draft.clear();
                self.cursor = 0;
                self.mode = SearchMode::Open { editing: true };
            }
            KeyCode::Down if self.error.is_none() => return SearchCommand::Next,
            KeyCode::Up if self.error.is_none() => return SearchCommand::Previous,
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.input_limited =
                    self.draft.len() + character.len_utf8() > crate::document::MAX_QUERY_BYTES;
                if self.input_limited {
                    return SearchCommand::None;
                }
                self.draft.insert(self.cursor, character);
                self.cursor += character.len_utf8();
                self.mode = SearchMode::Open { editing: true };
            }
            _ => {}
        }
        if self.draft == previous {
            SearchCommand::None
        } else {
            self.input_limited = false;
            SearchCommand::Changed
        }
    }
}

// Preserve the page editor's field access while keeping result ownership out
// of the shared input model. Outline search uses SearchInput directly.
impl Deref for SearchState {
    type Target = SearchInput;
    fn deref(&self) -> &Self::Target {
        &self.input
    }
}
impl DerefMut for SearchState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.input
    }
}

impl SearchState {
    pub(super) fn suspend(&mut self) {
        self.input.suspend();
        self.matches.clear();
        self.restart_needed = true;
        self.job = None;
        self.deadline = None;
        self.pending_move = None;
    }
    pub(super) fn close(&mut self) {
        self.input.close();
        self.matches.clear();
        self.pending_move = None;
        if self.job.take().is_some()
            || self.matching_query != self.query
            || self.matching_options != self.options
        {
            self.restart_needed = true;
            if self.matching_query != self.query || self.matching_options != self.options {
                self.scope_matches.clear();
                self.matches.clear();
            }
        }
        self.deadline = None;
    }
}

impl SearchMode {
    const fn is_open(self) -> bool {
        matches!(self, Self::Open { .. })
    }

    const fn is_editing(self) -> bool {
        matches!(self, Self::Open { editing: true })
    }
}

impl App {
    pub(super) fn open_search(&mut self) {
        if self.search_target == super::outline_search::SearchTarget::Page && self.search.is_open()
        {
            return;
        }
        self.finish_outline_search(true);
        self.search_target = super::outline_search::SearchTarget::Page;
        self.search.open();
        if self.search.restart_needed
            || self.search.render_width != self.geometry.content.width.max(1)
            || self.search.matching_query != self.search.effective_query()
            || self.search.matching_options != self.search.effective_options()
        {
            self.refresh_search(self.geometry.content.width.max(1));
        }
        self.sync_current_search_matches();
    }

    pub(super) fn close_search(&mut self) {
        match self.search_target {
            super::outline_search::SearchTarget::Page => self.search.close(),
            super::outline_search::SearchTarget::Outline => self.close_outline_search(),
        }
    }

    pub(super) fn handle_search_key(&mut self, key: KeyEvent) {
        if self.search_target == super::outline_search::SearchTarget::Outline {
            self.handle_outline_search_key(key);
            return;
        }
        let has_matches = !self.search.scope_matches.is_empty();
        match self.search.handle_key(key, has_matches) {
            SearchCommand::None => {}
            SearchCommand::Changed => self.schedule_page_search(Instant::now()),
            SearchCommand::Close => self.close_search(),
            SearchCommand::Confirm => {
                self.notice = None;
                self.search.initial_direction = self.search.direction;
                self.last_search_target = super::outline_search::SearchTarget::Page;
                self.ensure_page_search();
                self.search.pending_move = Some(0);
                self.search.initial_position = Some((
                    self.scope_documents
                        .iter()
                        .position(|document| Arc::ptr_eq(document, &self.session.current_bundle))
                        .unwrap_or(0),
                    self.session.content_scroll,
                ));
                self.search.mode = SearchMode::Closed;
                self.advance_page_search(Instant::now());
                self.fulfill_page_search_move();
            }
            SearchCommand::Next => {
                let initial = self.search.is_editing()
                    && (self.search.draft != self.search.query
                        || self.search.options != self.search.confirmed_options
                        || self.search.confirmed_target.is_none());
                self.search.confirm_navigation();
                self.last_search_target = super::outline_search::SearchTarget::Page;
                if initial {
                    self.ensure_page_search();
                    self.search.initial_direction = self.search.direction;
                    self.search.initial_position = Some((
                        self.scope_documents
                            .iter()
                            .position(|document| {
                                Arc::ptr_eq(document, &self.session.current_bundle)
                            })
                            .unwrap_or(0),
                        self.session.content_scroll,
                    ));
                    self.search.pending_move = Some(0);
                    self.advance_page_search(Instant::now());
                    self.fulfill_page_search_move();
                } else {
                    self.select_search_relative(self.search.direction);
                }
            }
            SearchCommand::Previous => {
                let initial = self.search.is_editing()
                    && (self.search.draft != self.search.query
                        || self.search.options != self.search.confirmed_options
                        || self.search.confirmed_target.is_none());
                self.search.confirm_navigation();
                self.last_search_target = super::outline_search::SearchTarget::Page;
                if initial {
                    self.ensure_page_search();
                    self.search.initial_direction = -self.search.direction;
                    self.search.initial_position = Some((
                        self.scope_documents
                            .iter()
                            .position(|document| {
                                Arc::ptr_eq(document, &self.session.current_bundle)
                            })
                            .unwrap_or(0),
                        self.session.content_scroll,
                    ));
                    self.search.pending_move = Some(0);
                    self.advance_page_search(Instant::now());
                    self.fulfill_page_search_move();
                } else {
                    self.select_search_relative(-self.search.direction);
                }
            }
        }
    }

    pub(super) fn refresh_search(&mut self, width: u16) {
        self.search.restart_needed = false;
        self.search.repaint = SearchRepaint::default();
        let query = self.search.effective_query().to_owned();
        self.search.reflow_target = if self.search.matching_query == query
            && self.search.matching_options == self.search.effective_options()
        {
            self.search
                .scope_matches
                .get(self.search.active_match)
                .map(|hit| SearchAnchor {
                    document: hit.document_index,
                    record: hit.record,
                    range: hit.range.clone(),
                })
                .or_else(|| self.search.reflow_target.take())
        } else {
            self.search
                .confirmed_target
                .as_ref()
                .filter(|(confirmed, options, _)| {
                    *confirmed == query && *options == self.search.effective_options()
                })
                .map(|(_, _, anchor)| anchor.clone())
        };
        self.search.matching_query = query;
        self.search.matching_options = self.search.effective_options();
        self.search.scope_matches.clear();
        self.search.matches.clear();
        self.search.active_match = 0;
        self.search.limited = false;
        self.search.error = None;
        self.search.job = if self.search.matching_query.is_empty() {
            None
        } else {
            match crate::document::PatternStream::compile(
                &self.search.matching_query,
                self.search.matching_options.syntax,
                self.search.matching_options.case,
            ) {
                Ok(stream) => Some(PageSearchJob {
                    document: 0,
                    record: 0,
                    fragments: 0,
                    stream,
                }),
                Err(error) => {
                    self.search.error = Some(error);
                    None
                }
            }
        };
        self.search.deadline = self.search.job.as_ref().map(|_| Instant::now());
        self.search.render_width = width;
    }

    fn schedule_page_search(&mut self, now: Instant) {
        self.refresh_search(self.geometry.content.width.max(1));
        self.search.deadline = self.search.job.as_ref().map(|_| now + SEARCH_DEBOUNCE);
        self.search.pending_move = None;
    }

    fn ensure_page_search(&mut self) {
        if self.search.restart_needed
            || self.search.render_width != self.geometry.content.width.max(1)
            || self.search.matching_query != self.search.effective_query()
            || self.search.matching_options != self.search.effective_options()
        {
            self.refresh_search(self.geometry.content.width.max(1));
        }
        if self.search.job.is_some() {
            self.search.deadline = Some(Instant::now());
        }
    }

    /// Scan immutable, already laid-out search records without reflowing per query.
    /// The current width is shared with the viewport. At most one other scope
    /// rendering is retained; non-current snapshots are prepared lazily once per
    /// visit, not on each keystroke. Width changes retire all stale cell results.
    pub(super) fn advance_page_search(&mut self, now: Instant) -> bool {
        if self.search.deadline.is_none_or(|deadline| deadline > now) {
            return false;
        }
        let Some(mut job) = self.search.job.take() else {
            self.search.deadline = None;
            return false;
        };
        let width = self.search.render_width.max(1);
        let previous_count = self.search.scope_matches.len();
        let wanted_navigation = self.search.pending_move.is_some();
        let mut budget = SEARCH_BYTES_PER_SLICE;
        let mut prepared_scope = false;
        while budget > 0 && job.document < self.scope_documents.len() {
            let rendered = if self.is_current_search_document(job.document) {
                Arc::clone(
                    self.session
                        .rendered_cache
                        .entry(width)
                        .or_insert_with(|| Arc::new(self.session.document.render(width))),
                )
            } else if let Some((index, prepared_width, rendered)) = &self.search.prepared
                && *index == job.document
                && *prepared_width == width
            {
                Arc::clone(rendered)
            } else {
                if prepared_scope {
                    break;
                }
                prepared_scope = true;
                let rendered = Arc::new(
                    crate::DocumentView::new(&self.scope_documents[job.document]).render(width),
                );
                self.search.prepared = Some((job.document, width, Arc::clone(&rendered)));
                rendered
            };
            if job.record == rendered.search_record_count() {
                job.document += 1;
                job.record = 0;
                job.stream.reset();
                budget = budget.saturating_sub(1);
                continue;
            }
            let text = rendered.search_record_text(job.record);
            let ranges = job.stream.advance(text, &mut budget);
            for range in ranges {
                if let Some(hit) = rendered.project_search_range(job.record, range.clone()) {
                    let fragments = hit.fragment_count();
                    if self.search.scope_matches.len() == MAX_PAGE_MATCHES
                        || job.fragments + fragments > MAX_PAGE_FRAGMENTS
                    {
                        self.search.limited = true;
                        self.search.deadline = None;
                        self.sync_current_search_matches();
                        self.fulfill_page_search_move();
                        return true;
                    }
                    job.fragments += fragments;
                    if self.search.reflow_target.as_ref().is_some_and(|anchor| {
                        anchor.document == job.document
                            && anchor.record == rendered.search_record_key(job.record)
                            && anchor.range == range
                    }) {
                        self.search.active_match = self.search.scope_matches.len();
                        self.search.reflow_target = None;
                    }
                    self.search.scope_matches.push(ScopedRenderedSearchMatch {
                        document_index: job.document,
                        record: rendered.search_record_key(job.record),
                        range,
                        rendered: hit,
                    });
                }
            }
            if job.stream.is_done() {
                job.record += 1;
                job.stream.reset();
                budget = budget.saturating_sub(1);
            }
        }
        if job.document < self.scope_documents.len() {
            self.search.job = Some(job);
            self.search.deadline = Some(now + SEARCH_SLICE_DELAY);
        } else {
            self.search.deadline = None;
        }
        if previous_count != self.search.scope_matches.len() {
            self.sync_current_search_matches();
        }
        self.fulfill_page_search_move();
        self.search.repaint.update(
            now,
            previous_count != self.search.scope_matches.len(),
            self.search.job.is_none(),
            wanted_navigation && self.search.pending_move.is_none(),
        )
    }

    fn fulfill_page_search_move(&mut self) {
        if self.overlay != super::Overlay::None {
            self.search.pending_move = None;
            return;
        }
        if self.search.reflow_target.is_some() && self.search.job.is_some() {
            return;
        }
        let Some(delta) = self.search.pending_move else {
            return;
        };
        let count = self.search.scope_matches.len();
        if delta == 0 && count > 0 {
            if let Some((document, row)) = self.search.initial_position {
                let direction = self.search.initial_direction;
                let index =
                    if direction > 0 {
                        self.search.scope_matches.iter().position(|hit| {
                            (hit.document_index, hit.rendered.row) >= (document, row)
                        })
                    } else {
                        self.search.scope_matches.iter().rposition(|hit| {
                            (hit.document_index, hit.rendered.row) <= (document, row)
                        })
                    };
                // A backward candidate is final only after the earlier scope
                // has been scanned; never publish an arbitrary partial hit.
                if direction < 0 && self.search.job.is_some() {
                    return;
                }
                if let Some(index) = index {
                    self.search.active_match = index;
                } else if self.search.job.is_some() {
                    return;
                } else if self.search.wrap && !self.search.limited {
                    self.search.active_match = if direction > 0 { 0 } else { count - 1 };
                    self.notice = Some("Search wrapped".to_owned());
                } else {
                    self.search.pending_move = None;
                    self.search.initial_position = None;
                    self.notice = Some("Pattern not found in search direction".to_owned());
                    return;
                }
            }
            self.search.pending_move = None;
            self.search.initial_position = None;
            self.select_active_search_match();
        } else if count > 0
            && (self.search.job.is_none()
                || (delta > 0 && self.search.active_match + 1 < count)
                || (delta < 0 && self.search.active_match > 0))
        {
            self.search.pending_move = None;
            self.move_page_result(delta);
        } else if self.search.job.is_none() {
            self.search.pending_move = None;
            if count == 0 && !self.search.query.is_empty() {
                self.notice = Some(
                    if self.search.limited {
                        "No indexed matches · partial"
                    } else {
                        "Pattern not found"
                    }
                    .to_owned(),
                );
            }
        }
    }

    pub(super) fn select_search_relative(&mut self, delta: isize) {
        self.notice = None;
        self.ensure_page_search();
        self.sync_current_search_matches();
        if self.search.job.is_some() {
            self.advance_page_search(Instant::now());
        }
        if self.search.reflow_target.is_none()
            && self
                .search
                .scope_matches
                .get(self.search.active_match)
                .is_some_and(|hit| {
                    !self.is_current_search_document(hit.document_index)
                        || hit.rendered.row != self.session.content_scroll
                })
        {
            self.search.initial_position = Some((
                self.scope_documents
                    .iter()
                    .position(|document| Arc::ptr_eq(document, &self.session.current_bundle))
                    .unwrap_or(0),
                self.session.content_scroll,
            ));
            self.search.initial_direction = delta;
            self.search.pending_move = Some(0);
            self.fulfill_page_search_move();
            return;
        }
        if self.search.job.is_some()
            && (self.search.reflow_target.is_some()
                || self.search.scope_matches.is_empty()
                || (delta > 0 && self.search.active_match + 1 >= self.search.scope_matches.len())
                || (delta < 0 && self.search.active_match == 0))
        {
            self.search.pending_move = Some(delta);
            return;
        }
        self.move_page_result(delta);
    }

    fn move_page_result(&mut self, delta: isize) {
        if self.search.scope_matches.is_empty() {
            return;
        }
        let length = isize::try_from(self.search.scope_matches.len()).unwrap_or(isize::MAX);
        let current = isize::try_from(self.search.active_match).unwrap_or_default();
        let next = current + delta;
        if !(0..length).contains(&next) {
            if !self.search.wrap || self.search.limited {
                self.notice = Some(
                    if self.search.limited {
                        "Search results are partial"
                    } else {
                        "No more matches in search direction"
                    }
                    .to_owned(),
                );
                return;
            }
            self.notice = Some("Search wrapped".to_owned());
        }
        self.search.active_match = usize::try_from(next.rem_euclid(length)).unwrap_or_default();
        self.select_active_search_match();
    }

    fn select_active_search_match(&mut self) {
        let Some(search_match) = self
            .search
            .scope_matches
            .get(self.search.active_match)
            .cloned()
        else {
            return;
        };
        if !self.search.is_editing() && self.search.matching_query == self.search.query {
            self.search.confirmed_target = Some((
                self.search.query.clone(),
                self.search.effective_options(),
                SearchAnchor {
                    document: search_match.document_index,
                    record: search_match.record,
                    range: search_match.range.clone(),
                },
            ));
        }
        if !self.is_current_search_document(search_match.document_index) {
            let current = self.current_location();
            let Some(bundle) = self
                .scope_documents
                .get(search_match.document_index)
                .cloned()
            else {
                return;
            };
            self.navigation
                .commit(super::HistoryDirection::New, current);
            self.replace_document(bundle, super::DocumentChangeReason::SearchResult);
        }
        self.sync_current_search_matches();
        self.session.content_scroll = search_match.rendered.row;
        self.select_section_at_row(search_match.rendered.row);
    }

    pub(super) fn active_rendered_search_match(&self) -> Option<usize> {
        let active = self.search.scope_matches.get(self.search.active_match)?;
        if !self.is_current_search_document(active.document_index) {
            return None;
        }
        Some(
            self.search
                .scope_matches
                .iter()
                .take(self.search.active_match)
                .filter(|candidate| self.is_current_search_document(candidate.document_index))
                .count(),
        )
    }

    fn sync_current_search_matches(&mut self) {
        if !self.search.highlighted {
            self.search.matches.clear();
            return;
        }
        self.search.matches = self
            .search
            .scope_matches
            .iter()
            .filter(|candidate| self.is_current_search_document(candidate.document_index))
            .map(|candidate| candidate.rendered.clone())
            .collect();
    }

    fn is_current_search_document(&self, index: usize) -> bool {
        self.scope_documents
            .get(index)
            .is_some_and(|bundle| Arc::ptr_eq(bundle, &self.session.current_bundle))
    }

    pub(super) fn move_search_cursor_to(&mut self, column: u16) {
        let prefix_width = u16::try_from(self.search_prompt().width()).unwrap_or(u16::MAX);
        let text_column =
            usize::from(column.saturating_sub(self.geometry.status.x.saturating_add(prefix_width)));
        self.active_search_input_mut()
            .move_cursor_to_column(text_column);
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
