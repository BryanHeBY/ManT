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
    json!({"schema":"mant.document/v0.11","producer":{"name":"test","version":"0"},
        "source":{"format":"markdown"},"meta":{},"sections":[],
        "blocks":[{"type":"definition-list","items":[item]}]})
}

#[test]
fn document_and_query_envelopes_reject_legacy_nested_facts() {
    for legacy in [None, Some("identity"), Some("unknown")] {
        let mut item = definition();
        if let Some(field) = legacy {
            item[field] = Value::Null;
        }
        let payload = document(&item);
        assert_eq!(
            serde_json::from_value::<DocumentResponse>(payload.clone()).is_ok(),
            legacy.is_none()
        );
        let query = json!({"schema":"mant.query/v0.11","label":"test","document":payload});
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
fn outline_names_and_entry_kind_are_closed_at_both_projection_levels() {
    let node = json!({"kind":"document-entry","path":"root/e1","id":"example",
        "title":"example","entryKind":{"kind":"command"},"case":"sensitive", "names":["example"]});
    let mut full_node = node.clone();
    full_node["forms"] = json!([]);
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
    let mut excerpt = json!({"schema":"mant.excerpt/v0.11","label":"test",
        "selections":[{"kind":"document-entry","outline":{"node":node},
            "entry":{"type":"definition-list","items":[definition()]}}]});
    assert!(serde_json::from_value::<QueryExcerpt>(excerpt.clone()).is_ok());
    excerpt["selections"][0]["entry"]["items"][0]["identity"] = Value::Null;
    assert!(serde_json::from_value::<QueryExcerpt>(excerpt).is_err());
}
