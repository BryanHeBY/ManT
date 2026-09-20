//! Exercise nested public decoders, not just schema additionalProperties flags.
use mant_protocol::{
    DocumentResponse, OutlineNode, OutlineNodeReference, QueryBundle, QueryExcerpt,
};
use serde_json::{Value, json};

fn definition() -> Value {
    json!({"entry":{"id":"command-example","kind":{"kind":"command"},
        "case":"sensitive","names":[],"forms":[]},"terms":[],"description":[]})
}

fn document(item: &Value) -> Value {
    json!({"schema":"mant.document/v0.12","producer":{"name":"test","version":"0"},
        "sources":[{"key":1,"identity":{"kind":"anonymous","name":"test"},"format":"markdown","decodedByteLength":0,"coordinates":{"kind":"decoded-utf8-bytes"}}],
        "rootSource":1,"meta":{},"sections":[],
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
