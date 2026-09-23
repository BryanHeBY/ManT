use mant_ir::{DocumentAddress, MarkdownOrigin, ResolvedContent};
use mant_protocol::{
    DocumentScope, DocumentTraversal, ResolvedDocumentScope, ScopedDocument, SearchCase,
    SearchQuery, SearchSyntax,
};

fn address(path: &str) -> DocumentAddress {
    DocumentAddress::Markdown {
        path: path.into(),
        origin: MarkdownOrigin::Documents,
    }
}

fn manual(path: &str) -> ResolvedContent {
    let mut content = crate::query_fixture::markdown(
        "# Manual\n\nA manual needle appears here.\n",
        Some(format!("{path}.md")),
    )
    .unwrap();
    content.address = Some(address(path));
    content
}

fn tldr_only(path: &str) -> ResolvedContent {
    let mut content = crate::query_fixture::markdown(
        "<!-- mant:tldr:start -->\n# quick\n\n> A quick needle appears here.\n\n- Run it:\n\n`quick`\n<!-- mant:tldr:end -->\n",
        Some(format!("{path}.md")),
    )
    .unwrap();
    content.address = Some(address(path));
    content.document = None;
    content
}

fn scope(contents: &[ResolvedContent]) -> ResolvedDocumentScope {
    ResolvedDocumentScope {
        query: DocumentScope {
            documents: Vec::new(),
            traversal: DocumentTraversal::default(),
        },
        documents: contents
            .iter()
            .map(|content| ScopedDocument {
                address: content.address.clone().unwrap(),
                depth: 0,
                root_indices: Vec::new(),
                reached_from: Vec::new(),
            })
            .collect(),
        edges: Vec::new(),
        frontier: Vec::new(),
        unresolved: Vec::new(),
        reference_limits: Vec::new(),
    }
}

fn query(pattern: &str) -> SearchQuery {
    SearchQuery {
        pattern: pattern.into(),
        syntax: SearchSyntax::Literal,
        case: SearchCase::Insensitive,
        scope: mant_protocol::SearchScope::Visible,
        word: false,
        context_lines: 0,
        limit: 100,
        offset: 0,
    }
}

#[test]
fn aggregate_metadata_budget_is_not_repaid_between_documents() {
    let mut bytes = mant_protocol::MAX_SEARCH_PRESENTATION_BYTES - 4;
    let error = super::accumulate_metadata_bytes(&"diagnostic", &mut bytes)
        .expect_err("serialized coverage must count against the aggregate budget");
    assert!(matches!(
        error,
        crate::ScopeExecutionError::Search(crate::search::SearchError::ResourceLimit)
    ));
}

#[test]
fn tldr_only_scope_search_retains_rendered_hit_without_authored_source() {
    let contents = vec![tldr_only("quick")];
    let graph = scope(&contents);
    let result = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &query("quick needle"),
    )
    .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.documents.len(), 1);
    assert!(result.documents[0].source_context.is_none());
    assert!(result.documents[0].content_projection.is_some());
    assert_eq!(result.coverage_by_document.len(), 1);
    assert!(result.coverage_by_document[0].source_context.is_none());
    assert!(result.documents[0].matches[0].node_source.is_none());
    serde_json::from_value::<mant_protocol::ScopeSearch>(serde_json::to_value(result).unwrap())
        .unwrap();
}

#[test]
fn zero_hit_tldr_only_search_stays_empty() {
    let contents = vec![tldr_only("quick")];
    let graph = scope(&contents);
    let result = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &query("absent"),
    )
    .unwrap();
    assert_eq!(result.total, 0);
    assert!(result.documents.is_empty());
    assert_eq!(result.coverage_by_document.len(), 1);
}

#[test]
fn mixed_manual_and_tldr_pages_keep_stable_global_pagination() {
    let contents = vec![manual("manual"), tldr_only("quick")];
    let graph = scope(&contents);
    let mut request = query("needle");
    request.limit = 1;
    let first = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &request,
    )
    .unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.next_offset, Some(1));
    assert_eq!(first.documents[0].matches[0].ordinal, 1);

    request.offset = 1;
    let second = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &request,
    )
    .unwrap();
    assert_eq!(second.total, 2);
    assert_eq!(second.next_offset, None);
    assert_eq!(second.documents[0].matches[0].ordinal, 2);
    assert!(second.documents[0].content_projection.is_some());
    assert_eq!(second.coverage_by_document.len(), 2);
}

#[test]
fn scoped_document_hits_keep_rendered_coordinates() {
    let contents = vec![manual("manual")];
    let graph = scope(&contents);
    let result = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &query("needle"),
    )
    .unwrap();

    assert_eq!(result.total, 1);
    assert!(result.documents[0].content_projection.is_some());
    assert!(matches!(
        result.documents[0].matches[0].location,
        mant_protocol::SearchLocation::VisibleFlow { .. }
    ));
    serde_json::from_value::<mant_protocol::ScopeSearch>(serde_json::to_value(result).unwrap())
        .unwrap();
}

#[test]
fn global_pagination_counts_complete_occurrences() {
    let contents = vec![manual("first"), manual("second")];
    let graph = scope(&contents);
    let mut request = query("needle");
    request.limit = 1;

    let first = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &request,
    )
    .unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.next_offset, Some(1));
    assert_eq!(first.documents[0].matches[0].ordinal, 1);

    request.offset = 1;
    let second = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &request,
    )
    .unwrap();
    assert_eq!(second.total, 2);
    assert_eq!(second.next_offset, None);
    assert_eq!(second.documents[0].matches[0].ordinal, 2);
}

#[test]
fn exhausted_page_counts_later_documents_without_materializing_fake_hits() {
    let contents = vec![manual("first"), tldr_only("quick"), manual("last")];
    let graph = scope(&contents);
    let mut request = query("needle");
    request.limit = 1;

    let result = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &request,
    )
    .unwrap();
    assert_eq!(result.total, 3);
    assert_eq!(result.returned, 1);
    assert_eq!(result.next_offset, Some(1));
    assert_eq!(result.documents.len(), 1);
    assert_eq!(result.documents[0].matches.len(), 1);
    assert_eq!(result.documents[0].matches[0].ordinal, 1);

    request.offset = 2;
    let last = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &request,
    )
    .unwrap();
    assert_eq!(last.total, 3);
    assert_eq!(last.returned, 1);
    assert_eq!(last.next_offset, None);
    assert_eq!(last.documents.len(), 1);
    assert_eq!(last.documents[0].matches[0].ordinal, 3);
}

#[test]
fn zero_hit_document_coverage_survives_global_pagination() {
    let mut first =
        crate::query_fixture::markdown("# Manual\n\nNo match here\n", Some("first.md".to_owned()))
            .unwrap();
    first.address = Some(address("first"));
    first
        .document
        .as_mut()
        .unwrap()
        .diagnostics
        .push(mant_ir::Diagnostic {
            level: mant_ir::DiagnosticLevel::Unsupported,
            impact: mant_ir::DiagnosticImpact::SemanticCoverage,
            code: Some("test.unverified".to_owned()),
            message: "test coverage gap".to_owned(),
            source: None,
            coverage_scope: Some(mant_ir::CoverageScope::Document),
        });
    let second = manual("second");
    let contents = vec![first, second];
    let graph = scope(&contents);
    let mut request = query("needle");
    request.offset = 0;
    request.limit = 1;
    let result = super::search_scope(
        crate::QueryScopeView::new(&graph, &contents).unwrap(),
        &request,
    )
    .unwrap();
    assert_eq!(result.coverage_by_document.len(), 2);
    assert!(!result.semantics_complete);
    assert!(!result.coverage_by_document[0].semantics_complete);
    assert!(result.coverage_by_document[0].source_context.is_some());
    assert_eq!(result.documents.len(), 1);
    assert_eq!(result.documents[0].address, address("second"));
}
