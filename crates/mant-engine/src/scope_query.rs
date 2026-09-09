//! Pure queries over an already-loaded, caller-owned document snapshot.
mod input;
mod search;
pub use input::{QueryScopeView, ScopeInputError};
pub use search::search_scope;

use std::{error::Error, fmt};

/// Query failures independent of catalog resolution and source I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeExecutionError {
    /// The explanation request is invalid.
    Explanation(crate::ExplanationError),
    /// The search request or matcher is invalid.
    Search(crate::SearchError),
    /// None of the supplied snapshots contains readable explanation content.
    NoReadableDocuments {
        /// Per-document content failures in the supplied stable order.
        reasons: Vec<String>,
    },
}

impl fmt::Display for ScopeExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Explanation(error) => error.fmt(f),
            Self::Search(error) => error.fmt(f),
            Self::NoReadableDocuments { reasons } => {
                f.write_str("none of the supplied documents contains readable content")?;
                if !reasons.is_empty() {
                    write!(f, ": {}", reasons.join("; "))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ScopeExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Explanation(error) => Some(error),
            Self::Search(error) => Some(error),
            Self::NoReadableDocuments { .. } => None,
        }
    }
}

/// Explain across existing snapshots with global classification and paging.
/// No catalog lookup, parsing or source acquisition is performed.
///
/// # Errors
/// Returns invalid explanation bounds or failures when all content is unreadable.
pub fn explain_scope(
    input: QueryScopeView<'_>,
    query: &mant_protocol::ExplanationQuery,
) -> Result<mant_protocol::ScopeExplanation, ScopeExecutionError> {
    crate::explanation::explain_scope(input, query)
}
