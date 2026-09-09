//! Full request adapters compose view-independent loading and pure queries.
use crate::{ProjectionError, SearchError, search_query, select_excerpt, validate_search_query};
use mant_ir::ResolvedContent;
use mant_loader::{DocumentLoader, LoadError, LoadPolicy, LoadSpec, validate_load_spec};
use mant_protocol::{
    EntryProjection, MAX_NODE_SELECTORS, MAX_SEMANTIC_ENTRY_CHARS, QueryExcerpt, QueryInput,
    QueryOutline, QueryRequest, QuerySearch, QueryView, ScopeTextError, SearchQuery,
    validate_scope_text,
};
use std::{error::Error, fmt};
mod adapter;
mod execution;
mod validation;
mod validation_error;
#[cfg(feature = "roff")]
pub use adapter::query_roff_bytes;
pub use adapter::{DocumentResolver, query_markdown_text};
pub use execution::project_query_view;
pub use validation::validate_query_request;
pub use validation_error::QueryValidationError;
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
    resolve_query_with_policy(request, LoadPolicy::default())
}

/// Query with an explicit input-resolution policy.
///
/// The entire request is validated before capturing local source configuration.
///
/// # Errors
///
/// Returns [`QueryError`] under the same conditions as [`resolve_query`].
pub fn resolve_query_with_policy(
    request: &QueryRequest,
    policy: LoadPolicy,
) -> Result<ResolvedContent, QueryError> {
    let resolver = validated_resolver(request, policy, DocumentResolver::from_system)?;
    resolver.resolve_validated(request, policy)
}

/// Load and materialize the view encoded in one native request.
///
/// Invalid input or view bounds are rejected before local source discovery.
///
/// # Errors
///
/// Returns a typed loading, projection, or search failure.
pub fn execute_query(
    request: &QueryRequest,
    policy: LoadPolicy,
) -> Result<QueryViewResult, QueryExecutionError> {
    let resolver = validated_resolver(request, policy, DocumentResolver::from_system)
        .map_err(QueryExecutionError::Query)?;
    resolver.execute_validated(request, policy)
}

// Capturing a system snapshot already reads manual configuration. Validation
// therefore precedes construction, not merely the loader's first lookup.
fn validated_resolver<T>(
    request: &QueryRequest,
    policy: LoadPolicy,
    factory: impl FnOnce() -> T,
) -> Result<T, QueryError> {
    validate_query_request(request, policy)?;
    Ok(factory())
}

#[cfg(test)]
mod tests;
