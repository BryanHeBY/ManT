//! Live document-catalog filtering for the modal document finder.

use std::collections::{BTreeMap, BTreeSet};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_protocol::{
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
    pub(super) tree: Vec<FinderTreeRow>,
    pub(super) selected: usize,
    expanded: BTreeSet<String>,
    tree_initialized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FinderTreeRow {
    Folder {
        path: String,
        name: String,
        depth: usize,
    },
    Document {
        index: usize,
        depth: usize,
    },
}

impl FinderState {
    pub(super) fn expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

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
                if self.draft.is_empty() {
                    match self.tree.get(self.selected).cloned() {
                        Some(FinderTreeRow::Folder { path, .. }) => {
                            if !self.expanded.remove(&path) {
                                self.expanded.insert(path);
                            }
                            self.rebuild_tree();
                        }
                        Some(FinderTreeRow::Document { index, .. }) => {
                            return self
                                .catalog
                                .get(index)
                                .map(|document| document.address.clone());
                        }
                        None => {}
                    }
                } else {
                    return self
                        .matches
                        .get(self.selected)
                        .and_then(|index| self.catalog.get(*index))
                        .map(|document| document.address.clone());
                }
            }
            KeyCode::Left if self.draft.is_empty() => self.collapse_selected(),
            KeyCode::Right if self.draft.is_empty() => self.expand_selected(),
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
                needle.is_empty()
                    || document.address.name().to_lowercase().contains(&needle)
                    || document
                        .address
                        .relative_path()
                        .to_lowercase()
                        .contains(&needle)
                    || needle.contains('/')
                        && document.catalog_path.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.matches.sort_by(|left, right| {
            let left_document = &self.catalog[*left];
            let right_document = &self.catalog[*right];
            let left = &left_document.address;
            let right = &right_document.address;
            finder_match_rank(left, &left_document.catalog_path, &self.draft)
                .cmp(&finder_match_rank(
                    right,
                    &right_document.catalog_path,
                    &self.draft,
                ))
                .then_with(|| left.name().to_lowercase().cmp(&right.name().to_lowercase()))
                .then_with(|| left.cmp(right))
        });
        self.rebuild_tree();
        self.selected = 0;
    }

    fn select_relative(&mut self, delta: isize) {
        let length = if self.draft.is_empty() {
            self.tree.len()
        } else {
            self.matches.len()
        };
        if length == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(length - 1);
    }

    fn rebuild_tree(&mut self) {
        let mut folders = BTreeSet::new();
        let mut documents = BTreeMap::<String, Vec<(String, usize)>>::new();
        for (index, document) in self.catalog.iter().enumerate() {
            let mut components = document.catalog_path.split('/').collect::<Vec<_>>();
            let Some(name) = components.pop() else {
                continue;
            };
            let parent = components.join("/");
            documents
                .entry(parent)
                .or_default()
                .push((name.to_owned(), index));
            for depth in 1..=components.len() {
                folders.insert(components[..depth].join("/"));
            }
        }
        if !self.tree_initialized {
            self.expanded.extend(folders.iter().cloned());
            self.tree_initialized = true;
        }
        self.tree.clear();
        append_tree_rows("", 0, &folders, &documents, &self.expanded, &mut self.tree);
    }

    fn collapse_selected(&mut self) {
        let Some(FinderTreeRow::Folder { path, .. }) = self.tree.get(self.selected) else {
            return;
        };
        if self.expanded.remove(path) {
            self.rebuild_tree();
        }
    }

    fn expand_selected(&mut self) {
        let Some(FinderTreeRow::Folder { path, .. }) = self.tree.get(self.selected) else {
            return;
        };
        if self.expanded.insert(path.clone()) {
            self.rebuild_tree();
        }
    }
}

fn finder_match_rank(
    address: &DocumentAddress,
    catalog_path: &str,
    pattern: &str,
) -> mant_protocol::CatalogMatchRank {
    let pattern = (!pattern.is_empty()).then_some(pattern);
    let relative_path = address.relative_path();
    [
        Some(address.name()),
        Some(relative_path.as_str()),
        pattern
            .is_some_and(|pattern| pattern.contains('/'))
            .then_some(catalog_path),
    ]
    .into_iter()
    .flatten()
    .map(|candidate| catalog_literal_match_rank(candidate, pattern, SearchCase::Insensitive))
    .min()
    .unwrap_or(mant_protocol::CatalogMatchRank::Unranked)
}

fn append_tree_rows(
    parent: &str,
    depth: usize,
    folders: &BTreeSet<String>,
    documents: &BTreeMap<String, Vec<(String, usize)>>,
    expanded: &BTreeSet<String>,
    output: &mut Vec<FinderTreeRow>,
) {
    let prefix = if parent.is_empty() {
        String::new()
    } else {
        format!("{parent}/")
    };
    let child_folders = folders
        .iter()
        .filter(|path| path.starts_with(&prefix))
        .filter(|path| !path[prefix.len()..].contains('/'))
        .cloned()
        .collect::<Vec<_>>();
    let child_documents = documents.get(parent).cloned().unwrap_or_default();
    let mut entries = child_folders
        .into_iter()
        .map(|path| {
            let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
            (name, true, path, 0)
        })
        .chain(
            child_documents
                .into_iter()
                .map(|(name, index)| (name, false, String::new(), index)),
        )
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.0
            .to_lowercase()
            .cmp(&right.0.to_lowercase())
            .then_with(|| right.1.cmp(&left.1))
    });
    for (name, folder, path, index) in entries {
        if folder {
            output.push(FinderTreeRow::Folder {
                path: path.clone(),
                name,
                depth,
            });
            if expanded.contains(&path) {
                append_tree_rows(&path, depth + 1, folders, documents, expanded, output);
            }
        } else {
            output.push(FinderTreeRow::Document { index, depth });
        }
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
        limit: if draft.is_empty() { 10_000 } else { 250 },
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
        } => format!("sources/{name}"),
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
