//! Search canonical Flow/TLDR text or final Fixed output without viewport wraps.

use std::{error::Error, fmt};

use mant_protocol::{MAX_SEARCH_PATTERN_CHARS, QuerySearch, SearchQuery};

use crate::ResolvedContent;

mod fixed_visible;
mod mapping;
mod origins;
mod owners;
mod plan;
mod render_compat;
pub(crate) use plan::SearchPlan;
pub use plan::validate_search_query;
use plan::{MAX_CONTEXT_LINES, MAX_SEARCH_LIMIT};

/// Invalid search input, missing logical content, or projection failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// The selected Fixed document requires annotated search support.
    UnsupportedFixed,
    /// Search pattern contained no bytes.
    EmptyPattern,
    /// Search pattern exceeded the request bound.
    PatternTooLong,
    /// Result limit was zero or exceeded the protocol maximum.
    InvalidLimit,
    /// Requested context exceeded the protocol maximum.
    ContextTooLarge,
    /// The selected result has no authoritative document content.
    MissingContent,
    /// A closed response-local content projection could not be produced.
    ContentProjection,
    /// Regular-expression compilation or execution failed.
    InvalidPattern(String),
    /// The selected Fixed document has invalid display or source relationships.
    InvalidFixed(mant_codec::encode::EncodeError),
    /// Exact search accounting or a required projection exceeded its budget.
    ResourceLimit,
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFixed => {
                formatter.write_str("Fixed document search is not yet supported")
            }
            Self::EmptyPattern => formatter.write_str("search pattern must not be empty"),
            Self::PatternTooLong => write!(
                formatter,
                "search pattern exceeds the {MAX_SEARCH_PATTERN_CHARS}-character limit"
            ),
            Self::InvalidLimit => write!(
                formatter,
                "search limit must be between 1 and {MAX_SEARCH_LIMIT}"
            ),
            Self::ContextTooLarge => write!(
                formatter,
                "search context must not exceed {MAX_CONTEXT_LINES} lines"
            ),
            Self::MissingContent => {
                formatter.write_str("search requires authoritative document content")
            }
            Self::ContentProjection => {
                formatter.write_str("search content projection could not be constructed")
            }
            Self::InvalidPattern(message) => write!(formatter, "invalid search pattern: {message}"),
            Self::InvalidFixed(error) => write!(formatter, "{error}"),
            Self::ResourceLimit => formatter.write_str("search resource limit exceeded"),
        }
    }
}

impl Error for SearchError {}

/// Search one complete query with v0.12 occurrence pagination.
///
/// Flow/TLDR uses the canonical Markdown-visible extractor; Fixed uses native
/// final-surface selections and proven joins, independent of viewport wraps.
///
/// # Errors
///
/// Returns a search error for an invalid request or exhausted query resources.
pub fn search_query(
    query: &ResolvedContent,
    request: &SearchQuery,
) -> Result<QuerySearch, SearchError> {
    SearchPlan::new(request)?.execute(query, request.offset, request.limit)
}

fn search_with_matcher(
    query: &ResolvedContent,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
    if query
        .document
        .as_ref()
        .is_some_and(|document| matches!(document.body(), mant_ir::DocumentBodyRef::Fixed(_)))
    {
        fixed_visible::search_with_matcher(query, request, matcher)
    } else {
        render_compat::search_with_matcher(query, request, matcher)
    }
}

#[cfg(test)]
mod tests;
