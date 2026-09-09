//! Full request adapters compose view-independent loading and pure queries.
use mant_ir::ResolvedContent;
use mant_loader::{DocumentLoader, LoadError, LoadPolicy, LoadSpec, validate_load_spec};
use mant_protocol::{
    EntryProjection, MAX_NODE_SELECTORS, MAX_SEMANTIC_ENTRY_CHARS, QueryExcerpt, QueryInput,
    QueryOutline, QueryRequest, QuerySearch, QueryView, ScopeTextError, SearchQuery,
    validate_scope_text,
};
use mant_query::{
    ProjectionError, SearchError, search_query, select_excerpt, validate_search_query,
};
use std::{error::Error, fmt};
mod adapter;
mod execution;
mod prepared;
pub use prepared::PreparedQueryRequest;
mod validation;
mod validation_error;
pub use adapter::DocumentResolver;
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
    let (prepared, resolver) = validated_resolver(request, policy, DocumentResolver::from_system)?;
    prepared.resolve(&resolver)
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
    let (prepared, resolver) = validated_resolver(request, policy, DocumentResolver::from_system)
        .map_err(QueryExecutionError::Query)?;
    prepared.execute(&resolver)
}

// Capturing a system snapshot already reads manual configuration. Validation
// therefore precedes construction, not merely the loader's first lookup.
fn validated_resolver<T>(
    request: &QueryRequest,
    policy: LoadPolicy,
    factory: impl FnOnce() -> T,
) -> Result<(PreparedQueryRequest<'_>, T), QueryError> {
    let prepared = PreparedQueryRequest::new(request, policy)?;
    Ok((prepared, factory()))
}

#[cfg(test)]
mod tests;
