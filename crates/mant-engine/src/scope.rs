//! Application-level scope request validation and query execution.
use crate::{DocumentResolver, validate_search_query};
use mant_loader::{LoadedDocumentScope, ScopeLoadError, validate_document_scope};
use mant_protocol::{
    DocumentScope, MAX_SEMANTIC_ENTRY_CHARS, ScopeQueryRequest, ScopeQueryResult, ScopeQueryView,
    ScopeTextError, SearchQuery, validate_scope_text,
};
use std::{error::Error, fmt};

mod execute;
pub use execute::execute_scope_query;

/// Invalid query configuration, failed scope loading, or query execution failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeQueryError {
    /// Source selection or initial-document acquisition failed.
    Load(ScopeLoadError),
    /// Pure collection-query execution failed after loading.
    Execution(crate::ScopeExecutionError),
    /// A loading result did not satisfy the collection-query mapping contract.
    InvalidLoadedScope(crate::ScopeInputError),
    /// A semantic-entry selector violated its native bound.
    EntrySelector(ScopeTextError),
    /// Invalid explanation result/content bounds.
    Explanation(crate::ExplanationError),
    /// Search configuration was invalid.
    Search(crate::SearchError),
}
impl fmt::Display for ScopeQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(error) => error.fmt(formatter),
            Self::Execution(error) => error.fmt(formatter),
            Self::InvalidLoadedScope(error) => error.fmt(formatter),
            Self::EntrySelector(error) => write!(
                formatter,
                "semantic entry {}",
                scope_text_error_message(*error)
            ),
            Self::Explanation(error) => error.fmt(formatter),
            Self::Search(error) => error.fmt(formatter),
        }
    }
}
impl Error for ScopeQueryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(error) => Some(error),
            Self::Execution(error) => Some(error),
            Self::InvalidLoadedScope(error) => Some(error),
            Self::Explanation(error) => Some(error),
            Self::Search(error) => Some(error),
            Self::EntrySelector(_) => None,
        }
    }
}
impl DocumentResolver {
    /// Resolve typed links against this application's existing loader snapshot.
    ///
    /// # Errors
    ///
    /// Returns invalid traversal input or failure to load any initial document.
    pub fn resolve_scope(
        &self,
        scope: &DocumentScope,
    ) -> Result<LoadedDocumentScope, ScopeQueryError> {
        self.loader()
            .resolve_scope(scope)
            .map_err(ScopeQueryError::Load)
    }
}

/// Validate the closed scope-query contract before document I/O.
///
/// # Errors
///
/// Returns the first violated bound or projection invariant.
pub fn validate_scope_query_request(request: &ScopeQueryRequest) -> Result<(), ScopeQueryError> {
    validate_document_scope(&request.scope).map_err(ScopeQueryError::Load)?;
    match &request.view {
        ScopeQueryView::Explain { entry, options } => {
            validate_scope_text(entry, MAX_SEMANTIC_ENTRY_CHARS)
                .map_err(ScopeQueryError::EntrySelector)?;
            crate::validate_explanation_query(&mant_protocol::ExplanationQuery {
                entry: entry.clone(),
                options: *options,
            })
            .map_err(ScopeQueryError::Explanation)
        }
        ScopeQueryView::Search {
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
        .map_err(ScopeQueryError::Search),
    }
}

fn scope_text_error_message(error: ScopeTextError) -> String {
    match error {
        ScopeTextError::Empty => "must not be empty".to_owned(),
        ScopeTextError::ControlCharacter => "must not contain control characters".to_owned(),
        ScopeTextError::TooLong { maximum } => {
            format!("must not exceed {maximum} Unicode scalar values")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::{DocumentAddress, MarkdownOrigin, ResolvedContent};
    use mant_protocol::{DocumentSelector, ResolvedDocumentScope, ScopedDocument};

    // Query tests own ordinary graph/content fixtures, not loader internals.
    struct QueryFixture {
        scope: ResolvedDocumentScope,
        documents: Vec<ResolvedContent>,
    }

    #[test]
    fn native_scope_request_enforces_entry_selector_contract() {
        let mut request = ScopeQueryRequest {
            schema: mant_protocol::ScopeRequestSchema::V0Dot11,
            scope: DocumentScope {
                documents: vec![DocumentSelector {
                    selector: "root".to_owned(),
                    source: None,
                    manual_section: None,
                }],
                traversal: mant_protocol::DocumentTraversal::default(),
            },
            view: ScopeQueryView::Explain {
                entry: "x".repeat(MAX_SEMANTIC_ENTRY_CHARS + 1),
                options: mant_protocol::ExplanationOptions::default(),
            },
        };
        assert_eq!(
            validate_scope_query_request(&request),
            Err(ScopeQueryError::EntrySelector(ScopeTextError::TooLong {
                maximum: MAX_SEMANTIC_ENTRY_CHARS,
            }))
        );

        request.view = ScopeQueryView::Explain {
            entry: "界".repeat(MAX_SEMANTIC_ENTRY_CHARS),
            options: mant_protocol::ExplanationOptions::default(),
        };
        assert_eq!(validate_scope_query_request(&request), Ok(()));
    }

    #[test]
    fn scope_explain_retains_a_visible_text_probe_as_a_qualified_failure() {
        let address = DocumentAddress::Markdown {
            path: "shell".to_owned(),
            origin: MarkdownOrigin::Documents,
        };
        let mut loaded = QueryFixture {
            scope: ResolvedDocumentScope {
                query: DocumentScope {
                    documents: vec![DocumentSelector {
                        selector: "documents/shell".to_owned(),
                        source: None,
                        manual_section: None,
                    }],
                    traversal: mant_protocol::DocumentTraversal::default(),
                },
                documents: vec![ScopedDocument {
                    address: address.clone(),
                    depth: 0,
                    root_indices: vec![0],
                    reached_from: Vec::new(),
                }],
                edges: Vec::new(),
                frontier: Vec::new(),
                unresolved: Vec::new(),
                reference_limits: Vec::new(),
            },
            documents: vec![
                crate::query_markdown_text(
                    "# Shell\n\n## Startup\n\nThe `VISUAL` name selects an editor.\n",
                    Some("shell.md".to_owned()),
                )
                .expect("probe fixture"),
            ],
        };

        for (source, content) in loaded.scope.documents.iter().zip(&mut loaded.documents) {
            content.address = Some(source.address.clone());
        }
        let explanation = crate::explain_scope(
            crate::QueryScopeView::new(&loaded.scope, &loaded.documents).unwrap(),
            &mant_protocol::ExplanationQuery {
                entry: "VISUAL".to_owned(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert!(explanation.failures.is_empty());
        assert_eq!(explanation.total, 1);
        assert_eq!(explanation.documents[0].address, address);
        assert_eq!(explanation.evidence[0].document_index, 0);
        let evidence = &explanation.evidence[0].evidence;
        assert_eq!(evidence.outline.path(), "1");
        assert_eq!(evidence.outline.title(), "Startup");
        assert!(evidence.entry.is_none());
        assert!(evidence.source.is_some());
    }

    #[test]
    fn scope_search_uses_one_global_cursor_and_global_ordinals() {
        let address = |path: &str| DocumentAddress::Markdown {
            path: path.to_owned(),
            origin: MarkdownOrigin::Documents,
        };
        let markdown = |title: &str, count: usize| {
            let body = (1..=count)
                .map(|index| format!("needle {index}"))
                .collect::<Vec<_>>()
                .join("\n\n");
            crate::query_markdown_text(&format!("# {title}\n\n{body}\n"), None)
                .expect("search fixture")
        };
        let documents = ["alpha", "beta"]
            .into_iter()
            .map(|path| ScopedDocument {
                address: address(path),
                depth: 0,
                root_indices: Vec::new(),
                reached_from: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut loaded = QueryFixture {
            scope: ResolvedDocumentScope {
                query: DocumentScope {
                    documents: Vec::new(),
                    traversal: mant_protocol::DocumentTraversal::default(),
                },
                documents,
                edges: Vec::new(),
                frontier: Vec::new(),
                unresolved: Vec::new(),
                reference_limits: Vec::new(),
            },
            documents: vec![markdown("Alpha", 3), markdown("Beta", 10)],
        };
        for (source, content) in loaded.scope.documents.iter().zip(&mut loaded.documents) {
            content.address = Some(source.address.clone());
        }
        let query = SearchQuery {
            pattern: "needle".to_owned(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Insensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 5,
            offset: 0,
        };

        let search = crate::search_scope(
            crate::QueryScopeView::new(&loaded.scope, &loaded.documents).unwrap(),
            &query,
        )
        .expect("scope search");

        assert_eq!(search.total, 13);
        assert_eq!(search.returned, 5);
        assert_eq!(search.next_offset, Some(5));
        assert_eq!(search.documents.len(), 2);
        assert_eq!(
            search
                .documents
                .iter()
                .flat_map(|document| document.matches.iter().map(|hit| hit.ordinal))
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );

        let cross_boundary_query = SearchQuery { offset: 2, ..query };
        let search = crate::search_scope(
            crate::QueryScopeView::new(&loaded.scope, &loaded.documents).unwrap(),
            &cross_boundary_query,
        )
        .expect("scope search");

        assert_eq!(search.total, 13);
        assert_eq!(search.returned, 5);
        assert_eq!(search.next_offset, Some(7));
        assert_eq!(search.documents.len(), 2);
        assert_eq!(
            search
                .documents
                .iter()
                .flat_map(|document| document.matches.iter().map(|hit| hit.ordinal))
                .collect::<Vec<_>>(),
            [3, 4, 5, 6, 7]
        );
    }
}
