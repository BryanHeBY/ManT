//! Query execution boundary; public entry points remain validated.
use super::{
    ProjectionError, QueryExcerpt, QueryExecutionError, QueryView, QueryViewResult,
    ResolvedContent, SearchCase, SearchQuery, SearchScope, SearchSyntax, search_query,
    select_excerpt, select_explanation,
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
        QueryView::Outline { entries, root } => {
            { crate::projection::build_outline_projection(&query, entries.clone(), root.clone()) }
                .map(QueryViewResult::Outline)
                .map_err(QueryExecutionError::Projection)
        }
        QueryView::Excerpt { selectors } => select_excerpt(&query, selectors)
            .map(QueryViewResult::Excerpt)
            .map_err(QueryExecutionError::Projection),
        QueryView::Explain { entry } => select_explanation_with_text_hint(&query, entry)
            .map(QueryViewResult::Excerpt)
            .map_err(QueryExecutionError::Projection),
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

pub(crate) fn select_explanation_with_text_hint(
    query: &ResolvedContent,
    entry: &str,
) -> Result<QueryExcerpt, ProjectionError> {
    match select_explanation(query, entry) {
        Err(ProjectionError::UnknownSelector { document, selector }) => {
            let probe = SearchQuery {
                pattern: selector.clone(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Insensitive,
                scope: SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 1,
                offset: 0,
            };
            if let Some(found) = search_query(query, &probe)
                .ok()
                .and_then(|result| result.matches.into_iter().next())
            {
                let line = found
                    .occurrences
                    .first()
                    .map_or(1, |occurrence| occurrence.markdown.start_line);
                return Err(ProjectionError::SelectorFoundOnlyInText {
                    document,
                    selector,
                    path: found.outline.path().to_owned(),
                    title: found.outline.title().to_owned(),
                    line,
                });
            }
            Err(ProjectionError::UnknownSelector { document, selector })
        }
        result => result,
    }
}
use super::validation::validate_query_view;
