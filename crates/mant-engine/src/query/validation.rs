//! Full requests compose independent loading and query-view validation.
use super::{
    EntryProjection, LoadPolicy, MAX_NODE_SELECTORS, MAX_SEMANTIC_ENTRY_CHARS, QueryError,
    QueryRequest, QueryValidationError, QueryView, ScopeTextError, SearchQuery,
    validate_scope_text, validate_search_query,
};

/// Validate the complete request before local I/O.
///
/// # Errors
/// Returns the originating loading or query-view validation error.
pub fn validate_query_request(
    request: &QueryRequest,
    policy: LoadPolicy,
) -> Result<(), QueryError> {
    super::validate_load_spec(super::adapter::load_spec(&request.input), policy)?;
    validate_query_view(&request.view).map_err(QueryError::QueryValidation)
}

pub(super) fn validate_query_view(view: &QueryView) -> Result<(), QueryValidationError> {
    match view {
        QueryView::Excerpt { selectors } => {
            if selectors.is_empty() {
                return Err(QueryValidationError::EmptySelection);
            }
            if selectors.len() > MAX_NODE_SELECTORS {
                return Err(QueryValidationError::TooManySelections {
                    maximum: MAX_NODE_SELECTORS,
                });
            }
            for selector in selectors {
                selector
                    .validate()
                    .map_err(|_| QueryValidationError::InvalidContentSelector)?;
            }
        }
        QueryView::Explain { entry, options } => {
            validate_scope_text(entry, MAX_SEMANTIC_ENTRY_CHARS).map_err(|error| {
                if error == ScopeTextError::Empty {
                    QueryValidationError::EmptyEntry
                } else {
                    QueryValidationError::InvalidViewSelector {
                        field: "semantic entry",
                        error,
                    }
                }
            })?;
            mant_query::validate_explanation_query(&mant_protocol::ExplanationQuery {
                entry: entry.clone(),
                options: *options,
            })
            .map_err(QueryValidationError::InvalidExplanation)?;
        }
        QueryView::Search {
            pattern,
            syntax,
            case,
            scope,
            word,
            context_lines,
            limit,
            offset,
        } => validate_search_query(&SearchQuery {
            pattern: pattern.clone(),
            syntax: *syntax,
            case: *case,
            scope: *scope,
            word: *word,
            context_lines: *context_lines,
            limit: *limit,
            offset: *offset,
        })
        .map_err(QueryValidationError::InvalidSearch)?,
        QueryView::Outline {
            entries,
            root,
            references,
        } => {
            references
                .validate()
                .map_err(QueryValidationError::InvalidReferenceProjection)?;
            if let Some(selector) = root {
                selector
                    .validate()
                    .map_err(|_| QueryValidationError::InvalidContentSelector)?;
            }
            if let EntryProjection::Kinds { kinds } = entries
                && (kinds.is_empty() || kinds.len() > 9)
            {
                return Err(QueryValidationError::InvalidEntryKinds);
            }
        }
        QueryView::Full {} => {}
    }
    Ok(())
}
