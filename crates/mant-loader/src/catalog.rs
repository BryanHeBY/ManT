//! Unifies registered Markdown and indexed manual pages for discovery clients.

use crate::{ManualIndex, discover_manual_roots};
use mant_protocol::{CatalogQuery, DocumentCatalog, MAX_CATALOG_PATTERN_CHARS};
use mant_sources::{SourceConfigError, list_registered_documents};
use std::{error::Error, fmt, path::PathBuf};

mod inventory;
mod selection;
#[cfg(test)]
mod tests;

pub(crate) use inventory::list_available_documents_from;
pub use selection::{PreparedCatalogQuery, query_available_documents};

/// Source family used to resolve one available document.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableDocumentKind {
    /// Registered Markdown document.
    Markdown,
    /// Indexed native manual page.
    Manual,
}

/// Precedence class and storage family for one available document.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableDocumentOrigin {
    /// User-authored primary documents tree.
    Documents,
    /// One configured source cache, named by its configuration key.
    Source(String),
    /// A directory discovered through the native manual search path.
    ManualPath,
}

/// One document discoverable by name through the ordinary query boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailableDocument {
    /// Short lookup name.
    pub name: String,
    /// Extension-free path relative to this document's origin.
    pub logical_path: String,
    /// Broad source format family.
    pub kind: AvailableDocumentKind,
    /// Native manual category, present only for manual pages.
    pub manual_section: Option<String>,
    /// Physical local source path.
    pub path: PathBuf,
    /// Storage namespace and precedence class.
    pub origin: AvailableDocumentOrigin,
    /// Configured priority relative to native manuals, or `None` otherwise.
    pub source_priority: Option<i32>,
}

/// Invalid document-catalog filter or regular expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogError {
    /// An explicit pattern contained no text.
    EmptyPattern,
    /// A pattern exceeded the bounded request size.
    PatternTooLong,
    /// Pagination limit was zero or exceeded the protocol maximum.
    InvalidLimit,
    /// Source-family filters cannot describe any valid document.
    ConflictingSelectors,
    /// A regular expression could not be compiled.
    InvalidPattern(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => formatter.write_str("catalog pattern must not be empty"),
            Self::PatternTooLong => {
                write!(
                    formatter,
                    "catalog pattern exceeds the {MAX_CATALOG_PATTERN_CHARS}-character limit"
                )
            }
            Self::InvalidLimit => formatter.write_str("catalog limit must be between 1 and 10000"),
            Self::ConflictingSelectors => {
                formatter.write_str("catalog source and manual-section filters cannot be combined")
            }
            Self::InvalidPattern(message) => {
                write!(formatter, "invalid catalog pattern: {message}")
            }
        }
    }
}

impl Error for CatalogError {}

/// List every registered document candidate and locally indexed manual page.
///
/// # Errors
///
/// Returns an error when the platform data root or source configuration cannot
/// be read or validated.
pub fn list_available_documents() -> Result<Vec<AvailableDocument>, SourceConfigError> {
    let manuals = ManualIndex::from_roots(discover_manual_roots());
    Ok(list_available_documents_from(
        list_registered_documents()?,
        manuals.pages(),
    ))
}

/// Load and query the current local document catalog.
///
/// # Errors
///
/// Returns source configuration or catalog validation failures as text because
/// both are operational boundaries for every frontend.
pub fn discover_documents(query: &CatalogQuery) -> Result<DocumentCatalog, String> {
    discover_with(query, list_available_documents)
}

fn discover_with(
    query: &CatalogQuery,
    inventory: impl FnOnce() -> Result<Vec<AvailableDocument>, SourceConfigError>,
) -> Result<DocumentCatalog, String> {
    let plan = PreparedCatalogQuery::new(query).map_err(|error| error.to_string())?;
    let documents = inventory().map_err(|error| error.to_string())?;
    Ok(plan.apply(&documents))
}
