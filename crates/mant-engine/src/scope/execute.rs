//! Application adapters join a loading report to a pure collection query.
use super::{LoadedDocumentScope, ScopeQueryError, ScopeQueryResult, SearchQuery};
use crate::{QueryScopeView, ScopeExecutionError};
use mant_protocol::{ScopeQueryRequest, ScopeQueryResponse, ScopeQuerySchema, ScopeQueryView};

impl crate::DocumentResolver {
    /// Resolve a scope and apply its closed multi-document projection.
    ///
    /// # Errors
    ///
    /// Returns request validation, resolution, or search errors. Ordinary
    /// per-document explanation misses do not fail the aggregate query.
    pub fn execute_scope_query(
        &self,
        request: &ScopeQueryRequest,
    ) -> Result<ScopeQueryResponse, ScopeQueryError> {
        super::validate_scope_query_request(request)?;
        let loaded = self.resolve_scope(&request.scope)?;
        let result = match &request.view {
            ScopeQueryView::Explain { entry, options } => execute_scope_explain(
                &loaded,
                &mant_protocol::ExplanationQuery {
                    entry: entry.clone(),
                    options: *options,
                },
            )?,
            ScopeQueryView::Search {
                pattern,
                syntax,
                case,
                scope,
                word,
                context_lines,
                limit,
                offset,
            } => execute_scope_search(
                &loaded,
                &SearchQuery {
                    pattern: pattern.clone(),
                    syntax: *syntax,
                    case: *case,
                    scope: *scope,
                    word: *word,
                    context_lines: *context_lines,
                    limit: *limit,
                    offset: *offset,
                },
            )?,
        };
        Ok(ScopeQueryResponse {
            schema: ScopeQuerySchema::V0Dot11,
            scope: loaded.into_parts().0,
            result,
        })
    }
}

pub(super) fn execute_scope_explain(
    loaded: &LoadedDocumentScope,
    query: &mant_protocol::ExplanationQuery,
) -> Result<ScopeQueryResult, ScopeQueryError> {
    let input = QueryScopeView::new(loaded.scope(), loaded.documents())
        .map_err(ScopeQueryError::InvalidLoadedScope)?;
    crate::explain_scope(input, query)
        .map(|explanation| ScopeQueryResult::Explain { explanation })
        .map_err(query_error)
}

pub(super) fn execute_scope_search(
    loaded: &LoadedDocumentScope,
    query: &SearchQuery,
) -> Result<ScopeQueryResult, ScopeQueryError> {
    let input = QueryScopeView::new(loaded.scope(), loaded.documents())
        .map_err(ScopeQueryError::InvalidLoadedScope)?;
    crate::search_scope(input, query)
        .map(|search| ScopeQueryResult::Search { search })
        .map_err(query_error)
}

fn query_error(error: ScopeExecutionError) -> ScopeQueryError {
    match error {
        ScopeExecutionError::Explanation(error) => ScopeQueryError::Explanation(error),
        ScopeExecutionError::Search(error) => ScopeQueryError::Search(error),
        ScopeExecutionError::NoReadableDocuments { reasons } => {
            ScopeQueryError::NoResolvedDocuments { reasons }
        }
    }
}
