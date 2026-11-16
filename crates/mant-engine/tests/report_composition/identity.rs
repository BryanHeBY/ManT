//! Validate external v0.14 snapshots with ordinary consumers after roundtrip.
use mant_ir::{
    Block, DOCUMENT_ROOT_ID, DiagnosticImpact, Document, DocumentAddress, DocumentIndex,
    DocumentReference, LinkTarget, MarkdownOrigin, ResolvedContent,
};
use mant_protocol::{
    ContentSelector, DocumentResponse, EntryProjection, LoadedFragment, ReferenceCoverageStatus,
    ReferenceProjection, ReferenceProjectionMode, ReferenceResolution, ReferenceTargetType,
};
use serde_json::{Value, json};

fn paragraph(children: Value) -> Value {
    let mut block = json!({"type": "paragraph"});
    block["children"] = children;
    block
}

fn text(value: &str) -> Value {
    json!({"type": "text", "value": value})
}

fn anchor(id: &str) -> Value {
    json!({"type": "anchor", "id": id})
}

fn add_link(target: &LinkTarget, label: &str) -> Block {
    serde_json::from_value(paragraph(json!([{
        "type": "link",
        "target": target,
        "children": [text(label)],
    }])))
    .unwrap()
}

fn snapshot(fields: Value) -> Document {
    let mut base =
        json!({"source": {"format": "markdown"}, "meta": {}, "blocks": [], "sections": []});
    let Value::Object(fields) = fields else {
        panic!("document fixture overrides")
    };
    base.as_object_mut().unwrap().extend(fields);
    let original: Document = serde_json::from_value(base).unwrap();
    let decoded: DocumentResponse =
        serde_json::from_str(&serde_json::to_string(&DocumentResponse::from(&original)).unwrap())
            .unwrap();
    let restored: Document = decoded.into();
    assert_eq!(original, restored, "frozen v0.14 roundtrip");
    restored
}

fn query(mut document: Document) -> ResolvedContent {
    document
        .diagnostics
        .extend(mant_ir::validate_document(&document));
    unresolved_source(document)
}

fn unresolved_source(document: Document) -> ResolvedContent {
    ResolvedContent {
        label: "external source".into(),
        address: Some(DocumentAddress::Markdown {
            path: "dir/source".into(),
            origin: MarkdownOrigin::Documents,
        }),
        document: Some(document),
        tldr: None,
    }
}

fn outline(query: &ResolvedContent, kind: ReferenceTargetType) -> mant_protocol::QueryOutline {
    mant_query::build_outline_with_references(
        query,
        EntryProjection::All,
        None,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![kind],
            ..ReferenceProjection::default()
        },
    )
    .unwrap()
}

fn index_collision_document(root: &str, collision: &str) -> Document {
    let mut fields = match root {
        "body" => json!({"blocks": [paragraph(json!([text("ROOTBODY")]))]}),
        "heading" => json!({"heading": {"content": [text("ROOTHEADING")]}}),
        "alias" => json!({"fragmentAliases": ["Root.Target"]}),
        _ => unreachable!(),
    };
    let link = add_link(
        &LinkTarget::Section {
            id: DOCUMENT_ROOT_ID.into(),
        },
        "ROOTLINK",
    );
    let mut blocks = vec![link];
    match collision {
        "section" => {}
        "entry" => blocks.push(serde_json::from_value(json!({
            "type": "definition-list",
            "items": [{
                "terms": [{"content": [text("TERM")]}],
                "description": [],
                "entry": {"id": DOCUMENT_ROOT_ID, "kind": {"kind": "term"}, "case": "sensitive", "names": []},
            }],
        })).unwrap()),
        "nested" => blocks.push(serde_json::from_value(json!({
            "type": "list",
            "kind": {"kind": "bullet"},
            "items": [{"blocks": [paragraph(json!([anchor(DOCUMENT_ROOT_ID), text("NESTED")]))]}],
        })).unwrap()),
        _ => unreachable!(),
    }
    let section_id = if collision == "section" {
        DOCUMENT_ROOT_ID
    } else {
        "section"
    };
    fields["sections"] = json!([{
        "id": section_id,
        "heading": {"content": [text("SECTION")]},
        "blocks": blocks,
        "children": [],
    }]);
    snapshot(fields)
}

#[test]
fn the_actual_overview_cannot_be_shadowed_by_an_unrelated_owner() {
    for root in ["body", "heading", "alias"] {
        for collision in ["section", "entry", "nested"] {
            let document = index_collision_document(root, collision);
            let checked = mant_ir::DocumentValidation::new(&document);
            assert!(
                checked.index().has_overview_conflict(),
                "{root}/{collision}"
            );
            assert_eq!(checked.index().fragment_target(DOCUMENT_ROOT_ID), None);
            assert!(checked.diagnostics().iter().any(|diagnostic| {
                diagnostic.code.as_deref() == Some(mant_ir::OVERVIEW_IDENTITY_COLLISION)
                    && diagnostic.impact == DiagnosticImpact::SemanticCoverage
            }));
            let query = query(document);
            let selector = ContentSelector::id(DOCUMENT_ROOT_ID);
            assert!(
                matches!(
                    mant_query::select_excerpt(&query, &[selector]),
                    Err(mant_query::ProjectionError::AmbiguousSelector { .. })
                ),
                "{root}/{collision}"
            );
            assert!(mant_query::select_excerpt(&query, &[ContentSelector::path("root")]).is_ok());
            let outline = outline(&query, ReferenceTargetType::Local);
            assert!(!outline.semantics_complete);
            assert!(outline.content_complete);
            assert!(matches!(
                outline.references.coverage.status,
                ReferenceCoverageStatus::Complete {}
            ));
            assert!(matches!(
                outline.references.records[0].resolution,
                ReferenceResolution::Loaded {
                    fragment: LoadedFragment::Ambiguous {},
                    ..
                }
            ));
            let markdown = mant_codec::encode::render_addressable_markdown(&query);
            assert!(!markdown.text().contains("id=\"document-overview\""));
            assert!(!markdown.text().contains("](#document-overview)"));
            assert!(markdown.text().contains("ROOTLINK"));
            assert!(markdown.text().contains("SECTION"));
        }
    }
}

#[test]
fn a_root_anchor_can_share_the_overview_and_a_rootless_section_keeps_its_id() {
    let target = LinkTarget::Section {
        id: DOCUMENT_ROOT_ID.into(),
    };
    for root in ["absent", "body", "heading"] {
        let mut fields = json!({"sections": [{
            "id": if root == "absent" {DOCUMENT_ROOT_ID} else {"section"},
            "heading": {"content": [text("SECTION")]},
            "blocks": [add_link(&target, "ROOTLINK")],
            "children": [],
        }]});
        let effective_root_anchor = json!({
            "type": "anchor", "id": DOCUMENT_ROOT_ID,
            "fragmentAliases": ["Inline.Valid", "Shared.Valid"],
        });
        if root == "body" {
            fields["blocks"] = json!([paragraph(json!([effective_root_anchor, text("ROOTBODY")]))]);
        }
        if root == "heading" {
            fields["heading"] = json!({"content": [effective_root_anchor, text("ROOTHEADING")]});
        }
        if root != "absent" {
            fields["fragmentAliases"] = json!(["Shared.Valid", "Shared.Valid"]);
        }
        let document = snapshot(fields);
        assert_eq!(mant_ir::validate_document(&document), []);
        assert_eq!(
            DocumentIndex::build(&document)
                .fragment_target(DOCUMENT_ROOT_ID)
                .unwrap(),
            DOCUMENT_ROOT_ID
        );
        let query = query(document);
        let outline = outline(&query, ReferenceTargetType::Local);
        assert!(matches!(
            outline.references.records[0].resolution,
            ReferenceResolution::Loaded {
                fragment: LoadedFragment::Valid { .. },
                ..
            }
        ));
        let artifact = mant_codec::encode::render_addressable_markdown(&query);
        assert_eq!(
            artifact.text().matches("id=\"document-overview\"").count(),
            1
        );
        assert!(artifact.text().contains("](#document-overview)"));
        if root != "absent" {
            for alias in ["Inline.Valid", "Shared.Valid"] {
                assert_eq!(
                    artifact.text().matches(&format!("id=\"{alias}\"")).count(),
                    1
                );
            }
        }
        let excerpt =
            mant_query::select_excerpt(&query, &[ContentSelector::id(DOCUMENT_ROOT_ID)]).unwrap();
        let markdown = mant_render::render_excerpt_markdown_with_options(
            &excerpt,
            mant_codec::encode::MarkdownFragmentOptions {
                preserve_anchors: true,
                ..mant_codec::encode::MarkdownFragmentOptions::default()
            },
        );
        assert_eq!(
            markdown.matches("id=\"document-overview\"").count(),
            1,
            "{root}: {markdown}"
        );
    }
}

#[test]
fn exact_aliases_still_reveal_distinct_source_sites_when_the_canonical_id_conflicts() {
    let alias_link = |alias: &str| {
        add_link(
            &LinkTarget::Document {
                name: "source".into(),
                fragment: Some(alias.into()),
            },
            alias,
        )
    };
    let document = snapshot(json!({
        "fragmentAliases": ["Root.Exact", "Shared.Exact"],
        "sections": [{
            "id": DOCUMENT_ROOT_ID,
            "fragmentAliases": ["Section.Exact", "Shared.Exact"],
            "heading": {"content": [text("SECTION")]},
            "blocks": [alias_link("Root.Exact"), alias_link("Section.Exact"), alias_link("Shared.Exact")],
            "children": [],
        }],
    }));
    let index = DocumentIndex::build(&document);
    for alias in ["Root.Exact", "Section.Exact", "Shared.Exact"] {
        assert_eq!(
            index.fragment_target(alias),
            None,
            "one ambiguous canonical ID"
        );
    }
    assert!(index.is_fragment_ambiguous("Shared.Exact"));
    assert!(!index.is_fragment_ambiguous("Root.Exact"));
    assert!(!index.is_fragment_ambiguous("Section.Exact"));
    let query = query(document);
    assert!(
        query
            .document
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .any(|finding| {
                finding.code.as_deref() == Some("ir.ambiguous-fragment-alias")
                    && finding.impact == DiagnosticImpact::SemanticCoverage
            })
    );
    let inventory = outline(&query, ReferenceTargetType::Document).references;
    assert_eq!(inventory.records.len(), 3);
    for (record, expected) in inventory.records.iter().zip([
        mant_ir::ContentReveal::Document {},
        mant_ir::ContentReveal::Section { sections: vec![0] },
    ]) {
        assert!(
            matches!(&record.resolution, ReferenceResolution::Loaded {
                fragment: LoadedFragment::Valid { reveal },
                ..
            } if reveal == &expected),
            "{record:?}",
        );
    }
    assert!(matches!(
        inventory.records[2].resolution,
        ReferenceResolution::Loaded {
            fragment: LoadedFragment::Ambiguous {},
            ..
        }
    ));
    let markdown = mant_codec::encode::render_addressable_markdown(&query);
    for alias in ["Root.Exact", "Section.Exact"] {
        assert!(markdown.text().contains(&format!("id=\"{alias}\"")));
    }
    assert!(!markdown.text().contains("id=\"document-overview\""));
    assert!(!markdown.text().contains("id=\"Shared.Exact\""));
    assert!(markdown.text().contains("[Shared.Exact]"));
}

#[test]
fn reading_a_conflicting_root_by_path_preserves_its_inert_link() {
    let source = add_link(
        &LinkTarget::Section {
            id: DOCUMENT_ROOT_ID.into(),
        },
        "ROOTJUMP",
    );
    let document = snapshot(json!({
        "blocks": [source],
        "sections": [{
            "id": DOCUMENT_ROOT_ID,
            "heading": {"content": [text("OTHER")]},
            "blocks": [],
            "children": [],
        }],
    }));
    assert_eq!(document.diagnostics, []);
    for query in [unresolved_source(document.clone()), query(document)] {
        let excerpt = mant_query::select_excerpt(&query, &[ContentSelector::path("root")]).unwrap();
        assert!(excerpt.content_complete);
        assert!(!excerpt.semantics_complete);
        assert_eq!(
            excerpt
                .diagnostics
                .iter()
                .filter(|finding| {
                    finding.code.as_deref() == Some(mant_ir::OVERVIEW_IDENTITY_COLLISION)
                        && finding.impact == DiagnosticImpact::SemanticCoverage
                })
                .count(),
            1
        );
        let excerpt: mant_protocol::QueryExcerpt =
            serde_json::from_str(&serde_json::to_string(&excerpt).unwrap()).unwrap();
        let markdown = mant_render::render_excerpt_markdown_with_options(
            &excerpt,
            mant_codec::encode::MarkdownFragmentOptions {
                preserve_anchors: true,
                ..mant_codec::encode::MarkdownFragmentOptions::default()
            },
        );
        assert!(markdown.contains("ROOTJUMP"));
        assert!(!markdown.contains("](#document-overview)"));
        assert!(!markdown.contains("id=\"document-overview\""));
    }
}

#[test]
fn malformed_document_link_facts_remain_visible_but_cannot_resolve() {
    for (name, fragment, valid) in [
        ("", None, false),
        ("/absolute", None, false),
        ("page?query", None, false),
        ("a#part", None, false),
        ("a\\leaf", None, false),
        ("a//leaf", None, false),
        ("scheme:name", None, false),
        ("a\nleaf", None, false),
        ("page", Some(""), false),
        ("page", Some("line\nbreak"), false),
        ("page", None, true),
        ("../peer", None, true),
        ("pages/章节", Some("一 二"), true),
        ("page", Some(" "), true),
        ("pages/%2F", Some("a# b"), true),
    ] {
        let target = LinkTarget::Document {
            name: name.into(),
            fragment: fragment.map(str::to_owned),
        };
        let document = snapshot(json!({"blocks": [add_link(&target, "LINKLABEL")]}));
        let diagnostics = mant_ir::validate_document(&document);
        if valid {
            assert_eq!(diagnostics, [], "{target:?}");
        } else {
            let [diagnostic] = diagnostics.as_slice() else {
                panic!("{target:?}: one invalid-link diagnostic: {diagnostics:?}")
            };
            assert_eq!(
                diagnostic.code.as_deref(),
                Some("ir.invalid-document-reference")
            );
            assert_eq!(diagnostic.impact, DiagnosticImpact::SemanticCoverage);
        }
        let uri = target.to_uri();
        assert_eq!(uri.is_some(), valid, "{target:?}");
        if let Some(uri) = &uri {
            assert_eq!(&LinkTarget::from_uri(uri), &target);
            if name == "pages/%2F" {
                assert_eq!(uri, "pages/%252F.md#a%23%20b");
            }
        }
        assert_eq!(
            DocumentReference::from_link_target(&target)
                .unwrap()
                .is_well_formed(),
            valid
        );
        let query = query(document);
        let outline = outline(&query, ReferenceTargetType::Document);
        assert_eq!(outline.semantics_complete, valid);
        assert!(outline.content_complete);
        assert!(matches!(
            outline.references.coverage.status,
            ReferenceCoverageStatus::Complete {}
        ));
        let record = &outline.references.records[0];
        assert_eq!(record.target, target);
        assert_eq!(record.label, "LINKLABEL");
        assert_eq!(
            matches!(
                record.resolution,
                ReferenceResolution::LogicalAddress { .. }
            ),
            valid
        );
        assert_eq!(
            matches!(record.resolution, ReferenceResolution::Restricted {}),
            !valid
        );

        let markdown = mant_codec::encode::render_markdown(&query);
        assert_eq!(markdown.matches("LINKLABEL").count(), 1);
        if let Some(uri) = uri {
            assert!(
                markdown.contains(&format!("[LINKLABEL]({uri})")),
                "{markdown}"
            );
        } else {
            assert!(!markdown.contains("[LINKLABEL]("), "{markdown}");
        }
    }
}
