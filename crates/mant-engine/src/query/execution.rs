//! Query execution boundary; public entry points remain validated.
use super::{
    QueryExecutionError, QueryView, QueryViewResult, ResolvedContent, SearchQuery, search_query,
    select_excerpt,
};

/// Materialize one view from an already loaded query.
///
/// # Errors
///
/// Returns a typed projection or search failure.
pub fn project_query_view(
    query: ResolvedContent,
    view: &QueryView,
) -> Result<QueryViewResult, QueryExecutionError> {
    validate_query_view(view).map_err(QueryExecutionError::Query)?;
    match view {
        QueryView::Full {} => Ok(QueryViewResult::Full(Box::new(query))),
        QueryView::Outline {
            entries,
            root,
            references,
        } => {
            crate::projection::build_outline_with_references(
                &query,
                entries.clone(),
                root.clone(),
                references,
            )
        }
        .map(QueryViewResult::Outline)
        .map_err(QueryExecutionError::Projection),
        QueryView::Excerpt { selectors } => select_excerpt(&query, selectors)
            .map(QueryViewResult::Excerpt)
            .map_err(QueryExecutionError::Projection),
        QueryView::Explain { entry, options } => crate::explain_query(
            &query,
            &mant_protocol::ExplanationQuery {
                entry: entry.clone(),
                options: *options,
            },
        )
        .map(QueryViewResult::Explanation)
        .map_err(|error| QueryExecutionError::Query(super::QueryError::InvalidExplanation(error))),
        QueryView::Search {
            pattern,
            syntax,
            case,
            scope,
            word,
            context_lines,
            limit,
            offset,
        } => search_query(
            &query,
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
        )
        .map(QueryViewResult::Search)
        .map_err(QueryExecutionError::Search),
    }
}

use super::validation::validate_query_view;
