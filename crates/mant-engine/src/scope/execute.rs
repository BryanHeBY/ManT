//! Application adapters join a loading report to a pure collection query.
use super::{LoadedDocumentScope, ScopeQueryError, ScopeQueryResult, SearchQuery};
use crate::QueryScopeView;
use mant_protocol::{ScopeQueryRequest, ScopeQueryResponse, ScopeQuerySchema, ScopeQueryView};

/// Validate a complete scope query before capturing the local source environment.
///
/// # Errors
///
/// Returns invalid query input, scope-loading failure, or query-execution failure.
pub fn execute_scope_query(
    request: &ScopeQueryRequest,
) -> Result<ScopeQueryResponse, ScopeQueryError> {
    validated_scope_resolver(request, crate::DocumentResolver::from_system)?
        .execute_validated_scope_query(request)
}

fn validated_scope_resolver<T>(
    request: &ScopeQueryRequest,
    factory: impl FnOnce() -> T,
) -> Result<T, ScopeQueryError> {
    super::validate_scope_query_request(request)?;
    Ok(factory())
}

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
        self.execute_validated_scope_query(request)
    }

    // Both public entry points validate the complete request exactly once before
    // this shared pipeline. Pure query APIs retain their own input validation.
    fn execute_validated_scope_query(
        &self,
        request: &ScopeQueryRequest,
    ) -> Result<ScopeQueryResponse, ScopeQueryError> {
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
        .map_err(ScopeQueryError::Execution)
}

pub(super) fn execute_scope_search(
    loaded: &LoadedDocumentScope,
    query: &SearchQuery,
) -> Result<ScopeQueryResult, ScopeQueryError> {
    let input = QueryScopeView::new(loaded.scope(), loaded.documents())
        .map_err(ScopeQueryError::InvalidLoadedScope)?;
    crate::search_scope(input, query)
        .map(|search| ScopeQueryResult::Search { search })
        .map_err(ScopeQueryError::Execution)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use mant_protocol::{
        DocumentScope, DocumentSelector, DocumentTraversal, ExplanationOptions,
        MAX_SEMANTIC_ENTRY_CHARS, ScopeRequestSchema, SearchCase, SearchScope, SearchSyntax,
    };

    fn request() -> ScopeQueryRequest {
        ScopeQueryRequest {
            schema: ScopeRequestSchema::V0Dot11,
            scope: DocumentScope {
                documents: vec![DocumentSelector {
                    selector: "root".to_owned(),
                    source: None,
                    manual_section: None,
                }],
                traversal: DocumentTraversal::default(),
            },
            view: ScopeQueryView::Explain {
                entry: "--help".to_owned(),
                options: ExplanationOptions::default(),
            },
        }
    }

    #[test]
    fn invalid_scope_views_and_roots_never_construct_the_environment() {
        let mut empty_roots = request();
        empty_roots.scope.documents.clear();
        let mut invalid_root = request();
        invalid_root.scope.documents[0].selector = "root\nother".to_owned();
        let mut invalid_entry = request();
        invalid_entry.view = ScopeQueryView::Explain {
            entry: "x".repeat(MAX_SEMANTIC_ENTRY_CHARS + 1),
            options: ExplanationOptions::default(),
        };
        let mut invalid_explanation_budget = request();
        invalid_explanation_budget.view = ScopeQueryView::Explain {
            entry: "--help".to_owned(),
            options: ExplanationOptions {
                content_bytes: 0,
                ..ExplanationOptions::default()
            },
        };
        let mut invalid_search = request();
        invalid_search.view = ScopeQueryView::Search {
            pattern: "needle".to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 101,
            limit: 10,
            offset: 0,
        };
        for invalid in [
            empty_roots,
            invalid_root,
            invalid_entry,
            invalid_explanation_budget,
            invalid_search,
        ] {
            let calls = Cell::new(0);
            let result = validated_scope_resolver(&invalid, || calls.set(calls.get() + 1));
            assert!(result.is_err());
            assert_eq!(
                calls.get(),
                0,
                "validation must precede environment discovery"
            );
        }
    }

    #[test]
    fn valid_scope_constructs_exactly_one_environment_after_validation() {
        let calls = Cell::new(0);
        let value = validated_scope_resolver(&request(), || {
            calls.set(calls.get() + 1);
            "snapshot"
        })
        .unwrap();
        assert_eq!(value, "snapshot");
        assert_eq!(calls.get(), 1);
    }
}
