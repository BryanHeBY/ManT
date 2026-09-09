//! Resolves local manuals, registered Markdown, and tldr content into one query.

use std::{
    error::Error,
    ffi::OsStr,
    fmt, fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use mant_ir::{Document, DocumentAddress, MarkdownOrigin, ResolvedContent, TldrDocument};
use mant_protocol::{
    CatalogQuery, DocumentCatalog, EntryProjection, InputFormat, MAX_NODE_SELECTORS,
    MAX_SEMANTIC_ENTRY_CHARS, QueryExcerpt, QueryInput, QueryOutline, QueryRequest, QuerySearch,
    QueryView, ScopeTextError, SearchQuery, validate_scope_text,
};
use mant_sources::{RegisteredDocumentIndex, RegisteredDocumentOrigin, SourceConfigError};

use crate::{
    ManualIndex, ManualPage, ManualRequest, ProjectionError, SearchError, discover_manual_roots,
    executable::query_name_candidates, locate_manual_source_in, parse_manual_bytes,
    parse_manual_page, parse_manual_source, read_cached_tldr_page, search_query, select_excerpt,
    validate_search_query,
};

mod adapter;
mod execution;
mod input;
mod load;
mod load_error;
mod named;
mod resolver;
mod validation;
mod validation_error;
pub use adapter::DocumentResolver;
pub use execution::project_query_view;
use load::{
    FullDocumentMode, LoadHost, LoadedManual, QuickReferenceMode, RegisteredLookupPhase,
    RegisteredSelection, RegisteredSelectionGroup, read_capped_utf8,
};
pub use load::{LoadSpec, MAX_MARKDOWN_BYTES, QueryPolicy, validate_load_spec};
pub use load_error::{LoadError, ManualLoadError};
pub use resolver::DocumentLoader;
pub use validation::validate_query_request;
pub use validation_error::QueryValidationError;

#[cfg(test)]
use adapter::query_with;
pub use adapter::{query_markdown_text, query_roff_bytes};
use input::{load_markdown_text, load_roff_bytes, load_with};
#[cfg(test)]
use load::read_capped_utf8_io;
use named::query_named_document;

/// Complete-request validation or local loading failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// Source selection or acquisition failed.
    Load(LoadError),
    /// The requested view cannot be executed.
    QueryValidation(QueryValidationError),
}

impl From<LoadError> for QueryError {
    fn from(error: LoadError) -> Self {
        Self::Load(error)
    }
}
impl From<QueryValidationError> for QueryError {
    fn from(error: QueryValidationError) -> Self {
        Self::QueryValidation(error)
    }
}
impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(error) => error.fmt(formatter),
            Self::QueryValidation(error) => error.fmt(formatter),
        }
    }
}
impl Error for QueryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(error) => Some(error),
            Self::QueryValidation(error) => Some(error),
        }
    }
}

/// Materialized result of the view carried by a [`QueryRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryViewResult {
    /// Complete resolved content with no projection.
    Full(Box<ResolvedContent>),
    /// Lightweight structural outline.
    Outline(QueryOutline),
    /// One or more selected document nodes.
    Excerpt(QueryExcerpt),
    /// Independent semantic evidence; multiple and zero owners are normal.
    Explanation(mant_protocol::QueryExplanation),
    /// Paginatable structure-aware search result.
    Search(QuerySearch),
}

/// A valid request could not be loaded or projected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryExecutionError {
    /// Input validation or document loading failure.
    Query(QueryError),
    /// Outline or selection projection failure.
    Projection(ProjectionError),
    /// Search compilation or execution failure.
    Search(SearchError),
}

impl fmt::Display for QueryExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query(error) => error.fmt(formatter),
            Self::Projection(error) => error.fmt(formatter),
            Self::Search(error) => error.fmt(formatter),
        }
    }
}

impl Error for QueryExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Query(error) => Some(error),
            Self::Projection(error) => Some(error),
            Self::Search(error) => Some(error),
        }
    }
}

/// Query the local man database and optional offline tldr caches.
///
/// # Errors
///
/// Returns [`QueryError`] for invalid input or when neither source can produce
/// readable content.
pub fn resolve_query(request: &QueryRequest) -> Result<ResolvedContent, QueryError> {
    resolve_query_with_policy(request, QueryPolicy::default())
}

/// Query with an explicit input-resolution policy.
///
/// # Errors
///
/// Returns [`QueryError`] under the same conditions as [`resolve_query`].
pub fn resolve_query_with_policy(
    request: &QueryRequest,
    policy: QueryPolicy,
) -> Result<ResolvedContent, QueryError> {
    let resolver = DocumentResolver::from_system();
    resolver.resolve(request, policy)
}

/// Load and materialize the view encoded in one native request.
///
/// # Errors
///
/// Returns a typed loading, projection, or search failure.
pub fn execute_query(
    request: &QueryRequest,
    policy: QueryPolicy,
) -> Result<QueryViewResult, QueryExecutionError> {
    let resolver = DocumentResolver::from_system();
    resolver.execute(request, policy)
}

#[cfg(test)]
mod tests;
