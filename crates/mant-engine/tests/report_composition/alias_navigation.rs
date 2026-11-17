//! Source-wide alias conflicts survive projection and detached wire round trips.

use super::detached_navigation::{OPTIONS, excerpt_wire, fragment_count, query, scope_graph};
use mant_codec::encode::{ScopedMarkdownFragments, render_addressable_markdown};
use mant_ir::{
    AMBIGUOUS_FRAGMENT_ALIAS, DOCUMENT_ROOT_ID, DiagnosticImpact, Document, DocumentIndex,
    EntryForm, ResolvedContent,
};
use mant_protocol::{ContentSelector, ExplanationOptions, ExplanationQuery, QueryExplanation};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
enum AliasOwners {
    RootAndSection,
    SectionAndSection,
    Aliases,
    Entry,
    Anchor,
    Unique,
}

fn text(value: &str) -> Value {
    json!({"type": "text", "value": value})
}

fn jump(id: &str, label: &str) -> Value {
    json!({"type": "link", "target": {"kind": "section", "id": id}, "children": [text(label)]})
}

fn paragraph(children: Vec<Value>) -> Value {
    let mut paragraph = json!({"type": "paragraph"});
    paragraph["children"] = Value::Array(children);
    paragraph
}

fn placed_jumps(placement: &str) -> Value {
    let body = paragraph(vec![
        json!({"type": "anchor", "id": "inline", "fragmentAliases": ["Inline.Exact"]}),
        jump("shared", "NESTEDJUMP"),
        text(" "),
        jump("valid", "NESTEDVALID"),
    ]);
    match placement {
        "list" => json!({
            "type": "list", "kind": {"kind": "bullet"},
            "items": [{"blocks": [body]}]
        }),
        "table" => json!({
            "type": "table", "rows": [{"cells": [{"blocks": [body]}]}]
        }),
        "paragraph" => body,
        _ => unreachable!(),
    }
}

fn add_overview(doc: &mut Value, root: &str) {
    let root_content = vec![
        json!({"type": "anchor", "id": DOCUMENT_ROOT_ID, "fragmentAliases": ["Root.Exact"]}),
        text("OVERVIEW "),
        jump("shared", "ROOTJUMP"),
        text(" "),
        jump("valid", "ROOTVALID"),
    ];
    match root {
        "heading" => doc["heading"] = json!({"content": root_content}),
        "blocks" => doc["blocks"] = json!([paragraph(root_content)]),
        "absent" => {}
        _ => unreachable!(),
    }
}

fn fixture(root: &str, owners: AliasOwners, placement: &str) -> Document {
    let entry_id = if matches!(owners, AliasOwners::Entry) {
        "shared"
    } else {
        "term-entry"
    };
    let section_id = if matches!(
        owners,
        AliasOwners::RootAndSection | AliasOwners::SectionAndSection | AliasOwners::Unique
    ) {
        "shared"
    } else {
        "other"
    };
    let mut doc = json!({
        "source": {"format": "markdown", "path": "/test/private/source.md"},
        "meta": {},
        "sections": [{
            "id": "read",
            "heading": {"content": [
                jump("shared", "HEADINGJUMP"), text(" "), jump("valid", "VALIDHEADING"), text(" "),
                {"type": "link", "target": {"kind": "external", "uri": "https://example.test"},
                 "children": [text("EXTERNAL")]}, text(" "),
                {"type": "link", "target": {"kind": "manual", "name": "printf", "manualSection": "1"},
                 "children": [text("MANUAL")]}
            ]},
            "blocks": [{
                "type": "definition-list",
                "items": [{
                    "terms": [{"content": [text("PREFACE")]}],
                    "entry": {
                        "id": "preface-entry", "kind": {"kind": "term"}, "case": "sensitive",
                        "names": [], "forms": [EntryForm::term(0)]
                    },
                    "description": []
                }, {
                    "terms": [{"content": [
                        text("TERM "), jump("shared", "TERMJUMP"), text(" "), jump("valid", "TERMVALID")
                    ]}],
                    "entry": {
                        "id": entry_id, "kind": {"kind": "term"}, "case": "sensitive",
                        "names": [], "forms": [EntryForm::term(0)]
                    },
                    "description": [
                        paragraph(vec![
                            json!({"type": "anchor", "id": "valid", "fragmentAliases": ["Valid.Exact"]}),
                            text("PROVIDER "), jump("shared", "BODYJUMP"), text(" "),
                            jump("valid", "BODYVALID")
                        ]),
                        placed_jumps(placement)
                    ]
                }],
                "declarationGroups": [{"startItem": 0, "endItem": 2}]
            }],
            "children": []
        }, {
            "id": section_id, "heading": {"content": [text("OTHER OWNER")]},
            "blocks": [], "children": []
        }]
    });
    add_overview(&mut doc, root);
    match owners {
        AliasOwners::RootAndSection => doc["fragmentAliases"] = json!(["shared", "Root.Exact"]),
        AliasOwners::SectionAndSection => doc["sections"][0]["fragmentAliases"] = json!(["shared"]),
        AliasOwners::Aliases => {
            doc["sections"][0]["fragmentAliases"] = json!(["shared"]);
            doc["sections"][1]["fragmentAliases"] = json!(["shared"]);
        }
        AliasOwners::Entry => doc["sections"][1]["fragmentAliases"] = json!(["shared"]),
        AliasOwners::Anchor => {
            doc["sections"][1]["fragmentAliases"] = json!(["shared"]);
            doc["sections"][0]["blocks"]
                .as_array_mut()
                .unwrap()
                .push(paragraph(vec![json!({"type": "anchor", "id": "shared"})]));
        }
        AliasOwners::Unique => {
            if root == "absent" {
                doc["sections"][0]["id"] = json!(DOCUMENT_ROOT_ID);
            } else {
                doc["fragmentAliases"] = json!(["Root.Exact"]);
            }
        }
    }
    serde_json::from_value(doc).expect("custom IR fixture")
}

fn suppressed(markdown: &str, id: &str) {
    assert_eq!(fragment_count(markdown, &format!("#{id}")), 0, "{markdown}");
    assert!(!markdown.contains(&format!("id=\"{id}\"")), "{markdown}");
}

fn exact_selection(source: &ResolvedContent, selectors: &[ContentSelector]) -> String {
    mant_query::with_selected_excerpt(source, selectors, |excerpt, snapshot| {
        let snapshot = snapshot.expect("the selection has an original document");
        assert!(std::ptr::eq(
            snapshot.document(),
            source.document.as_ref().unwrap()
        ));
        mant_render::render_selected_excerpt_markdown(
            excerpt,
            ScopedMarkdownFragments::for_snapshot(OPTIONS, snapshot),
        )
    })
    .unwrap()
}

#[test]
fn aliases_keep_their_original_ownership_across_full_and_selected_markdown() {
    for root in ["heading", "blocks", "absent"] {
        for owners in [
            AliasOwners::RootAndSection,
            AliasOwners::SectionAndSection,
            AliasOwners::Aliases,
            AliasOwners::Entry,
            AliasOwners::Anchor,
        ] {
            if root == "absent" && matches!(owners, AliasOwners::RootAndSection) {
                continue;
            }
            for (placement, validated) in [("list", false), ("table", true), ("paragraph", true)] {
                let source = query(fixture(root, owners, placement), validated, "alias");
                let document = source.document.as_ref().unwrap();
                assert!(DocumentIndex::build(document).is_fragment_ambiguous("shared"));
                let full = render_addressable_markdown(&source);
                suppressed(full.text(), "shared");
                assert_ne!(
                    fragment_count(full.text(), "#valid"),
                    0,
                    "{root}/{owners:?}"
                );
                assert!(full.text().contains("id=\"Valid.Exact\""));

                let mut selections = vec![vec!["1"], vec!["1/e2"]];
                if document.has_overview() {
                    selections.extend([vec!["root"], vec!["root", "1/e2"]]);
                }
                for paths in selections {
                    let selectors: Vec<_> =
                        paths.iter().map(|p| ContentSelector::path(*p)).collect();
                    let excerpt = mant_query::select_excerpt(&source, &selectors).unwrap();
                    assert!(excerpt.content_complete);
                    assert!(!excerpt.semantics_complete);
                    assert!(excerpt.diagnostics.iter().any(|finding| {
                        finding.code.as_deref() == Some(AMBIGUOUS_FRAGMENT_ALIAS)
                            && finding.impact == DiagnosticImpact::SemanticCoverage
                    }));
                    assert_eq!(
                        excerpt
                            .selections
                            .iter()
                            .map(|s| s.outline().path())
                            .collect::<Vec<_>>(),
                        paths,
                    );
                    let detached = mant_render::render_excerpt_markdown_with_options(
                        &excerpt_wire(&excerpt),
                        OPTIONS,
                    );
                    let exact = exact_selection(&source, &selectors);
                    for markdown in [&detached, &exact] {
                        suppressed(markdown, "shared");
                        if paths.iter().any(|path| matches!(*path, "1" | "1/e2")) {
                            assert!(markdown.contains("PROVIDER"), "{markdown}");
                            assert!(markdown.contains("TERMJUMP"), "{markdown}");
                            assert!(markdown.contains("NESTEDJUMP"), "{markdown}");
                        }
                        if paths.contains(&"1") {
                            assert_eq!(fragment_count(markdown, "https://example.test"), 1);
                            assert_eq!(fragment_count(markdown, "man:printf(1)"), 1);
                        }
                    }
                    suppressed(&detached, "valid");
                    suppressed(&detached, "Valid.Exact");
                    assert_ne!(fragment_count(&exact, "#valid"), 0, "{paths:?}: {exact}");
                    if paths.contains(&"root") {
                        assert_eq!(exact.matches("id=\"Root.Exact\"").count(), 1, "{exact}");
                    }
                }
                if matches!(
                    owners,
                    AliasOwners::RootAndSection | AliasOwners::SectionAndSection
                ) {
                    // Addressing a canonical owner for reading does not settle an authored jump.
                    let selected =
                        mant_query::select_excerpt(&source, &[ContentSelector::id("shared")])
                            .expect("canonical owner is still selectable by ID");
                    assert_eq!(selected.selections[0].outline().path(), "2");
                    let exact = exact_selection(&source, &[ContentSelector::id("shared")]);
                    suppressed(&exact, "shared");
                    assert!(exact.contains("OTHER OWNER"));
                }
            }
        }
    }
}

#[test]
fn recognized_alias_codes_control_detached_links_without_message_or_source_lookup() {
    let source = query(
        fixture("absent", AliasOwners::SectionAndSection, "list"),
        true,
        "same",
    );
    let clean = query(fixture("absent", AliasOwners::Unique, "list"), true, "same");
    let selectors = [ContentSelector::path("1")];
    let rendered = exact_selection(&source, &selectors);
    suppressed(&rendered, "shared");
    let clean_rendered = exact_selection(&clean, &selectors);
    assert_ne!(fragment_count(&clean_rendered, "#shared"), 0);
    assert!(clean_rendered.contains("id=\"document-overview\""));
    assert!(
        render_addressable_markdown(&clean)
            .text()
            .contains("id=\"document-overview\"")
    );

    let mut excerpt = mant_query::select_excerpt(&source, &selectors).unwrap();
    let expected = mant_render::render_excerpt_markdown_with_options(&excerpt, OPTIONS);
    for message in ["irrelevant diagnostic", ""] {
        let finding = excerpt
            .diagnostics
            .iter_mut()
            .find(|finding| finding.code.as_deref() == Some(AMBIGUOUS_FRAGMENT_ALIAS))
            .unwrap();
        finding.message = message.into();
        finding.source = None;
        let detached =
            mant_render::render_excerpt_markdown_with_options(&excerpt_wire(&excerpt), OPTIONS);
        assert_eq!(detached, expected);
        suppressed(&detached, "shared");
        suppressed(&detached, "valid");
    }
}

#[test]
fn detached_evidence_and_support_do_not_reactivate_a_fragment_or_affect_another_source() {
    let sources = [
        query(
            fixture("absent", AliasOwners::SectionAndSection, "list"),
            false,
            "conflicting",
        ),
        query(
            fixture("absent", AliasOwners::Unique, "list"),
            true,
            "unique",
        ),
    ];
    let request = ExplanationQuery {
        entry: "PREFACE".into(),
        options: ExplanationOptions::default(),
    };
    let mut direct = Vec::new();
    for source in &sources {
        let report = mant_query::explain_query(source, &request).unwrap();
        assert_eq!(report.counts.direct_entry.returned, 1);
        assert_eq!(report.supports.len(), 1);
        let restored: QueryExplanation =
            serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(restored, report);
        let markdown = mant_render::render_explanation_markdown_with_options(&restored, OPTIONS);
        assert!(markdown.contains("PREFACE"), "{markdown}");
        assert!(markdown.contains("PROVIDER"), "{markdown}");
        assert!(markdown.contains("BODYJUMP"), "{markdown}");
        direct.push(markdown);
    }
    for id in ["shared", "valid", "Inline.Exact", "Valid.Exact"] {
        suppressed(&direct[0], id);
    }
    assert_ne!(fragment_count(&direct[1], "#shared"), 0);
    assert_ne!(fragment_count(&direct[1], "#valid"), 0);

    let graph = scope_graph(&sources);
    let report = mant_query::explain_scope(
        mant_query::QueryScopeView::new(&graph, &sources).unwrap(),
        &request,
    )
    .unwrap();
    let restored: mant_protocol::ScopeExplanation =
        serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
    assert_eq!(restored, report);
    assert!(!restored.documents[0].semantics_complete);
    assert!(restored.documents[1].semantics_complete);
    let markdown = mant_render::render_scope_explanation_markdown_with_options(&restored, OPTIONS);
    for target in ["#shared", "#valid"] {
        assert_eq!(
            fragment_count(&markdown, target),
            fragment_count(&direct[1], target)
        );
    }
    assert_eq!(markdown.matches("PROVIDER").count(), 2, "{markdown}");
    assert_eq!(
        markdown.matches("id=\"Inline.Exact\"").count(),
        1,
        "{markdown}"
    );
}
