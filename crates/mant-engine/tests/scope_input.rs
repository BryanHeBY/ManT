//! Collection query input is caller-owned IR, not a resolver or loading service.
use mant_ir::{
    Block, Document, DocumentAddress, DocumentMeta, DocumentSource, Inline, LayoutHint,
    MarkdownOrigin, ResolvedContent, SourceFormat,
};
use mant_protocol::{
    DocumentEdge, DocumentEdgeKind, DocumentScope, DocumentSelector, DocumentTraversal,
    ExplanationOptions, ExplanationQuery, ResolvedDocumentScope, ScopedDocument, SearchCase,
    SearchQuery, SearchScope, SearchSyntax,
};
use mant_query::{QueryScopeView, ScopeInputError, explain_scope, search_scope};

fn address(path: &str) -> DocumentAddress {
    DocumentAddress::Markdown {
        path: path.into(),
        origin: MarkdownOrigin::Documents,
    }
}

fn snapshot() -> (ResolvedDocumentScope, Vec<ResolvedContent>) {
    let documents = ["a", "b"]
        .map(|name| ResolvedContent {
            address: Some(address(name)),
            label: name.into(),
            tldr: None,
            document: Some(Document {
                parser: None,
                source: DocumentSource {
                    format: SourceFormat::Markdown,
                    path: None,
                },
                meta: DocumentMeta::default(),
                heading: None,
                fragment_aliases: vec![],
                diagnostics: vec![],
                sections: vec![],
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: format!("needle in {name}"),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                }],
            }),
        })
        .to_vec();
    let graph = ResolvedDocumentScope {
        query: DocumentScope {
            documents: ["a", "b"]
                .map(|name| DocumentSelector {
                    selector: name.into(),
                    source: None,
                    manual_section: None,
                })
                .to_vec(),
            traversal: DocumentTraversal::default(),
        },
        documents: ["a", "b"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| ScopedDocument {
                address: address(name),
                depth: 0,
                root_indices: vec![u16::try_from(index).unwrap()],
                reached_from: vec![],
            })
            .collect(),
        edges: vec![],
        frontier: vec![],
        unresolved: vec![],
        reference_limits: vec![],
    };
    (graph, documents)
}

#[test]
fn pure_collection_queries_reuse_exact_borrowed_ir_and_global_order() {
    let (graph, documents) = snapshot();
    let input = QueryScopeView::new(&graph, &documents).unwrap();
    assert!(std::ptr::eq(input.graph(), std::ptr::from_ref(&graph)));
    assert!(std::ptr::eq(input.documents(), documents.as_slice()));
    for ((source, content), expected) in input.iter().zip(&documents) {
        assert!(std::ptr::eq(content, expected));
        assert_eq!(Some(&source.address), content.address.as_ref());
    }
    let result = search_scope(
        input,
        &SearchQuery {
            pattern: "needle".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 1,
            offset: 1,
        },
    )
    .unwrap();
    assert_eq!(result.total, 2);
    assert_eq!(result.returned, 1);
    assert_eq!(result.documents[0].address, address("b"));
    assert_eq!(result.documents[0].matches[0].ordinal, 2);
    let result = explain_scope(
        input,
        &ExplanationQuery {
            entry: "needle".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.total, 2);
    assert_eq!(result.evidence[0].document_index, 0);
    assert_eq!(result.evidence[1].document_index, 1);
}

#[test]
fn mismatched_or_reordered_sets_cannot_silently_zip() {
    let (mut graph, mut documents) = snapshot();
    assert_eq!(
        QueryScopeView::new(&graph, &documents[..1]).unwrap_err(),
        ScopeInputError::LengthMismatch
    );
    documents.swap(0, 1);
    assert_eq!(
        QueryScopeView::new(&graph, &documents).unwrap_err(),
        ScopeInputError::AddressMismatch { index: 0 }
    );
    documents.swap(0, 1);
    documents[0].address = None;
    assert_eq!(
        QueryScopeView::new(&graph, &documents).unwrap_err(),
        ScopeInputError::AddressMismatch { index: 0 }
    );
    documents[0].address = Some(address("a"));
    graph.documents[1].address = address("a");
    documents[1].address = Some(address("a"));
    assert_eq!(
        QueryScopeView::new(&graph, &documents).unwrap_err(),
        ScopeInputError::DuplicateAddress { index: 1 }
    );
}

#[test]
fn source_coordinates_and_graph_provenance_must_refer_to_the_same_set() {
    let (mut graph, documents) = snapshot();
    graph.documents[0].root_indices = vec![2];
    assert_eq!(
        QueryScopeView::new(&graph, &documents).unwrap_err(),
        ScopeInputError::InvalidSource { index: 0 }
    );
    graph.documents[0].root_indices.clear();
    graph.documents[0].depth = 1;
    assert_eq!(
        QueryScopeView::new(&graph, &documents).unwrap_err(),
        ScopeInputError::InvalidSource { index: 1 }
    );
    graph.documents[0].depth = 0;
    graph.edges.push(DocumentEdge {
        from: address("a"),
        to: address("missing"),
        kind: DocumentEdgeKind::Document,
    });
    assert_eq!(
        QueryScopeView::new(&graph, &documents).unwrap_err(),
        ScopeInputError::UnknownGraphAddress
    );
    graph.edges[0].to = address("b");
    assert!(QueryScopeView::new(&graph, &documents).is_ok());
    graph.documents[1].reached_from.push(address("missing"));
    assert_eq!(
        QueryScopeView::new(&graph, &documents).unwrap_err(),
        ScopeInputError::UnknownGraphAddress
    );
}
