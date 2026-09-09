//! Resolves typed document links into bounded, deterministic query scopes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::{error::Error, fmt, io::Write};

#[cfg(test)]
use mant_ir::{Block, Inline};
use mant_ir::{DocumentAddress, DocumentReference, ResolvedContent};
use mant_protocol::{
    DocumentEdge, DocumentEdgeKind, DocumentFrontier, DocumentScope, DocumentSelector,
    MAX_DOCUMENT_SELECTOR_CHARS, MAX_SCOPE_CONTENT_BYTES, MAX_SCOPE_DEPTH,
    MAX_SCOPE_DOCUMENT_LIMIT, MAX_SCOPE_DOCUMENTS, MAX_SEMANTIC_ENTRY_CHARS, QueryInput,
    QueryRequest, RequestSchema, ResolvedDocumentScope, ScopeQueryRequest, ScopeQueryResult,
    ScopeQueryView, ScopeTextError, ScopedDocument, SearchQuery, TraversalLimit,
    UnresolvedDocument, validate_scope_text,
};

use crate::{DocumentResolver, QueryError, QueryPolicy, validate_search_query};

mod execute;
mod references;
mod resolve;
#[cfg(test)]
use execute::{execute_scope_explain, execute_scope_search};
#[cfg(test)]
use references::{ScopeReference, document_references};

/// A logical scope together with the loaded documents in matching order.
#[derive(Debug, Clone)]
pub struct LoadedDocumentScope {
    /// Transport-neutral logical graph.
    scope: ResolvedDocumentScope,
    /// Loaded documents in the same order as [`ResolvedDocumentScope::documents`].
    documents: Vec<ResolvedContent>,
}

impl LoadedDocumentScope {
    /// Logical graph and source coverage in the loader's stable order.
    #[must_use]
    pub const fn scope(&self) -> &ResolvedDocumentScope {
        &self.scope
    }
    /// Immutable original content paired with the logical graph.
    #[must_use]
    pub fn documents(&self) -> &[ResolvedContent] {
        &self.documents
    }
    /// Transfer ownership together, without cloning documents.
    #[must_use]
    pub fn into_parts(self) -> (ResolvedDocumentScope, Vec<ResolvedContent>) {
        (self.scope, self.documents)
    }
}

/// Invalid scope configuration or failure to resolve any initial document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeQueryError {
    /// A loading result did not satisfy the collection-query mapping contract.
    InvalidLoadedScope(crate::ScopeInputError),
    /// No initial document was supplied.
    EmptyScope,
    /// The initial document count exceeded the native bound.
    TooManyDocuments,
    /// Traversal depth exceeded the native bound.
    DepthLimit,
    /// The document budget was zero, too large, or smaller than the root set.
    DocumentLimit,
    /// Traversal limits were supplied while link following was disabled.
    TraversalLimitsRequireLinks,
    /// A logical document selector violated its native bound.
    DocumentSelector(ScopeTextError),
    /// A semantic-entry selector violated its native bound.
    EntrySelector(ScopeTextError),
    /// Invalid explanation result/content bounds.
    Explanation(crate::ExplanationError),
    /// Search configuration was invalid.
    Search(crate::SearchError),
    /// No initial document could be loaded.
    NoResolvedDocuments {
        /// Compact seed-resolution diagnostics.
        reasons: Vec<String>,
    },
}

impl fmt::Display for ScopeQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLoadedScope(error) => error.fmt(formatter),
            Self::EmptyScope => formatter.write_str("at least one document is required"),
            Self::TooManyDocuments => write!(
                formatter,
                "at most {MAX_SCOPE_DOCUMENTS} initial documents are allowed"
            ),
            Self::DepthLimit => write!(
                formatter,
                "maximum link depth must not exceed {MAX_SCOPE_DEPTH}"
            ),
            Self::DocumentLimit => write!(
                formatter,
                "document limit must include every initial document and not exceed {MAX_SCOPE_DOCUMENT_LIMIT}"
            ),
            Self::TraversalLimitsRequireLinks => {
                formatter.write_str("maxDepth and maxDocuments require followLinks=true")
            }
            Self::DocumentSelector(error) => {
                write!(
                    formatter,
                    "document selector {}",
                    scope_text_error_message(*error)
                )
            }
            Self::EntrySelector(error) => {
                write!(
                    formatter,
                    "semantic entry {}",
                    scope_text_error_message(*error)
                )
            }
            Self::Search(error) => error.fmt(formatter),
            Self::Explanation(error) => error.fmt(formatter),
            Self::NoResolvedDocuments { reasons } => {
                formatter.write_str("none of the initial documents could be resolved")?;
                if !reasons.is_empty() {
                    write!(formatter, ": {}", reasons.join("; "))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ScopeQueryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidLoadedScope(error) => Some(error),
            Self::Search(error) => Some(error),
            Self::Explanation(error) => Some(error),
            Self::EmptyScope
            | Self::TooManyDocuments
            | Self::DepthLimit
            | Self::DocumentLimit
            | Self::TraversalLimitsRequireLinks
            | Self::DocumentSelector(_)
            | Self::EntrySelector(_)
            | Self::NoResolvedDocuments { .. } => None,
        }
    }
}

/// Validate the closed scope-query contract before document I/O.
///
/// # Errors
///
/// Returns the first violated bound or projection invariant.
pub fn validate_scope_query_request(request: &ScopeQueryRequest) -> Result<(), ScopeQueryError> {
    validate_document_scope(&request.scope)?;
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

fn validate_document_scope(scope: &DocumentScope) -> Result<(), ScopeQueryError> {
    if scope.documents.is_empty() {
        return Err(ScopeQueryError::EmptyScope);
    }
    if scope.documents.len() > MAX_SCOPE_DOCUMENTS {
        return Err(ScopeQueryError::TooManyDocuments);
    }
    for selector in &scope.documents {
        validate_scope_text(&selector.selector, MAX_DOCUMENT_SELECTOR_CHARS)
            .map_err(ScopeQueryError::DocumentSelector)?;
    }
    if !scope.traversal.follow_links
        && (scope.traversal.max_depth.is_some() || scope.traversal.max_documents.is_some())
    {
        return Err(ScopeQueryError::TraversalLimitsRequireLinks);
    }
    if scope.traversal.effective_max_depth() > MAX_SCOPE_DEPTH {
        return Err(ScopeQueryError::DepthLimit);
    }
    let root_count = u32::try_from(scope.documents.len()).unwrap_or(u32::MAX);
    if scope.traversal.effective_max_documents() < root_count
        || scope.traversal.effective_max_documents() > MAX_SCOPE_DOCUMENT_LIMIT
    {
        return Err(ScopeQueryError::DocumentLimit);
    }
    Ok(())
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
    use mant_ir::{DocumentAddress, MarkdownOrigin};

    use super::*;

    #[test]
    fn entry_domains_participate_in_typed_document_traversal() {
        let parsed = crate::parse_markdown(
            "# SSH\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a key.\n\n  <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->\n",
            None,
        )
        .expect("semantic domain document");
        let references = document_references(&ResolvedContent {
            label: "ssh".to_owned(),
            address: Some(DocumentAddress::Manual {
                name: "ssh".to_owned(),
                manual_section: "1".to_owned(),
            }),
            document: Some(parsed.document),
            tldr: None,
        })
        .references;

        assert!(matches!(
            references.as_slice(),
            [ScopeReference {
                target: DocumentReference::Manual {
                    name,
                    manual_section: Some(section),
                },
                kind: DocumentEdgeKind::Manual,
                ..
            }] if name == "ssh_config" && section == "5"
        ));
    }

    #[test]
    fn ambiguous_entry_domains_do_not_participate_in_traversal() {
        let parsed = crate::parse_markdown(
            "# SSH\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a key.\n\n  <!-- mant:domain entries=first.md roles=configuration-key -->\n  <!-- mant:domain entries=second.md roles=configuration-key -->\n",
            None,
        )
        .expect("ambiguous semantic domain document");
        let references = document_references(&ResolvedContent {
            label: "ssh".to_owned(),
            address: Some(DocumentAddress::Markdown {
                path: "ssh".to_owned(),
                origin: MarkdownOrigin::Documents,
            }),
            document: Some(parsed.document),
            tldr: None,
        })
        .references;

        assert!(references.is_empty());
    }

    #[test]
    fn semantic_relationships_follow_authored_source_order() {
        let parsed = crate::parse_markdown(
            "# Tools\n\n<!-- mant:entries role=command case=sensitive -->\n- [`target`](target.md): See [description](description.md).\n\n  <!-- mant:domain entries=domain.md roles=command -->\n",
            None,
        )
        .expect("semantic relationships");
        let references = document_references(&ResolvedContent {
            label: "tools".to_owned(),
            address: Some(DocumentAddress::Markdown {
                path: "indexes/tools".to_owned(),
                origin: mant_ir::MarkdownOrigin::Documents,
            }),
            document: Some(parsed.document),
            tldr: None,
        })
        .references;

        assert_eq!(
            references
                .iter()
                .map(|reference| match &reference.target {
                    DocumentReference::Document { name, .. }
                    | DocumentReference::Manual { name, .. } => name.as_str(),
                })
                .collect::<Vec<_>>(),
            ["target", "description", "domain"]
        );
    }

    #[test]
    fn scope_bounds_include_every_root() {
        let scope = DocumentScope {
            documents: vec![
                DocumentSelector {
                    selector: "a".to_owned(),
                    source: None,
                    manual_section: None,
                },
                DocumentSelector {
                    selector: "b".to_owned(),
                    source: None,
                    manual_section: None,
                },
            ],
            traversal: mant_protocol::DocumentTraversal {
                follow_links: true,
                max_documents: Some(1),
                ..mant_protocol::DocumentTraversal::default()
            },
        };
        assert_eq!(
            validate_document_scope(&scope),
            Err(ScopeQueryError::DocumentLimit)
        );
    }

    #[test]
    fn explicit_traversal_limits_require_link_following() {
        let scope = DocumentScope {
            documents: vec![DocumentSelector {
                selector: "a".to_owned(),
                source: None,
                manual_section: None,
            }],
            traversal: mant_protocol::DocumentTraversal {
                follow_links: false,
                max_depth: Some(0),
                max_documents: None,
            },
        };
        assert_eq!(
            validate_document_scope(&scope),
            Err(ScopeQueryError::TraversalLimitsRequireLinks)
        );
    }

    #[test]
    fn native_scope_request_enforces_document_selector_contract() {
        let mut scope = DocumentScope {
            documents: vec![DocumentSelector {
                selector: "a".repeat(MAX_DOCUMENT_SELECTOR_CHARS + 1),
                source: None,
                manual_section: None,
            }],
            traversal: mant_protocol::DocumentTraversal::default(),
        };
        assert_eq!(
            validate_document_scope(&scope),
            Err(ScopeQueryError::DocumentSelector(ScopeTextError::TooLong {
                maximum: MAX_DOCUMENT_SELECTOR_CHARS,
            }))
        );

        scope.documents[0].selector = "界".repeat(MAX_DOCUMENT_SELECTOR_CHARS);
        assert_eq!(validate_document_scope(&scope), Ok(()));

        scope.documents[0].selector = "root\nother".to_owned();
        assert_eq!(
            validate_document_scope(&scope),
            Err(ScopeQueryError::DocumentSelector(
                ScopeTextError::ControlCharacter
            ))
        );
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
        let mut loaded = LoadedDocumentScope {
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
        let ScopeQueryResult::Explain { explanation } = execute_scope_explain(
            &loaded,
            &mant_protocol::ExplanationQuery {
                entry: "VISUAL".to_owned(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap() else {
            panic!("explain result");
        };
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
        let mut loaded = LoadedDocumentScope {
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

        let ScopeQueryResult::Search { search } =
            execute_scope_search(&loaded, &query).expect("scope search")
        else {
            panic!("search result");
        };

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
        let ScopeQueryResult::Search { search } =
            execute_scope_search(&loaded, &cross_boundary_query).expect("scope search")
        else {
            panic!("search result");
        };

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

    #[test]
    fn decoded_document_paths_still_respect_the_registered_namespace() {
        let from = DocumentAddress::Markdown {
            path: "guide/start".to_owned(),
            origin: MarkdownOrigin::Documents,
        };
        for (uri, expected) in [
            ("../space%20name.md", Some("documents/space name")),
            ("%2E%2E/%2E%2E/outside.md", None),
            ("literal%2520.md", Some("documents/guide/literal%20")),
        ] {
            let query = crate::query_markdown_text(&format!("[link]({uri})"), None).unwrap();
            let document = query.document.unwrap();
            let Block::Paragraph { children, .. } = &document.blocks[0] else {
                panic!("paragraph")
            };
            let Inline::Link { target, .. } = &children[0] else {
                panic!("link")
            };
            let reference = ScopeReference {
                target: DocumentReference::from_link_target(target).unwrap(),
                kind: DocumentEdgeKind::Document,
                source_offset: None,
                sequence: 0,
            };
            assert_eq!(
                reference.selector(&from).map(|s| s.selector),
                expected.map(str::to_owned),
                "{uri}"
            );
        }
    }

    #[test]
    fn relative_links_use_the_current_markdown_namespace() {
        let reference = ScopeReference {
            target: DocumentReference::Document {
                name: "../other".to_owned(),
                fragment: None,
            },
            kind: DocumentEdgeKind::Document,
            source_offset: None,
            sequence: 0,
        };
        let from = DocumentAddress::Markdown {
            path: "guide/start".to_owned(),
            origin: MarkdownOrigin::Documents,
        };
        assert_eq!(
            reference.selector(&from).map(|selector| selector.selector),
            Some("documents/other".to_owned())
        );
    }
}
