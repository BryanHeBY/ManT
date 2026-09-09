//! Independent oracle: BFS reports do not dictate evidence page order.
use mant_ir::{DocumentAddress, MarkdownOrigin};
use mant_protocol::{
    DocumentScope, EvidenceClass, ExplanationOptions, ExplanationQuery, ResolvedDocumentScope,
    ScopedDocument,
};

fn loaded() -> (ResolvedDocumentScope, Vec<mant_ir::ResolvedContent>) {
    let mut documents = [
        "# A\n\n--help mentioned in ordinary prose.\n",
        "# B\n\n<!-- mant:entries role=option case=sensitive -->\n- `--help`: Direct. <!-- mant:entry {\"id\":\"same\"} -->\n- `-Q`: Mentions --help.\n",
        "# C\n\n<!-- mant:entries role=option case=sensitive -->\n- `--help`: Direct. <!-- mant:entry {\"id\":\"same\"} -->\n",
    ].map(|source| crate::query_markdown_text(source, None).unwrap()).to_vec();
    let sources: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .map(|path| ScopedDocument {
            address: DocumentAddress::Markdown {
                path: path.into(),
                origin: MarkdownOrigin::Documents,
            },
            depth: 0,
            root_indices: vec![],
            reached_from: vec![],
        })
        .collect();
    for (source, content) in sources.iter().zip(&mut documents) {
        content.address = Some(source.address.clone());
    }
    let graph = ResolvedDocumentScope {
        reference_limits: Vec::new(),
        query: DocumentScope {
            documents: vec![],
            traversal: mant_protocol::DocumentTraversal::default(),
        },
        documents: sources,
        edges: vec![],
        frontier: vec![],
        unresolved: vec![],
    };
    (graph, documents)
}

#[test]
fn global_classification_paging_and_source_report_counts_are_consistent() {
    let (graph, documents) = loaded();
    let input = crate::QueryScopeView::new(&graph, &documents).unwrap();
    let mut query = ExplanationQuery {
        entry: "--help".into(),
        options: ExplanationOptions::default(),
    };
    let full = super::explain(input, &query).unwrap();
    assert_eq!(full.total, 4);
    assert_eq!(
        full.evidence
            .iter()
            .map(|r| r.document_index)
            .collect::<Vec<_>>(),
        [1, 2, 1, 0]
    );
    assert_eq!(
        full.evidence[0].evidence.outline.node.id(),
        full.evidence[1].evidence.outline.node.id()
    );
    assert_eq!(
        full.documents
            .iter()
            .map(|d| d.address.catalog_path())
            .collect::<Vec<_>>(),
        ["documents/a", "documents/b", "documents/c"]
    );
    for class in EvidenceClass::ALL {
        assert_eq!(
            full.counts.get(class).total,
            full.documents
                .iter()
                .map(|d| d.counts.get(class).total)
                .sum::<u32>()
        );
    }
    query.options.limit = 1;
    let mut paged = Vec::new();
    loop {
        let page = super::explain(input, &query).unwrap();
        assert_eq!(page.returned, 1);
        assert_eq!(page.documents.iter().map(|d| d.returned).sum::<u32>(), 1);
        for class in EvidenceClass::ALL {
            assert_eq!(page.counts.get(class).total, full.counts.get(class).total);
            assert_eq!(
                page.counts.get(class).returned,
                page.documents
                    .iter()
                    .map(|d| d.counts.get(class).returned)
                    .sum::<u32>()
            );
        }
        paged.extend(page.evidence);
        if let Some(next) = page.next_offset {
            query.options.offset = next;
        } else {
            break;
        }
    }
    assert_eq!(paged, full.evidence);
    query.options.offset = 0;
    query.options.content_bytes = 1;
    let bounded = super::explain(input, &query).unwrap();
    assert_eq!(bounded.evidence[0].document_index, 1);
    assert_eq!(
        bounded.evidence[0].evidence.class,
        EvidenceClass::DirectEntry
    );
    assert!(bounded.evidence[0].evidence.details_omitted);
    assert_eq!(bounded.documents[0].returned, 0);
    assert!(!bounded.documents[0].truncation.content);
    assert!(bounded.documents[1].truncation.content);
    let value = serde_json::to_value(&full).unwrap();
    for doc in value["documents"].as_array().unwrap() {
        for absent in ["query", "explanation", "evidence", "nextOffset"] {
            assert!(doc.get(absent).is_none());
        }
    }
}

#[test]
fn source_indices_refer_to_readable_reports_not_the_loading_graph() {
    let (graph, mut documents) = loaded();
    documents[0].document = None;
    documents[0].tldr = None;
    let query = ExplanationQuery {
        entry: "--help".into(),
        options: ExplanationOptions::default(),
    };
    let result = super::explain(
        crate::QueryScopeView::new(&graph, &documents).unwrap(),
        &query,
    )
    .unwrap();
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.documents.len(), 2);
    assert_eq!(result.evidence[0].document_index, 0);
    assert_eq!(result.documents[0].address.catalog_path(), "documents/b");
    assert_eq!(result.evidence[1].document_index, 1);
    assert_eq!(result.documents[1].address.catalog_path(), "documents/c");
}
