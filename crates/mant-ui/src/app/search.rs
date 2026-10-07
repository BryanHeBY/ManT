//! Owns search-field editing and translates keys into document-level commands.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use super::App;
use crate::RenderedSearchMatch;

#[derive(Debug, Clone)]
pub(super) struct ScopedRenderedSearchMatch {
    pub(super) document_index: usize,
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
}

/// Common input editor; each search surface owns its own query and cursor.
#[derive(Debug, Clone)]
pub(super) struct SearchInput {
    resume_mode: Option<SearchMode>,
    pub(super) mode: SearchMode,
    pub(super) draft: String,
    pub(super) cursor: usize,
    pub(super) query: String,
}

impl Default for SearchInput {
    fn default() -> Self {
        Self {
            resume_mode: None,
            mode: SearchMode::Closed,
            draft: String::new(),
            cursor: 0,
            query: String::new(),
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

    pub(super) fn open(&mut self) {
        if let Some(mode) = self.resume_mode.take() {
            self.mode = mode;
            return;
        }
        self.mode = SearchMode::Open { editing: false };
        self.draft.clone_from(&self.query);
        self.cursor = self.draft.len();
    }

    pub(super) fn close(&mut self) {
        self.resume_mode = None;
        self.mode = SearchMode::Closed;
        self.draft.clear();
        self.cursor = 0;
    }

    pub(super) fn suspend(&mut self) {
        if self.is_open() {
            self.resume_mode = Some(self.mode);
            self.mode = SearchMode::Closed;
        }
    }

    pub(super) fn move_cursor_to_column(&mut self, column: usize) {
        self.cursor = cursor_byte_at_column(&self.draft, column);
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent, has_matches: bool) -> SearchCommand {
        match key.code {
            KeyCode::Esc => return SearchCommand::Close,
            KeyCode::Enter => {
                if !self.is_editing() && self.draft == self.query {
                    return SearchCommand::Next;
                }
                self.query.clone_from(&self.draft);
                self.mode = SearchMode::Open { editing: false };
                return SearchCommand::Confirm;
            }
            KeyCode::Char('n' | 'N')
                if !self.is_editing() && self.draft == self.query && has_matches =>
            {
                return if key.code == KeyCode::Char('N')
                    || key.modifiers.contains(KeyModifiers::SHIFT)
                {
                    SearchCommand::Previous
                } else {
                    SearchCommand::Next
                };
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
            KeyCode::Down if !self.is_editing() => return SearchCommand::Next,
            KeyCode::Up if !self.is_editing() => return SearchCommand::Previous,
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.draft.insert(self.cursor, character);
                self.cursor += character.len_utf8();
                self.mode = SearchMode::Open { editing: true };
            }
            _ => {}
        }
        SearchCommand::None
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
        self.render_width = 0;
    }
    pub(super) fn close(&mut self) {
        self.input.close();
        self.matches.clear();
        self.render_width = 0;
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
            SearchCommand::Close => self.close_search(),
            SearchCommand::Confirm => {
                self.last_search_target = super::outline_search::SearchTarget::Page;
                self.refresh_search(self.geometry.content.width.max(1));
                self.select_active_search_match();
            }
            SearchCommand::Next => {
                self.last_search_target = super::outline_search::SearchTarget::Page;
                self.select_search_relative(1);
            }
            SearchCommand::Previous => {
                self.last_search_target = super::outline_search::SearchTarget::Page;
                self.select_search_relative(-1);
            }
        }
    }

    pub(super) fn refresh_search(&mut self, width: u16) {
        let query = self.search.query.clone();
        self.search.scope_matches =
            self.scope_documents
                .iter()
                .enumerate()
                .flat_map(|(document_index, bundle)| {
                    let rendered = crate::DocumentView::new(bundle).render(width);
                    rendered.search(&query).into_iter().map(move |rendered| {
                        ScopedRenderedSearchMatch {
                            document_index,
                            rendered,
                        }
                    })
                })
                .collect();
        self.search.active_match = self
            .search
            .active_match
            .min(self.search.scope_matches.len().saturating_sub(1));
        self.sync_current_search_matches();
        self.search.render_width = width;
    }

    pub(super) fn select_search_relative(&mut self, delta: isize) {
        if self.search.scope_matches.is_empty() {
            return;
        }
        let length = isize::try_from(self.search.scope_matches.len()).unwrap_or(isize::MAX);
        let current = isize::try_from(self.search.active_match).unwrap_or_default();
        self.search.active_match =
            usize::try_from((current + delta).rem_euclid(length)).unwrap_or_default();
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
        if !self.search.is_open() {
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
        let prefix_width = u16::try_from(self.search_prompt().len()).unwrap_or(u16::MAX);
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
