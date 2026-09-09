use super::*;
use mant_ir::{DocumentReference, MarkdownOrigin};

#[test]
fn failed_resolution_is_cached_by_policy_and_qualified_selector_only_for_one_request() {
    let mut cache = ResolutionFailures::default();
    let mut calls = 0;
    let base = DocumentSelector {
        selector: "missing".into(),
        source: None,
        manual_section: None,
    };
    for (policy, selector) in [
        (QueryPolicy::Combined, base.clone()),
        (QueryPolicy::Combined, base.clone()),
        (QueryPolicy::ManualOnly, base.clone()),
        (
            QueryPolicy::Combined,
            DocumentSelector {
                source: Some("other".into()),
                ..base.clone()
            },
        ),
        (
            QueryPolicy::Combined,
            DocumentSelector {
                manual_section: Some("7".into()),
                ..base.clone()
            },
        ),
    ] {
        let result: Result<(), String> = cache.resolve(&selector, policy, || {
            calls += 1;
            Err("not found".into())
        });
        assert!(result.is_err());
    }
    assert_eq!(calls, 4);
    assert!(
        ResolutionFailures::default()
            .resolve(&base, QueryPolicy::Combined, || Ok::<_, String>(()))
            .is_ok()
    );
}

#[test]
fn unresolved_records_keep_distinct_origins_without_repeating_the_same_edge() {
    let scope = DocumentScope {
        documents: vec![],
        traversal: mant_protocol::DocumentTraversal::default(),
    };
    let mut resolution = ScopeResolution::new(&scope);
    let failure = UnresolvedDocument {
        from: None,
        selector: DocumentSelector {
            selector: "missing".into(),
            source: None,
            manual_section: None,
        },
        reason: "not found".into(),
    };
    resolution.record_unresolved(failure.clone());
    resolution.record_unresolved(failure.clone());
    resolution.record_unresolved(UnresolvedDocument {
        from: Some(DocumentAddress::Manual {
            name: "other".into(),
            manual_section: "1".into(),
        }),
        ..failure
    });
    assert_eq!(resolution.graph.unresolved.len(), 2);
    assert!(resolution.graph.unresolved[0].from.is_none());
}

#[test]
fn frontier_retains_unresolved_manual_targets_without_inventing_an_address() {
    let scope = DocumentScope {
        documents: vec![DocumentSelector {
            selector: "root".to_owned(),
            source: None,
            manual_section: Some("1".to_owned()),
        }],
        traversal: mant_protocol::DocumentTraversal {
            follow_links: true,
            max_depth: None,
            max_documents: Some(1),
        },
    };
    let from = DocumentAddress::Manual {
        name: "root".to_owned(),
        manual_section: "1".to_owned(),
    };
    let reference = ScopeReference {
        target: DocumentReference::Manual {
            name: "child".to_owned(),
            manual_section: None,
        },
        kind: DocumentEdgeKind::Manual,
        source_offset: None,
        sequence: 0,
    };
    let mut resolution = ScopeResolution::new(&scope);
    resolution.record_frontier(&from, &reference, TraversalLimit::MaxDocuments);

    assert_eq!(resolution.graph.frontier.len(), 1);
    assert_eq!(resolution.graph.frontier[0].target.selector, "child");
    assert_eq!(resolution.graph.frontier[0].target.manual_section, None);
    assert_eq!(
        resolution.graph.frontier[0].limit,
        TraversalLimit::MaxDocuments
    );
}

#[test]
fn normalized_content_budget_refuses_another_document_before_retaining_it() {
    let scope = DocumentScope {
        documents: vec![DocumentSelector {
            selector: "root".to_owned(),
            source: None,
            manual_section: None,
        }],
        traversal: mant_protocol::DocumentTraversal::default(),
    };
    let mut content =
        crate::query_markdown_text("# Child\n\nBody.\n", None).expect("fixture content");
    let address = DocumentAddress::Markdown {
        path: "child".into(),
        origin: MarkdownOrigin::Documents,
    };
    content.address = Some(address.clone());
    let bytes = normalized_content_bytes(&content);
    assert!(bytes > 0 && bytes <= MAX_SCOPE_CONTENT_BYTES);

    let mut resolution = ScopeResolution::new(&scope);
    resolution.content_bytes = MAX_SCOPE_CONTENT_BYTES - bytes + 1;

    let from = DocumentAddress::Markdown {
        path: "root".into(),
        origin: MarkdownOrigin::Documents,
    };
    let edge = DocumentEdge {
        from: from.clone(),
        to: address.clone(),
        kind: DocumentEdgeKind::Document,
    };
    assert!(!resolution.insert_linked(content, address, &from, 1, edge));
    assert_eq!(
        resolution.content_bytes,
        MAX_SCOPE_CONTENT_BYTES - bytes + 1
    );
    assert!(resolution.documents.is_empty());
    assert!(resolution.graph.documents.is_empty());
    assert!(resolution.graph.edges.is_empty());
    assert!(resolution.positions.is_empty());
    assert!(resolution.queue.is_empty());
}

#[test]
fn scope_admission_keeps_graph_content_queue_and_budget_in_one_commit() {
    let selector = DocumentSelector {
        selector: "root".into(),
        source: None,
        manual_section: None,
    };
    let scope = DocumentScope {
        documents: vec![selector.clone(), selector.clone()],
        traversal: mant_protocol::DocumentTraversal {
            follow_links: true,
            max_depth: Some(2),
            max_documents: Some(4),
        },
    };
    let mut root = crate::query_markdown_text("# Root\n\nBody.\n", None).unwrap();
    let address = DocumentAddress::Markdown {
        path: "root".into(),
        origin: MarkdownOrigin::Documents,
    };
    root.address = Some(address.clone());
    let root_bytes = normalized_content_bytes(&root);
    let mut resolution = ScopeResolution::new(&scope);
    resolution.insert_root(root.clone(), &selector, 0);
    resolution.insert_root(root, &selector, 1);
    assert_eq!(resolution.content_bytes, root_bytes);
    assert_eq!(resolution.documents.len(), 1);
    assert_eq!(resolution.graph.documents[0].root_indices, [0, 1]);
    assert_eq!(resolution.queue.iter().copied().collect::<Vec<_>>(), [0]);

    let mut child = crate::query_markdown_text("# Child\n\nBody.\n", None).unwrap();
    let child_address = DocumentAddress::Markdown {
        path: "child".into(),
        origin: MarkdownOrigin::Documents,
    };
    child.address = Some(child_address.clone());
    let child_bytes = normalized_content_bytes(&child);
    let edge = DocumentEdge {
        from: address.clone(),
        to: child_address.clone(),
        kind: DocumentEdgeKind::Document,
    };
    // Synthetic near-limit accounting avoids allocating a 64 MiB fixture.
    resolution.content_bytes = MAX_SCOPE_CONTENT_BYTES - child_bytes;
    assert!(resolution.insert_linked(child, child_address.clone(), &address, 1, edge.clone()));
    assert_eq!(resolution.content_bytes, MAX_SCOPE_CONTENT_BYTES);
    assert_eq!(resolution.positions.get(&child_address), Some(&1));
    assert_eq!(resolution.queue.iter().copied().collect::<Vec<_>>(), [0, 1]);
    assert!(resolution.record_existing_edge(&edge));
    assert!(resolution.record_existing_edge(&DocumentEdge {
        from: child_address,
        to: address,
        kind: DocumentEdgeKind::Document,
    }));
    assert_eq!(resolution.documents.len(), 2);
    assert_eq!(resolution.graph.edges.len(), 2);
    assert_eq!(resolution.content_bytes, MAX_SCOPE_CONTENT_BYTES);
    assert!(crate::QueryScopeView::new(&resolution.graph, &resolution.documents).is_ok());

    let mut rejected = crate::query_markdown_text("# Rejected\n", None).unwrap();
    rejected.address = Some(DocumentAddress::Markdown {
        path: "rejected".into(),
        origin: MarkdownOrigin::Documents,
    });
    resolution.insert_root(rejected, &selector, 0);
    assert_eq!(resolution.graph.unresolved.len(), 1);
    assert_eq!(resolution.documents.len(), 2);
    assert_eq!(resolution.graph.documents.len(), 2);
    assert_eq!(resolution.graph.edges.len(), 2);
    assert_eq!(resolution.positions.len(), 2);
    assert_eq!(resolution.queue.iter().copied().collect::<Vec<_>>(), [0, 1]);
    assert_eq!(resolution.content_bytes, MAX_SCOPE_CONTENT_BYTES);
}

#[test]
fn root_content_budget_is_reported_as_an_unresolved_root() {
    let selector = DocumentSelector {
        selector: "root".to_owned(),
        source: None,
        manual_section: None,
    };
    let scope = DocumentScope {
        documents: vec![selector.clone()],
        traversal: mant_protocol::DocumentTraversal::default(),
    };
    let mut content =
        crate::query_markdown_text("# Root\n\nBody.\n", None).expect("fixture content");
    content.address = Some(DocumentAddress::Markdown {
        path: "root".to_owned(),
        origin: MarkdownOrigin::Documents,
    });
    let mut resolution = ScopeResolution::new(&scope);
    resolution.content_bytes = MAX_SCOPE_CONTENT_BYTES;

    resolution.insert_root(content, &selector, 0);

    assert!(resolution.documents.is_empty());
    assert_eq!(resolution.graph.unresolved.len(), 1);
    assert!(
        resolution.graph.unresolved[0]
            .reason
            .contains("aggregate scope content budget")
    );
}
