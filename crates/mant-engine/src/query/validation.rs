//! Query validation boundary; public entry points remain validated.
use super::{
    EntryProjection, MAX_DOCUMENT_SELECTOR_CHARS, MAX_NODE_SELECTORS, MAX_SEMANTIC_ENTRY_CHARS,
    MAX_SOURCE_SELECTOR_CHARS, QueryError, QueryInput, QueryPolicy, QueryRequest, QueryView,
    ScopeTextError, SearchQuery, validate_scope_text, validate_search_query,
};

/// Validate all request and policy invariants before local I/O.
///
/// # Errors
///
/// Returns the exact invalid input constraint.
pub fn validate_query_request(
    request: &QueryRequest,
    policy: QueryPolicy,
) -> Result<(), QueryError> {
    match &request.input {
        QueryInput::Document {
            selector,
            source,
            manual_section,
        } => {
            validate_scope_text(selector, MAX_DOCUMENT_SELECTOR_CHARS).map_err(|error| {
                if error == ScopeTextError::Empty {
                    QueryError::EmptyName
                } else {
                    QueryError::InvalidViewSelector {
                        field: "document selector",
                        error,
                    }
                }
            })?;
            if let Some(source) = source {
                validate_scope_text(source, MAX_SOURCE_SELECTOR_CHARS).map_err(|error| {
                    if error == ScopeTextError::Empty {
                        QueryError::InvalidSource
                    } else {
                        QueryError::InvalidViewSelector {
                            field: "document source",
                            error,
                        }
                    }
                })?;
            }
            if manual_section
                .as_deref()
                .is_some_and(|value| !crate::is_manual_section(value.trim()))
            {
                return Err(QueryError::InvalidManualSection);
            }
            if policy == QueryPolicy::TldrOnly
                && let Some(section) = manual_section.as_deref()
                && !crate::is_command_manual_section(section.trim())
            {
                return Err(QueryError::TldrManualSection {
                    section: section.trim().to_owned(),
                });
            }
            if source.is_some() && (manual_section.is_some() || policy == QueryPolicy::ManualOnly) {
                return Err(QueryError::ConflictingSourceSelectors);
            }
        }
        QueryInput::File { path, .. } => {
            if path.trim().is_empty() {
                return Err(QueryError::EmptyMarkdownPath);
            }
            if policy != QueryPolicy::Combined {
                return Err(QueryError::Markdown {
                    path: path.trim().to_owned(),
                    detail: "content-only policies do not apply to direct input".to_owned(),
                });
            }
        }
    }
    validate_query_view(&request.view)
}

pub(super) fn validate_query_view(view: &QueryView) -> Result<(), QueryError> {
    match view {
        QueryView::Excerpt { selectors } => {
            if selectors.is_empty() {
                return Err(QueryError::EmptySelection);
            }
            if selectors.len() > MAX_NODE_SELECTORS {
                return Err(QueryError::TooManySelections {
                    maximum: MAX_NODE_SELECTORS,
                });
            }
            for selector in selectors {
                selector
                    .validate()
                    .map_err(|_| QueryError::InvalidContentSelector)?;
            }
        }
        QueryView::Explain { entry, options } => {
            validate_scope_text(entry, MAX_SEMANTIC_ENTRY_CHARS).map_err(|error| {
                if error == ScopeTextError::Empty {
                    QueryError::EmptyEntry
                } else {
                    QueryError::InvalidViewSelector {
                        field: "semantic entry",
                        error,
                    }
                }
            })?;
            crate::validate_explanation_query(&mant_protocol::ExplanationQuery {
                entry: entry.clone(),
                options: *options,
            })
            .map_err(QueryError::InvalidExplanation)?;
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
        .map_err(QueryError::InvalidSearch)?,
        QueryView::Outline {
            entries,
            root,
            references,
        } => {
            references
                .validate()
                .map_err(QueryError::InvalidReferenceProjection)?;
            if let Some(selector) = root {
                selector
                    .validate()
                    .map_err(|_| QueryError::InvalidContentSelector)?;
            }
            if let EntryProjection::Kinds { kinds } = entries
                && (kinds.is_empty() || kinds.len() > 9)
            {
                return Err(QueryError::InvalidEntryKinds);
            }
        }
        QueryView::Full {} => {}
    }
    Ok(())
}
