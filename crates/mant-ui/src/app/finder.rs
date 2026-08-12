//! Live document-catalog filtering for the modal document finder.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ast::{
    CatalogQuery, DocumentAddress, DocumentCatalog, DocumentSummary, MarkdownOrigin, SearchCase,
    catalog_literal_match_rank,
};

use super::{App, Overlay};

#[derive(Debug, Default)]
pub(super) struct FinderState {
    pub(super) draft: String,
    pub(super) cursor: usize,
    pub(super) catalog: Vec<DocumentSummary>,
    pub(super) total: u32,
    pub(super) matches: Vec<usize>,
    pub(super) selected: usize,
}

impl FinderState {
    pub(super) fn replace_catalog(&mut self, catalog: DocumentCatalog) {
        self.total = catalog.total;
        self.catalog = catalog.documents;
        self.refresh();
    }

    pub(super) fn open(&mut self) {
        self.refresh();
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Option<DocumentAddress> {
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Enter => {
                return self
                    .matches
                    .get(self.selected)
                    .and_then(|index| self.catalog.get(*index))
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
                    self.refresh();
                }
            }
            KeyCode::Delete => {
                if let Some(next) = next_char_boundary(&self.draft, self.cursor) {
                    self.draft.drain(self.cursor..next);
                    self.refresh();
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
                self.refresh();
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.draft.insert(self.cursor, character);
                self.cursor += character.len_utf8();
                self.refresh();
            }
            _ => {}
        }
        None
    }

    fn refresh(&mut self) {
        let needle = self.draft.to_lowercase();
        self.matches = self
            .catalog
            .iter()
            .enumerate()
            .filter(|(_, document)| {
                needle.is_empty() || document.address.name().to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.matches.sort_by(|left, right| {
            let left = &self.catalog[*left].address;
            let right = &self.catalog[*right].address;
            catalog_literal_match_rank(
                left.name(),
                (!self.draft.is_empty()).then_some(self.draft.as_str()),
                SearchCase::Insensitive,
            )
            .cmp(&catalog_literal_match_rank(
                right.name(),
                (!self.draft.is_empty()).then_some(self.draft.as_str()),
                SearchCase::Insensitive,
            ))
            .then_with(|| left.name().to_lowercase().cmp(&right.name().to_lowercase()))
            .then_with(|| left.cmp(right))
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
        self.finder.open();
        self.pending_discovery = Some(finder_query(&self.finder.draft));
        self.overlay = Overlay::DocumentFinder;
    }

    pub(super) fn handle_finder_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.pending_discovery = None;
            self.overlay = Overlay::None;
            return;
        }
        let previous_draft = self.finder.draft.clone();
        if let Some(address) = self.finder.handle_key(key) {
            self.overlay = Overlay::None;
            self.request_open(address, None);
        } else if self.finder.draft != previous_draft {
            self.pending_discovery = Some(finder_query(&self.finder.draft));
        }
    }
}

fn finder_query(draft: &str) -> CatalogQuery {
    CatalogQuery {
        pattern: (!draft.is_empty()).then(|| draft.to_owned()),
        limit: 250,
        ..CatalogQuery::default()
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
