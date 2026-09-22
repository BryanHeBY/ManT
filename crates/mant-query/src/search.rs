//! Search the canonical, anchor-free Markdown presentation without using terminal wraps.

use std::{error::Error, fmt};

use mant_protocol::{MAX_SEARCH_PATTERN_CHARS, QuerySearch, SearchQuery};

use crate::ResolvedContent;

mod mapping;
mod owners;
mod plan;
mod render_compat;
pub(crate) use plan::SearchPlan;
pub use plan::validate_search_query;
use plan::{MAX_CONTEXT_LINES, MAX_SEARCH_LIMIT};

/// Invalid search input, missing logical content, or projection failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
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
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
        }
    }
}

impl Error for SearchError {}

/// Search one complete query with the stable v0.12 line-group pagination.
///
/// The canonical render comes from logical IR, never native terminal rows, so
/// a formatter's physical wrap cannot split a match or alter a result cursor.
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
    render_compat::search_with_matcher(query, request, matcher)
}

#[cfg(test)]
mod tests;
