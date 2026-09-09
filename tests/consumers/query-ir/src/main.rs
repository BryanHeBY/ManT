//! Standalone queries over authored IR, with no parser, loader, or report renderer.

use mant_ir::{
    Block, DefinitionItem, DefinitionLayout, Document, DocumentAddress, DocumentMeta,
    DocumentSource, EntryFacts, EntryForm, EntryKind, EntryNameBinding, EntryNameEvidence, Inline,
    LayoutHint, LinkTarget, MarkdownOrigin, NameCase, ReferenceScope, ResolvedContent,
    SourceFormat,
};
use mant_protocol::{
    ContentSelector, DocumentEdge, DocumentEdgeKind, DocumentScope, DocumentSelector,
    DocumentTraversal, EntryProjection, ExcerptSelection, ExplanationOptions, ExplanationQuery,
    OutlineNode, ReferenceCount, ReferenceProjection, ReferenceProjectionMode,
    ResolvedDocumentScope, ScopedDocument, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};
use mant_query::{
    QueryScopeView, build_outline_projection, explain_query, explain_scope, project_references,
    search_query, search_scope, select_excerpt,
};

fn address(name: &str) -> DocumentAddress {
    DocumentAddress::Markdown {
        path: name.into(),
        origin: MarkdownOrigin::Documents,
    }
}

fn content(name: &str, links: bool) -> ResolvedContent {
    let mut body = vec![Inline::Text {
        value: format!("needle in {name}."),
    }];
    if links {
        for label in ["First reference", "Second reference"] {
            body.push(Inline::Text { value: " ".into() });
            body.push(Inline::Link {
                title: None,
                children: vec![Inline::Text {
                    value: label.into(),
                }],
                target: LinkTarget::Document {
                    name: "b".into(),
                    fragment: Some("command-run".into()),
                },
            });
        }
    }
    ResolvedContent {
        address: Some(address(name)),
        label: name.into(),
        tldr: None,
        document: Some(Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                // This label is provenance, not permission to read a physical path.
                path: Some(format!("not-loaded/{name}.md")),
            },
            meta: DocumentMeta::default(),
            heading: Some(name.into()),
            fragment_aliases: vec![],
            diagnostics: vec![],
            sections: vec![],
            blocks: vec![Block::DefinitionList {
                declaration_groups: vec![],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
                items: vec![DefinitionItem {
                    terms: vec![vec![Inline::Code {
                        value: "run".into(),
                    }]],
                    description: vec![Block::Paragraph {
                        children: body,
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    layout: DefinitionLayout::default(),
                    source: None,
                    entry: Some(EntryFacts {
                        id: "command-run".into(),
                        kind: EntryKind::Command,
                        case: NameCase::Sensitive,
                        names: vec!["run".into()],
                        name_bindings: vec![EntryNameBinding {
                            name: 0,
                            evidence: EntryNameEvidence::Declared,
                            occurrences: vec![EntryForm::term(0)],
                        }],
                        forms: vec![EntryForm::term(0)],
                        alias_groups: vec![],
                        alias_of: None,
                        value_domain: None,
                    }),
                }],
            }],
        }),
    }
}

fn exercise_queries() -> Result<(), Box<dyn std::error::Error>> {
    let documents = vec![content("a", true), content("b", false)];
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
        edges: vec![DocumentEdge {
            from: address("a"),
            to: address("b"),
            kind: DocumentEdgeKind::Document,
        }],
        frontier: vec![],
        unresolved: vec![],
        reference_limits: vec![],
    };
    // Test-only copies detect mutation; query inputs below always borrow originals.
    let original_documents = documents.clone();
    let original_graph = graph.clone();
    for document in &documents {
        assert!(mant_ir::validate_document(document.document.as_ref().unwrap()).is_empty());
    }
    let first = &documents[0];
    let selector = ContentSelector::id("command-run");
    let outline = build_outline_projection(first, EntryProjection::All, Some(selector.clone()))?;
    assert!(matches!(
        &outline.nodes[..],
        [OutlineNode::DocumentEntry { id, path, .. }]
        if id == "command-run" && path.as_ref() == "root/e1"
    ));
    let excerpt = select_excerpt(first, &[selector])?;
    let [ExcerptSelection::DocumentEntry { entry, .. }] = &excerpt.selections[..] else {
        panic!("exact semantic owner excerpt");
    };
    assert_eq!(entry.entry_owner().unwrap().facts().unwrap().names, ["run"]);

    let mut search = SearchQuery {
        pattern: "needle".into(),
        syntax: SearchSyntax::Literal,
        case: SearchCase::Sensitive,
        scope: SearchScope::Visible,
        word: false,
        context_lines: 0,
        limit: 1,
        offset: 0,
    };
    let matches = search_query(first, &search)?;
    assert_eq!((matches.total, matches.returned), (1, 1));
    let mut explanation = ExplanationQuery {
        entry: "run".into(),
        options: ExplanationOptions::default(),
    };
    let evidence = explain_query(first, &explanation)?;
    assert_eq!((evidence.total, evidence.returned), (1, 1));
    assert!(evidence.evidence[0].entry.is_some());
    assert!(evidence.evidence[0].content.is_some());

    let references = project_references(
        first.document.as_ref().unwrap(),
        first.address.as_ref(),
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            offset: 1,
            limit: 1,
            ..ReferenceProjection::default()
        },
    );
    assert_eq!(references.occurrences, ReferenceCount::Exact { value: 2 });
    assert_eq!(references.targets, ReferenceCount::Exact { value: 1 });
    assert_eq!(references.records.len(), 1);
    assert_eq!((references.page.offset, references.page.returned), (1, 1));

    let view = QueryScopeView::new(&graph, &documents)?;
    assert!(std::ptr::eq(view.graph(), &graph));
    assert!(std::ptr::eq(view.documents(), documents.as_slice()));
    for ((_, borrowed), original) in view.iter().zip(&documents) {
        assert!(std::ptr::eq(borrowed, original));
    }
    search.offset = 1;
    let matches = search_scope(view, &search)?;
    assert_eq!((matches.total, matches.returned), (2, 1));
    assert_eq!(matches.documents[0].address, address("b"));
    assert_eq!(matches.documents[0].matches[0].ordinal, 2);

    explanation.options = ExplanationOptions {
        limit: 1,
        offset: 1,
        content_bytes: 1,
    };
    let evidence = explain_scope(view, &explanation)?;
    assert_eq!((evidence.total, evidence.returned), (2, 1));
    assert_eq!(evidence.evidence[0].document_index, 1);
    assert!(evidence.evidence[0].evidence.content.is_none());
    assert!(evidence.evidence[0].evidence.content_omitted);
    assert_eq!(documents, original_documents);
    assert_eq!(graph, original_graph);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    exercise_queries()
}

#[cfg(test)]
mod tests {
    #[test]
    fn independent_ir_queries_preserve_sources_and_respect_result_budgets() {
        super::exercise_queries().unwrap();
    }
}
