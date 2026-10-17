//! Shared search-field editing, options and less-style confirmation.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_protocol::{SearchCase, SearchSyntax};
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) struct SearchOptions {
    pub(in crate::app) syntax: SearchSyntax,
    pub(in crate::app) case: SearchCase,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            syntax: SearchSyntax::Regex,
            case: SearchCase::Sensitive,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum SearchMode {
    Closed,
    Open { editing: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum SearchCommand {
    None,
    Changed,
    Close,
    Confirm,
    Next,
    Previous,
}

/// Common input editor; each search surface owns its own query and cursor.
#[derive(Debug, Clone)]
pub(in crate::app) struct SearchInput {
    resume_mode: Option<SearchMode>,
    pub(in crate::app) mode: SearchMode,
    pub(in crate::app) draft: String,
    pub(in crate::app) cursor: usize,
    pub(in crate::app) query: String,
    pub(in crate::app) input_limited: bool,
    pub(in crate::app) options: SearchOptions,
    confirmed_options: SearchOptions,
    pub(in crate::app) direction: isize,
    confirmed_direction: isize,
    pub(in crate::app) error: Option<String>,
    pub(in crate::app) wrap: bool,
    pub(in crate::app) highlighted: bool,
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
    pub(in crate::app) const fn is_open(&self) -> bool {
        self.mode.is_open()
    }

    pub(in crate::app) const fn is_editing(&self) -> bool {
        self.mode.is_editing()
    }

    pub(in crate::app) fn effective_query(&self) -> &str {
        if self.is_editing() {
            &self.draft
        } else {
            &self.query
        }
    }
    pub(in crate::app) fn effective_options(&self) -> SearchOptions {
        if self.is_editing() {
            self.options
        } else {
            self.confirmed_options
        }
    }
    pub(in crate::app) fn repeat_direction(&self) -> isize {
        self.confirmed_direction
    }
    pub(in crate::app) fn has_changed_draft(&self) -> bool {
        self.is_editing() && (self.draft != self.query || self.options != self.confirmed_options)
    }

    pub(in crate::app) fn open(&mut self) {
        self.highlighted = true;
        if let Some(mode) = self.resume_mode.take() {
            self.mode = mode;
            return;
        }
        self.mode = SearchMode::Open { editing: true };
        self.draft.clear();
        self.cursor = 0;
    }

    pub(in crate::app) fn close(&mut self) {
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

    pub(in crate::app) fn confirm_navigation(&mut self) {
        if self.is_editing() {
            self.query.clone_from(&self.draft);
            self.confirmed_options = self.options;
            self.confirmed_direction = self.direction;
            self.mode = SearchMode::Open { editing: false };
        }
    }

    pub(in crate::app) fn suspend(&mut self) {
        self.highlighted = false;
        if self.is_open() {
            self.resume_mode = Some(self.mode);
            self.mode = SearchMode::Closed;
            self.highlighted = false;
        }
    }

    pub(in crate::app) fn move_cursor_to_column(&mut self, column: usize) {
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

    pub(in crate::app) fn handle_key(
        &mut self,
        key: KeyEvent,
        _has_matches: bool,
    ) -> SearchCommand {
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

impl SearchMode {
    pub(super) const fn is_open(self) -> bool {
        matches!(self, Self::Open { .. })
    }

    pub(super) const fn is_editing(self) -> bool {
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
