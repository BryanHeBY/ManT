//! Live document-catalog filtering for the modal document finder.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ast::{DocumentAddress, DocumentSummary, MarkdownOrigin};

use super::{App, Overlay};

#[derive(Debug, Default)]
pub(super) struct FinderState {
    pub(super) draft: String,
    pub(super) cursor: usize,
    pub(super) matches: Vec<usize>,
    pub(super) selected: usize,
}

impl FinderState {
    pub(super) fn open(&mut self, catalog: &[DocumentSummary]) {
        self.refresh(catalog);
    }

    pub(super) fn handle_key(
        &mut self,
        key: KeyEvent,
        catalog: &[DocumentSummary],
    ) -> Option<DocumentAddress> {
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Enter => {
                return self
                    .matches
                    .get(self.selected)
                    .and_then(|index| catalog.get(*index))
                    .map(|document| document.address.clone());
            }
            KeyCode::Up => self.select_relative(-1),
            KeyCode::Down => self.select_relative(1),
            KeyCode::PageUp => self.select_relative(-10),
            KeyCode::PageDown => self.select_relative(10),
            KeyCode::Backspace => {
                if let Some(previous) = previous_char_boundary(&self.draft, self.cursor) {
                    self.draft.drain(previous..self.cursor);
                    self.cursor = previous;
                    self.refresh(catalog);
                }
            }
            KeyCode::Delete => {
                if let Some(next) = next_char_boundary(&self.draft, self.cursor) {
                    self.draft.drain(self.cursor..next);
                    self.refresh(catalog);
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
                self.refresh(catalog);
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.draft.insert(self.cursor, character);
                self.cursor += character.len_utf8();
                self.refresh(catalog);
            }
            _ => {}
        }
        None
    }

    fn refresh(&mut self, catalog: &[DocumentSummary]) {
        let needle = self.draft.to_lowercase();
        self.matches = catalog
            .iter()
            .enumerate()
            .filter(|(_, document)| {
                needle.is_empty() || document.address.name().to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.matches.sort_by_key(|index| {
            let name = catalog[*index].address.name().to_lowercase();
            let rank = if needle.is_empty() {
                3
            } else if name == needle {
                0
            } else if name.starts_with(&needle) {
                1
            } else {
                2
            };
            (rank, name)
        });
        self.selected = 0;
    }

    fn select_relative(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.matches.len() - 1);
    }
}

impl App {
    pub(super) fn open_document_finder(&mut self) {
        self.close_search();
        self.finder.open(&self.catalog);
        self.overlay = Overlay::DocumentFinder;
    }

    pub(super) fn handle_finder_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.overlay = Overlay::None;
            return;
        }
        if let Some(address) = self.finder.handle_key(key, &self.catalog) {
            self.pending_open = Some(address);
            self.overlay = Overlay::None;
        }
    }
}

pub(super) fn document_category(address: &DocumentAddress) -> String {
    match address {
        DocumentAddress::Markdown {
            origin: MarkdownOrigin::Documents,
            ..
        } => "documents".to_owned(),
        DocumentAddress::Markdown {
            origin: MarkdownOrigin::Source { name },
            ..
        } => name.clone(),
        DocumentAddress::Manual { section, .. } => format!("manual/{section}"),
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
