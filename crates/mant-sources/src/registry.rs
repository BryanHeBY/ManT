//! Discovers hierarchical local Markdown documents from the user data directory.

mod scan;
mod selection;

use super::{SourceConfig, SourceConfigError, load_source_config};
pub(crate) use scan::managed_document_count;
use scan::{scan_directory, source_directory_ready};
use std::path::PathBuf;

/// Priority shared by built-in native manuals and cached tldr pages.
pub const BUILTIN_CONTENT_PRIORITY: i32 = 0;

/// Storage class for one registered Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisteredDocumentOrigin {
    /// A file inside the singular user `documents` tree.
    Documents,
    /// A file installed from one configured source.
    Source(String),
}

/// One Markdown document registered in `ManT`'s document namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredDocument {
    /// Extension-free path relative to this document's origin.
    pub logical_path: String,
    /// Absolute path of the readable Markdown file.
    pub path: PathBuf,
    /// Storage namespace used for precedence and explicit selection.
    pub origin: RegisteredDocumentOrigin,
    /// Configured priority relative to native manuals, or `None` for `documents/`.
    pub source_priority: Option<i32>,
}

/// Same-origin documents matching one selector at one precedence position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredDocumentMatch {
    /// Storage namespace shared by every candidate in this group.
    pub origin: RegisteredDocumentOrigin,
    /// Normalized selector whose first match produced this group.
    pub selector: String,
    /// Exact match or every ambiguous component-suffix match in this origin.
    pub documents: Vec<RegisteredDocument>,
}

/// Immutable snapshot of the configured Markdown document namespace.
///
/// Loading the snapshot reads `sources.toml` and scans each eligible
/// directory exactly once. Candidate fallback therefore changes only lookup
/// order; it never repeats filesystem discovery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisteredDocumentIndex {
    config: SourceConfig,
    documents: Vec<RegisteredDocument>,
    ready_sources: std::collections::BTreeSet<String>,
}

impl RegisteredDocumentIndex {
    /// Load the current user's registered Markdown namespace.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform data root or `sources.toml` is invalid.
    pub fn load() -> Result<Self, SourceConfigError> {
        let (paths, config) = load_source_config()?;
        let mut documents = scan_directory(&paths.documents, true)?
            .into_iter()
            .map(|(logical_path, path)| RegisteredDocument {
                logical_path,
                path,
                origin: RegisteredDocumentOrigin::Documents,
                source_priority: None,
            })
            .collect::<Vec<_>>();
        let mut ready_sources = std::collections::BTreeSet::new();
        for source in config.precedence() {
            let Some(priority) = config.get(source).map(|source| source.priority) else {
                continue;
            };
            let directory = paths.sources.join(source);
            if !source_directory_ready(&directory) {
                continue;
            }
            ready_sources.insert(source.to_owned());
            documents.extend(scan_directory(&directory, false)?.into_iter().map(
                |(logical_path, path)| RegisteredDocument {
                    logical_path,
                    path,
                    origin: RegisteredDocumentOrigin::Source(source.to_owned()),
                    source_priority: Some(priority),
                },
            ));
        }
        Ok(Self {
            config,
            documents,
            ready_sources,
        })
    }

    /// Documents in root-first and configured-source precedence order.
    #[must_use]
    pub fn documents(&self) -> &[RegisteredDocument] {
        &self.documents
    }
}

/// Find a registered document using root-first or explicit-source resolution.
///
/// # Errors
///
/// Returns an error when the platform data root or `sources.toml` is invalid.
pub fn find_registered_document_candidates(
    candidates: &[String],
    source: Option<&str>,
) -> Result<Option<RegisteredDocument>, SourceConfigError> {
    let index = RegisteredDocumentIndex::load()?;
    Ok(index.find(candidates, source)?.cloned())
}

/// List every root and configured-source candidate in fallback order.
///
/// Documents with the same logical path remain visible so callers can select
/// a source explicitly instead of losing shadowed candidates.
///
/// # Errors
///
/// Returns an error when the platform data root or `sources.toml` is invalid.
pub fn list_registered_documents() -> Result<Vec<RegisteredDocument>, SourceConfigError> {
    Ok(RegisteredDocumentIndex::load()?.documents)
}
