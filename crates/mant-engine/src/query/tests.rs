use super::{
    LoadError, LoadPolicy, QueryError, QueryExecutionError, QueryValidationError,
    project_query_view, query_markdown_text, validate_query_request,
};
use mant_protocol::{
    MAX_NODE_SELECTORS, MAX_SEMANTIC_ENTRY_CHARS, QueryInput, QueryRequest, QueryView,
    RequestSchema, ScopeTextError,
};
fn request() -> QueryRequest {
    QueryRequest {
        schema: RequestSchema::V0Dot11,
        input: QueryInput::Document {
            selector: " tool ".to_owned(),
            source: None,
            manual_section: None,
        },
        view: QueryView::Full {},
    }
}

#[test]
fn invalid_query_views_never_touch_the_loading_host() {
    for view in [
        QueryView::Excerpt {
            selectors: Vec::new(),
        },
        QueryView::Explain {
            entry: String::new(),
            options: mant_protocol::ExplanationOptions::default(),
        },
        QueryView::Outline {
            entries: mant_protocol::EntryProjection::Kinds { kinds: Vec::new() },
            root: None,
            references: mant_protocol::ReferenceProjection::default(),
        },
        QueryView::Search {
            pattern: "body".into(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Sensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 0,
            offset: 0,
        },
    ] {
        let calls = std::cell::Cell::new(0);
        let mut request = request();
        request.view = view;
        assert!(
            matches!(
                super::adapter::query_with(&request, LoadPolicy::Combined, || {
                    calls.set(calls.get() + 1);
                    unreachable!("invalid view cannot load")
                }),
                Err(QueryError::QueryValidation(_))
            ),
            "{:?}",
            request.view
        );
        assert_eq!(calls.get(), 0, "{:?}", request.view);
        // Constructing the real snapshot also performs configuration I/O.
        // A host-call counter alone does not guard that earlier boundary.
        let factory_calls = std::cell::Cell::new(0);
        assert!(matches!(
            super::validated_resolver(&request, LoadPolicy::Combined, || {
                factory_calls.set(factory_calls.get() + 1);
            }),
            Err(QueryError::QueryValidation(_))
        ));
        assert_eq!(factory_calls.get(), 0, "{:?}", request.view);
    }
}

#[test]
fn invalid_load_selection_never_constructs_a_system_snapshot() {
    let mut request = request();
    request.input = QueryInput::Document {
        selector: String::new(),
        source: None,
        manual_section: None,
    };
    let factory_calls = std::cell::Cell::new(0);
    assert!(matches!(
        super::validated_resolver(&request, LoadPolicy::Combined, || {
            factory_calls.set(factory_calls.get() + 1);
        }),
        Err(QueryError::Load(LoadError::EmptyName))
    ));
    assert_eq!(factory_calls.get(), 0);
}

#[test]
fn valid_query_constructs_exactly_one_snapshot_after_validation() {
    let factory_calls = std::cell::Cell::new(0);
    let snapshot = super::validated_resolver(&request(), LoadPolicy::Combined, || {
        factory_calls.set(factory_calls.get() + 1);
        "snapshot"
    })
    .unwrap();
    assert_eq!(snapshot, "snapshot");
    assert_eq!(factory_calls.get(), 1);
}

#[test]
fn every_single_document_selector_obeys_the_shared_native_bound() {
    let oversized = "x".repeat(MAX_SEMANTIC_ENTRY_CHARS + 1);
    for (field, view) in [
        (
            "semantic entry",
            QueryView::Explain {
                entry: oversized.clone(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        ),
        (
            "outline node",
            QueryView::Excerpt {
                selectors: vec![mant_protocol::ContentSelector::id(oversized.clone())],
            },
        ),
        (
            "outline root",
            QueryView::Outline {
                references: mant_protocol::ReferenceProjection::default(),
                entries: mant_protocol::EntryProjection::Summary,
                root: Some(mant_protocol::ContentSelector::id(oversized.clone())),
            },
        ),
    ] {
        let mut request = request();
        request.view = view;
        assert_eq!(
            validate_query_request(&request, LoadPolicy::default()),
            Err(if field == "semantic entry" {
                QueryError::QueryValidation(QueryValidationError::InvalidViewSelector {
                    field,
                    error: ScopeTextError::TooLong {
                        maximum: MAX_SEMANTIC_ENTRY_CHARS,
                    },
                })
            } else {
                QueryError::QueryValidation(QueryValidationError::InvalidContentSelector)
            })
        );
    }
}

#[test]
fn focused_projection_enforces_view_bounds_without_a_request_producer() {
    let query = query_markdown_text("# Demo\n\nBody.\n", None).expect("Markdown query");
    let oversized = "x".repeat(MAX_SEMANTIC_ENTRY_CHARS + 1);
    assert_eq!(
        project_query_view(
            query.clone(),
            &QueryView::Explain {
                entry: oversized,
                options: mant_protocol::ExplanationOptions::default()
            }
        ),
        Err(QueryExecutionError::Query(QueryError::QueryValidation(
            QueryValidationError::InvalidViewSelector {
                field: "semantic entry",
                error: ScopeTextError::TooLong {
                    maximum: MAX_SEMANTIC_ENTRY_CHARS,
                },
            }
        )))
    );

    let selectors = (0..=MAX_NODE_SELECTORS)
        .map(|index| mant_protocol::ContentSelector::id(format!("node-{index}")))
        .collect();
    assert_eq!(
        project_query_view(query, &QueryView::Excerpt { selectors }),
        Err(QueryExecutionError::Query(QueryError::QueryValidation(
            QueryValidationError::TooManySelections {
                maximum: MAX_NODE_SELECTORS,
            }
        )))
    );
}

#[test]
fn explanation_misses_distinguish_visible_prose_from_absent_text() {
    let query = query_markdown_text(
        "# shell\n\n## Invocation\n\nThe option `-b` ends option processing.\n",
        None,
    )
    .expect("Markdown query");
    let result = project_query_view(
        query.clone(),
        &QueryView::Explain {
            entry: "-b".to_owned(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .expect("prose is normal evidence");
    let crate::QueryViewResult::Explanation(result) = result else {
        panic!("expected evidence result");
    };
    assert_eq!(result.evidence[0].outline.path(), "1");
    assert_eq!(result.evidence[0].outline.title(), "Invocation");
    assert!(result.evidence[0].entry.is_none());
    assert!(result.evidence[0].source.is_some());

    assert!(matches!(
        project_query_view(
            query,
            &QueryView::Explain {
                entry: "--absent".to_owned(),
                options: mant_protocol::ExplanationOptions::default()
            },
        ),
        Ok(crate::QueryViewResult::Explanation(result)) if result.outcome == mant_protocol::ExplanationOutcome::NoEvidence
    ));
}
