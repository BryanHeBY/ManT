//! Exercise nested public decoders, not just schema additionalProperties flags.
use mant_protocol::{
    DocumentResponse, OutlineNode, OutlineNodeReference, QueryBundle, QueryExcerpt,
};
use serde_json::{Value, json};

fn definition() -> Value {
    json!({"entry":{"id":"command-example","kind":{"kind":"command"},
        "case":"sensitive","names":[],"forms":[]},"terms":[],"description":[]})
}

fn empty_content_store() -> Value {
    json!({"owners":[],"roots":[],"atoms":[],"points":[],"links":[]})
}

#[test]
fn wire_document_rejects_partial_or_uncovered_atoms() {
    use mant_ir::{
        Block, ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, Inline,
        LayoutHint, Provenance,
    };

    let mut builder = ContentStoreBuilder::new();
    let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
    let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
    let first = builder.push_text(
        root,
        "hello".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let mut payload = document(&definition());
    payload["contentStore"] = serde_json::to_value(builder.finish()).unwrap();
    payload["blocks"] = serde_json::to_value([Block::Paragraph {
        children: vec![Inline::Text { content: first }],
        layout: LayoutHint::default(),
        source: None,
    }])
    .unwrap();
    let _: DocumentResponse = serde_json::from_value(payload.clone()).unwrap();
    let mut hidden = payload.clone();
    hidden["blocks"] = json!([]);
    assert!(serde_json::from_value::<DocumentResponse>(hidden).is_err());
    let mut duplicated = payload.clone();
    let leaf = duplicated["blocks"][0]["children"][0].clone();
    duplicated["blocks"][0]["children"]
        .as_array_mut()
        .unwrap()
        .push(leaf);
    assert!(serde_json::from_value::<DocumentResponse>(duplicated).is_err());
    payload["blocks"][0]["children"][0]["content"]["bytes"]["end"] = json!(2);
    assert!(serde_json::from_value::<DocumentResponse>(payload).is_err());
}

#[test]
fn wire_document_rejects_nested_link_wrappers() {
    use mant_ir::{
        Block, ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, Inline,
        LayoutHint, LinkTarget, Provenance,
    };
    let mut builder = ContentStoreBuilder::new();
    let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
    let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
    let outer_text = builder.push_text(
        root,
        "a".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let inner_text = builder.push_text(
        root,
        "b".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let outer = builder
        .push_link_for_atoms(
            &[outer_text.atom],
            LinkTarget::External {
                uri: "https://example.invalid/a".into(),
            },
            None,
            Provenance::Unknown,
        )
        .unwrap();
    let inner = builder
        .push_link_for_atoms(
            &[inner_text.atom],
            LinkTarget::External {
                uri: "https://example.invalid/b".into(),
            },
            None,
            Provenance::Unknown,
        )
        .unwrap();
    let mut payload = document(&definition());
    payload["contentStore"] = serde_json::to_value(builder.finish()).unwrap();
    payload["blocks"] = serde_json::to_value([Block::Paragraph {
        children: vec![
            Inline::Link {
                occurrence: outer,
                children: vec![Inline::Text {
                    content: outer_text,
                }],
            },
            Inline::Link {
                occurrence: inner,
                children: vec![Inline::Text {
                    content: inner_text,
                }],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }])
    .unwrap();
    let _: DocumentResponse = serde_json::from_value(payload.clone()).unwrap();
    let inner_wrapper = payload["blocks"][0]["children"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    payload["blocks"][0]["children"][0]["children"]
        .as_array_mut()
        .unwrap()
        .push(inner_wrapper);
    assert!(serde_json::from_value::<DocumentResponse>(payload).is_err());
}

#[test]
fn complete_wire_document_rejects_root_atoms_reordered_across_paragraphs() {
    use mant_ir::{
        Block, ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, Inline,
        LayoutHint, Provenance,
    };

    let mut builder = ContentStoreBuilder::new();
    let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
    let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
    let first = builder.push_text(
        root,
        "first".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let second = builder.push_text(
        root,
        "second".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let mut payload = document(&definition());
    payload["contentStore"] = serde_json::to_value(builder.finish()).unwrap();
    payload["blocks"] = serde_json::to_value([first, second].map(|content| Block::Paragraph {
        children: vec![Inline::Text { content }],
        layout: LayoutHint::default(),
        source: None,
    }))
    .unwrap();
    let _: DocumentResponse = serde_json::from_value(payload.clone()).unwrap();
    payload["blocks"].as_array_mut().unwrap().swap(0, 1);
    assert!(serde_json::from_value::<DocumentResponse>(payload).is_err());
}

fn document(item: &Value) -> Value {
    json!({"schema":"mant.document/v0.12","producer":{"name":"test","version":"0"},
        "sources":[{"key":1,"identity":{"kind":"anonymous","name":"test"},"format":"markdown","decodedByteLength":0,"coordinates":{"kind":"decoded-utf8-bytes"}}],
        "rootSource":1,"contentStore":empty_content_store(),"meta":{},"sections":[],
        "blocks":[{"type":"definition-list","items":[item]}]})
}

#[test]
fn nested_diagnostic_impact_is_required_at_document_and_query_boundaries() {
    for impact in [
        None,
        Some("none"),
        Some("semantic-coverage"),
        Some("unknown"),
    ] {
        let mut payload = document(&definition());
        let mut diagnostic = json!({"level":"warning","message":"custom producer finding"});
        if let Some(impact) = impact {
            diagnostic["impact"] = json!(impact);
        }
        payload["diagnostics"] = json!([diagnostic]);
        let valid = matches!(impact, Some("none" | "semantic-coverage"));
        assert_eq!(
            serde_json::from_value::<DocumentResponse>(payload.clone()).is_ok(),
            valid
        );
        let query = json!({"schema":"mant.query/v0.12","label":"test","document":payload});
        assert_eq!(serde_json::from_value::<QueryBundle>(query).is_ok(), valid);
    }
}

#[test]
fn document_and_query_envelopes_reject_legacy_nested_facts() {
    for legacy in [
        None,
        Some("identity"),
        Some("inlineTerm"),
        Some("spacingBeforeLines"),
        Some("unknown"),
    ] {
        let mut item = definition();
        if let Some(field) = legacy {
            item[field] = Value::Null;
        }
        let payload = document(&item);
        assert_eq!(
            serde_json::from_value::<DocumentResponse>(payload.clone()).is_ok(),
            legacy.is_none()
        );
        let query = json!({"schema":"mant.query/v0.12","label":"test","document":payload});
        assert_eq!(
            serde_json::from_value::<QueryBundle>(query).is_ok(),
            legacy.is_none()
        );
    }
    for field in ["role", "aliases", "unknown"] {
        let mut item = definition();
        item["entry"][field] = Value::Null;
        assert!(serde_json::from_value::<DocumentResponse>(document(&item)).is_err());
    }
}

#[test]
fn source_context_rejects_unknown_and_out_of_range_nested_spans() {
    let mut unknown = document(&definition());
    unknown["blocks"][0]["source"] = json!({"source":2,"line":1,"column":1});
    assert!(serde_json::from_value::<DocumentResponse>(unknown).is_err());

    let mut out_of_range = document(&definition());
    out_of_range["blocks"][0]["source"] =
        json!({"source":1,"byteRange":{"start":0,"end":1},"line":1,"column":1});
    assert!(serde_json::from_value::<DocumentResponse>(out_of_range).is_err());

    let excerpt = json!({
        "schema":"mant.excerpt/v0.12",
        "label":"test",
        "sourceContext":{
            "sources":[{
                "key":1,
                "identity":{"kind":"anonymous","name":"test"},
                "format":"markdown",
                "decodedByteLength":0,
                "coordinates":{"kind":"decoded-utf8-bytes"}
            }],
            "rootSource":1
        },
        "selections":[{
            "kind":"document-root",
            "outline":{"node":{"kind":"document-root","path":"root","id":"root","title":"root"}},
            "blocks":[{
                "type":"paragraph",
                "children":[],
                "source":{"source":2,"line":1,"column":1}
            }]
        }]
    });
    assert!(serde_json::from_value::<QueryExcerpt>(excerpt).is_err());
}

#[test]
fn outline_names_and_entry_kind_are_closed_at_both_projection_levels() {
    let node = json!({"kind":"document-entry","path":"root/e1","id":"example",
        "title":"example","entryKind":{"kind":"command"},"case":"sensitive", "names":["example"]});
    let mut full_node = node.clone();
    full_node["forms"] = json!([]);
    full_node["owner"] =
        json!({"kind":"owner","sections":[],"blocks":[{"kind":"block","index":0}],"itemIndex":0});
    let _: OutlineNode = serde_json::from_value(full_node.clone()).unwrap();
    assert!(serde_json::from_value::<OutlineNodeReference>(node.clone()).is_ok());
    for field in ["role", "aliases", "unknown"] {
        let mut invalid = node.clone();
        invalid[field] = Value::Null;
        let mut full_invalid = full_node.clone();
        full_invalid[field] = Value::Null;
        assert!(serde_json::from_value::<OutlineNode>(full_invalid).is_err());
        assert!(serde_json::from_value::<OutlineNodeReference>(invalid).is_err());
    }
    let raw = serde_json::to_string(&node).unwrap();
    let duplicate = raw.replacen('{', "{\"names\":[],", 1);
    let raw_full = serde_json::to_string(&full_node).unwrap();
    let duplicate_full = raw_full.replacen('{', "{\"names\":[],", 1);
    assert!(serde_json::from_str::<OutlineNode>(&duplicate_full).is_err());
    assert!(serde_json::from_str::<OutlineNodeReference>(&duplicate).is_err());
    let mut excerpt = json!({"schema":"mant.excerpt/v0.12","label":"test",
        "contentProjection":{"contentStore":empty_content_store()},
        "selections":[{"kind":"document-entry","outline":{"node":node},
            "entry":{"type":"definition-list","items":[definition()]}}]});
    assert!(serde_json::from_value::<QueryExcerpt>(excerpt.clone()).is_ok());
    excerpt["selections"][0]["entry"]["items"][0]["identity"] = Value::Null;
    assert!(serde_json::from_value::<QueryExcerpt>(excerpt).is_err());
}

#[test]
fn query_list_kinds_reject_legacy_or_inapplicable_start() {
    for kind in [
        json!({"kind":"bullet"}),
        json!({"kind":"plain"}),
        json!({"kind":"ordered"}),
        json!({"kind":"ordered","start":0}),
    ] {
        let mut payload = document(&definition());
        payload["blocks"] = json!([{"type":"list","kind":kind,"items":[]}]);
        let _: DocumentResponse = serde_json::from_value(payload.clone()).unwrap();
        payload["blocks"][0]["start"] = Value::Null;
        assert!(serde_json::from_value::<DocumentResponse>(payload).is_err());
    }
    let mut payload = document(&definition());
    payload["blocks"] = json!([{"type":"list","kind":{"kind":"bullet","start":null},"items":[]}]);
    assert!(
        serde_json::from_value::<QueryBundle>(
            json!({"schema":"mant.query/v0.12","label":"test","document":payload})
        )
        .is_err()
    );
}
