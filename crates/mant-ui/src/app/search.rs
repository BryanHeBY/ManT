//! Owns search-field editing and translates keys into document-level commands.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

use crate::RenderedSearchMatch;

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

#[derive(Debug)]
pub(super) struct SearchState {
    pub(super) mode: SearchMode,
    pub(super) draft: String,
    pub(super) cursor: usize,
    pub(super) query: String,
    pub(super) matches: Vec<RenderedSearchMatch>,
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
                if !self.is_editing() && self.draft == self.query && !self.matches.is_empty() =>
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
