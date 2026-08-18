//! Owns search-field editing and translates keys into document-level commands.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

use mant_protocol::DocumentAddress;

use super::App;
use crate::RenderedSearchMatch;

#[derive(Debug, Clone)]
pub(super) struct ScopedRenderedSearchMatch {
    pub(super) document_index: usize,
    pub(super) address: Option<DocumentAddress>,
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
    Confirm,
    Next,
    Previous,
}

#[derive(Debug, Clone)]
pub(super) struct SearchState {
    pub(super) mode: SearchMode,
    pub(super) draft: String,
    pub(super) cursor: usize,
    pub(super) query: String,
    pub(super) matches: Vec<RenderedSearchMatch>,
    pub(super) scope_matches: Vec<ScopedRenderedSearchMatch>,
    pub(super) active_match: usize,
    pub(super) render_width: u16,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            mode: SearchMode::Closed,
            draft: String::new(),
            cursor: 0,
            query: String::new(),
            matches: Vec::new(),
            scope_matches: Vec::new(),
            active_match: 0,
            render_width: 0,
        }
    }
}

impl SearchState {
    pub(super) const fn is_open(&self) -> bool {
        self.mode.is_open()
    }

    pub(super) const fn is_editing(&self) -> bool {
        self.mode.is_editing()
    }

    pub(super) fn open(&mut self) {
        self.mode = SearchMode::Open { editing: false };
        self.draft.clone_from(&self.query);
        self.cursor = self.draft.len();
    }

    pub(super) fn close(&mut self) {
        *self = Self::default();
    }

    pub(super) fn move_cursor_to_column(&mut self, column: usize) {
        self.cursor = cursor_byte_at_column(&self.draft, column);
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> SearchCommand {
        match key.code {
            KeyCode::Esc => self.close(),
            KeyCode::Enter => {
                if !self.is_editing() && self.draft == self.query {
                    return SearchCommand::Next;
                }
                self.query.clone_from(&self.draft);
                self.mode = SearchMode::Open { editing: false };
                return SearchCommand::Confirm;
            }
            KeyCode::Char('n' | 'N')
                if !self.is_editing()
                    && self.draft == self.query
                    && !self.scope_matches.is_empty() =>
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
        self.search.open();
    }

    pub(super) fn close_search(&mut self) {
        self.search.close();
    }

    pub(super) fn handle_search_key(&mut self, key: KeyEvent) {
        match self.search.handle_key(key) {
            SearchCommand::None => {}
            SearchCommand::Confirm => {
                self.refresh_search(self.geometry.content.width.max(1));
                self.select_active_search_match();
            }
            SearchCommand::Next => self.select_search_relative(1),
            SearchCommand::Previous => self.select_search_relative(-1),
        }
    }

    pub(super) fn refresh_search(&mut self, width: u16) {
        let query = self.search.query.clone();
        self.search.scope_matches =
            self.scope_documents
                .iter()
                .enumerate()
                .flat_map(|(document_index, bundle)| {
                    let address = bundle.address.clone();
                    let rendered = crate::DocumentView::new(bundle).render(width);
                    rendered.search(&query).into_iter().map(move |rendered| {
                        ScopedRenderedSearchMatch {
                            document_index,
                            address: address.clone(),
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
        if self.current_address != search_match.address {
            let current = self.current_location();
            let Some(bundle) = self
                .scope_documents
                .get(search_match.document_index)
                .cloned()
            else {
                return;
            };
            let search = self.search.clone();
            super::push_history(&mut self.back_history, current);
            self.forward_history.clear();
            self.replace_document(&bundle);
            self.search = search;
            self.sync_current_search_matches();
        }
        self.content_scroll = search_match.rendered.row;
        self.select_section_at_row(search_match.rendered.row);
    }

    pub(super) fn active_rendered_search_match(&self) -> Option<usize> {
        let active = self.search.scope_matches.get(self.search.active_match)?;
        if active.address != self.current_address {
            return None;
        }
        Some(
            self.search
                .scope_matches
                .iter()
                .take(self.search.active_match)
                .filter(|candidate| candidate.address == self.current_address)
                .count(),
        )
    }

    fn sync_current_search_matches(&mut self) {
        self.search.matches = self
            .search
            .scope_matches
            .iter()
            .filter(|candidate| candidate.address == self.current_address)
            .map(|candidate| candidate.rendered.clone())
            .collect();
    }

    pub(super) fn move_search_cursor_to(&mut self, column: u16) {
        const SEARCH_PREFIX_WIDTH: u16 = 7;
        let text_column = usize::from(
            column.saturating_sub(self.geometry.status.x.saturating_add(SEARCH_PREFIX_WIDTH)),
        );
        self.search.move_cursor_to_column(text_column);
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
