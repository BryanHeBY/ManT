//! Exact path selection preserves the source's navigation decisions.
use mant_codec::encode::{MarkdownFragmentOptions, MarkdownMath};
use mant_ir::{
    DOCUMENT_ROOT_ID, DiagnosticImpact, Document, DocumentAddress, EntryForm, MarkdownOrigin,
    OVERVIEW_IDENTITY_COLLISION, ResolvedContent,
};
use mant_protocol::{
    ContentSelector, DocumentResponse, DocumentScope, DocumentSelector, ExplanationOptions,
    ExplanationQuery, QueryExcerpt, QueryExplanation, ResolvedDocumentScope, ScopeExplanation,
    ScopedDocument,
};
use pulldown_cmark::{Event, Parser, Tag};
use serde_json::{Value, json};

const OPTIONS: MarkdownFragmentOptions = MarkdownFragmentOptions {
    math: MarkdownMath::Readable,
    preserve_anchors: true,
};

fn text(value: &str) -> Value {
    json!({"type": "text", "value": value})
}

fn paragraph(children: Value) -> Value {
    let mut paragraph = json!({"type": "paragraph"});
    paragraph["children"] = children;
    paragraph
}

fn link(id: &str, label: &str) -> Value {
    json!({
        "type": "link",
        "target": {"kind": "section", "id": id},
        "children": [{"type": "strong", "children": [text(label)]}],
    })
}

fn anchor(id: &str, alias: &str) -> Value {
    json!({"type": "anchor", "id": id, "fragmentAliases": [alias]})
}

fn definition(id: &str, term: &str, description: Value) -> Value {
    let mut head = vec![text(term)];
    if id == DOCUMENT_ROOT_ID {
        head.insert(0, anchor(id, "Entry.Exact"));
    }
    let mut item = json!({
        "terms": [{"content": head}],
        "entry": {
            "id": id, "kind": {"kind": "term"}, "case": "sensitive", "names": [],
            "forms": [EntryForm::term(0)]
        }
    });
    item["description"] = description;
    item
}

fn block_at(placement: &str, children: Value) -> Value {
    if placement == "literal" {
        let mut block = paragraph(children);
        block["type"] = json!("preformatted");
        return block;
    }
    let body = paragraph(children);
    match placement {
        "paragraph" => body,
        "list" => json!({
            "type": "list", "kind": {"kind": "bullet"}, "items": [{"blocks": [body]}]
        }),
        "table" => json!({"type": "table", "rows": [{"cells": [{"blocks": [body]}]}]}),
        "definition" => json!({
            "type": "definition-list", "items": [{
                "terms": [{"content": [text("nested term")]}], "description": [body]
            }]
        }),
        _ => unreachable!("nested content placement"),
    }
}

fn document(root: &str, collision: &str, placement: &str, grouped: bool) -> Document {
    let mut value = json!({
        "source": {"format": "markdown"}, "meta": {}, "blocks": [], "sections": [],
    });
    let root_children = json!([text("ROOTBODY "), link(DOCUMENT_ROOT_ID, "ROOTJUMP"),]);
    match root {
        "blocks" => value["blocks"] = json!([paragraph(root_children)]),
        "heading" => value["heading"] = json!({"content": root_children}),
        "alias" => value["fragmentAliases"] = json!(["Root.Exact"]),
        "absent" => {}
        _ => unreachable!("source overview shape"),
    }
    let entry_id = if collision == "entry" {
        DOCUMENT_ROOT_ID
    } else {
        "selected-term"
    };
    let marker_id = if collision == "nested-anchor" {
        DOCUMENT_ROOT_ID
    } else {
        "nested-marker"
    };
    let section_id = if collision == "section" {
        DOCUMENT_ROOT_ID
    } else {
        "section"
    };
    let children = json!([
        anchor(marker_id, "Nested.Exact"),
        text("NESTEDBODY "),
        link(DOCUMENT_ROOT_ID, "ENTRYJUMP"),
    ]);
    let mut items = vec![definition(
        entry_id,
        "TERM",
        json!([
            block_at(
                if placement == "heading" {
                    "paragraph"
                } else {
                    placement
                },
                children
            ),
            paragraph(json!([
                anchor("allowed", "Valid.Exact"),
                link("allowed", "VALIDJUMP"),
                text(" "),
                link(DOCUMENT_ROOT_ID, "BODYJUMP"),
            ])),
        ]),
    )];
    let mut list = json!({"type": "definition-list"});
    if grouped {
        items.insert(0, definition("preface-term", "PREFACE", json!([])));
        list["declarationGroups"] = json!([{"startItem": 0, "endItem": 2}]);
    }
    list["items"] = json!(items);
    value["sections"] = json!([{
        "id": section_id, "fragmentAliases": ["Section.Exact"],
        "heading": {"content": if placement == "heading" {
            json!([text("SECTION "), link(DOCUMENT_ROOT_ID, "HEADINGJUMP")])
        } else {
            json!([text("SECTION")])
        }},
        "blocks": [list], "children": [],
    }]);
    serde_json::from_value(value).unwrap()
}

fn query(mut document: Document, validate: bool, label: &str) -> ResolvedContent {
    if validate {
        document
            .diagnostics
            .extend(mant_ir::validate_document(&document));
    }
    let decoded: DocumentResponse =
        serde_json::from_str(&serde_json::to_string(&DocumentResponse::from(&document)).unwrap())
            .unwrap();
    let restored: Document = decoded.into();
    assert_eq!(restored, document);
    ResolvedContent {
        label: label.into(),
        address: Some(DocumentAddress::Markdown {
            path: label.into(),
            origin: MarkdownOrigin::Documents,
        }),
        document: Some(restored),
        tldr: None,
    }
}

fn fragment_count(markdown: &str, target: &str) -> usize {
    Parser::new(markdown)
        .filter(|event| matches!(event, Event::Start(Tag::Link {dest_url, ..}) if dest_url.as_ref() == target))
        .count()
}

fn assert_inactive(markdown: &str) {
    assert_eq!(
        fragment_count(markdown, "#document-overview"),
        0,
        "{markdown}"
    );
    assert!(!markdown.contains("id=\"document-overview\""), "{markdown}");
}

fn excerpt_wire(excerpt: &QueryExcerpt) -> QueryExcerpt {
    let restored: QueryExcerpt =
        serde_json::from_str(&serde_json::to_string(excerpt).unwrap()).unwrap();
    assert_eq!(restored, *excerpt);
    restored
}

#[test]
fn exact_paths_keep_source_wide_navigation_rejections_through_json() {
    let mut cases = 0;
    for root in ["blocks", "heading", "alias"] {
        for collision in ["section", "entry", "nested-anchor"] {
            for placement in [
                "heading",
                "paragraph",
                "list",
                "definition",
                "table",
                "literal",
            ] {
                for validate in [false, true] {
                    let source = query(
                        document(root, collision, placement, false),
                        validate,
                        "probe",
                    );
                    let full = mant_codec::encode::render_addressable_markdown(&source);
                    assert_inactive(full.text());
                    for paths in [
                        &["root"][..],
                        &["1"],
                        &["1/e1"],
                        &["root", "1"],
                        &["root", "1/e1"],
                    ] {
                        let selectors = paths
                            .iter()
                            .map(|path| ContentSelector::path(*path))
                            .collect::<Vec<_>>();
                        let excerpt = mant_query::select_excerpt(&source, &selectors).unwrap();
                        assert!(excerpt.content_complete);
                        assert!(!excerpt.semantics_complete);
                        assert_eq!(
                            excerpt
                                .diagnostics
                                .iter()
                                .filter(|finding| {
                                    finding.code.as_deref() == Some(OVERVIEW_IDENTITY_COLLISION)
                                        && finding.impact == DiagnosticImpact::SemanticCoverage
                                })
                                .count(),
                            1
                        );
                        let restored = excerpt_wire(&excerpt);
                        let markdown =
                            mant_render::render_excerpt_markdown_with_options(&restored, OPTIONS);
                        assert_inactive(&markdown);
                        assert_eq!(
                            markdown.matches("ROOTBODY").count(),
                            usize::from(root == "heading")
                                + usize::from(root != "alias" && paths.contains(&"root")),
                            "{root}/{collision}/{placement}/{paths:?}: {markdown}"
                        );
                        if paths.last() != Some(&"root") {
                            assert_eq!(fragment_count(&markdown, "#allowed"), 1, "{markdown}");
                            for marker in [
                                "NESTEDBODY",
                                "ENTRYJUMP",
                                "BODYJUMP",
                                "Nested.Exact",
                                "Valid.Exact",
                            ] {
                                assert!(
                                    markdown.contains(marker),
                                    "{paths:?}/{placement}: {markdown}"
                                );
                            }
                            assert!(markdown.contains("**BODYJUMP**"), "{markdown}");
                            if paths.contains(&"1") {
                                assert!(markdown.contains("id=\"Section.Exact\""), "{markdown}");
                            }
                        }
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 540);
}

#[test]
fn ordinary_root_spelling_keeps_its_destination_without_an_overview() {
    for (collision, paths) in [
        ("section", ["1", "1/e1"]),
        ("entry", ["1", "1/e1"]),
        ("nested-anchor", ["1", "1/e1"]),
    ] {
        let source = query(
            document("absent", collision, "heading", false),
            true,
            "valid",
        );
        let document = source.document.as_ref().unwrap();
        assert_eq!(document.diagnostics, []);
        assert!(!document.has_overview());
        for path in paths {
            let excerpt =
                mant_query::select_excerpt(&source, &[ContentSelector::path(path)]).unwrap();
            let markdown =
                mant_render::render_excerpt_markdown_with_options(&excerpt_wire(&excerpt), OPTIONS);
            assert_ne!(
                fragment_count(&markdown, "#document-overview"),
                0,
                "{markdown}"
            );
            if path == "1" || collision != "section" {
                assert!(markdown.contains("id=\"document-overview\""), "{markdown}");
            }
        }
        // Heading links automatically enable destinations in the default export.
        let section = mant_query::select_excerpt(&source, &[ContentSelector::path("1")]).unwrap();
        let markdown = mant_render::render_excerpt_markdown(&section);
        assert_ne!(
            fragment_count(&markdown, "#document-overview"),
            0,
            "{markdown}"
        );
    }
}

fn explanation_query(grouped: bool) -> ExplanationQuery {
    ExplanationQuery {
        entry: if grouped { "PREFACE" } else { "TERM" }.into(),
        options: ExplanationOptions::default(),
    }
}

#[test]
fn explanation_and_declaration_support_keep_the_source_navigation_policy() {
    for grouped in [false, true] {
        let source = query(
            document("blocks", "entry", "list", grouped),
            false,
            "invalid",
        );
        let explanation = mant_query::explain_query(&source, &explanation_query(grouped)).unwrap();
        assert_eq!(explanation.counts.direct_entry.returned, 1);
        assert_eq!(!explanation.supports.is_empty(), grouped);
        assert!(!explanation.semantics_complete);
        let restored: QueryExplanation =
            serde_json::from_str(&serde_json::to_string(&explanation).unwrap()).unwrap();
        let markdown = mant_render::render_explanation_markdown_with_options(&restored, OPTIONS);
        assert_inactive(&markdown);
        assert!(markdown.contains("id=\"Entry.Exact\""), "{markdown}");
        assert!(markdown.contains("id=\"Nested.Exact\""), "{markdown}");
        assert!(markdown.contains("**BODYJUMP**"), "{markdown}");
        assert_eq!(fragment_count(&markdown, "#allowed"), 1, "{markdown}");
    }
}

fn scope_graph(sources: &[ResolvedContent]) -> ResolvedDocumentScope {
    ResolvedDocumentScope {
        query: DocumentScope {
            documents: sources
                .iter()
                .map(|source| DocumentSelector {
                    selector: source.label.as_str().into(),
                    source: None,
                    manual_section: None,
                })
                .collect(),
            traversal: mant_protocol::DocumentTraversal::default(),
        },
        documents: sources
            .iter()
            .enumerate()
            .map(|(index, source)| ScopedDocument {
                address: source.address.clone().unwrap(),
                depth: 0,
                root_indices: vec![u16::try_from(index).unwrap()],
                reached_from: vec![],
            })
            .collect(),
        edges: vec![],
        frontier: vec![],
        unresolved: vec![],
        reference_limits: vec![],
    }
}

#[test]
fn scoped_explanation_keeps_navigation_policy_per_source() {
    for grouped in [false, true] {
        let sources = [
            query(
                document("blocks", "entry", "list", grouped),
                true,
                "ambiguous",
            ),
            query(
                document("absent", "entry", "list", grouped),
                false,
                "unique",
            ),
        ];
        let graph = scope_graph(&sources);
        let request = explanation_query(grouped);
        let ordinary = mant_query::explain_query(&sources[1], &request).unwrap();
        let valid_markdown =
            mant_render::render_explanation_markdown_with_options(&ordinary, OPTIONS);
        let active = fragment_count(&valid_markdown, "#document-overview");
        assert_ne!(active, 0, "{valid_markdown}");
        let input = mant_query::QueryScopeView::new(&graph, &sources).unwrap();
        let scope = mant_query::explain_scope(input, &request).unwrap();
        assert_eq!(scope.documents.len(), 2);
        assert!(!scope.documents[0].semantics_complete);
        assert!(scope.documents[1].semantics_complete);
        assert_eq!(scope.counts.direct_entry.returned, 2);
        let restored: ScopeExplanation =
            serde_json::from_str(&serde_json::to_string(&scope).unwrap()).unwrap();
        let markdown =
            mant_render::render_scope_explanation_markdown_with_options(&restored, OPTIONS);
        assert_eq!(
            markdown.matches("id=\"document-overview\"").count(),
            1,
            "{markdown}"
        );
        assert_eq!(
            fragment_count(&markdown, "#document-overview"),
            active,
            "{markdown}"
        );
        assert_eq!(markdown.matches("BODYJUMP").count(), 2, "{markdown}");
    }
}
