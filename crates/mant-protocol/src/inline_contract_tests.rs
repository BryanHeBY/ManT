//! One accepted inline body in the unreleased v0.12 contract.

use crate::QueryBundle;
use mant_ir::Inline;
use serde_json::json;

#[test]
fn retired_display_shapes_are_rejected_at_every_inline_depth() {
    for invalid in [
        json!({"type":"portable-display","display":"alias","children":[]}),
        json!({"type":"text","value":"accepted","display":"alias"}),
        json!({"type":"strong","display":"alias","children":[]}),
        json!({"type":"link","target":{"kind":"external","uri":"https://ex.org"},
               "display":"alias","children":[]}),
    ] {
        assert!(serde_json::from_str::<Inline>(&invalid.to_string()).is_err());
        for wrap in [
            invalid.clone(),
            json!({"type":"strong","children":[invalid.clone()]}),
            json!({"type":"link","target":{"kind":"external","uri":"https://ex.org"},
                   "children":[invalid.clone()]}),
        ] {
            let mut query: serde_json::Value = query_fixture();
            query["document"]["sections"][0]["blocks"][0]["children"] = json!([wrap]);
            assert!(serde_json::from_str::<QueryBundle>(&query.to_string()).is_err());
        }
    }
}

#[test]
fn accepted_styles_links_and_rows_round_trip_without_a_second_spelling() {
    let body = json!([
        {"type":"strong","children":[{"type":"text","value":"-alphaBSD"}]},
        {"type":"line-break","indentColumns":6},
        {"type":"link","target":{"kind":"external","uri":"https://ex.org"},
         "children":[{"type":"emphasis","children":[{"type":"text","value":"label"}]}]},
        {"type":"text","value":": https://ex.org"}
    ]);
    let mut value: serde_json::Value = query_fixture();
    value["document"]["sections"][0]["blocks"][0]["children"] = body.clone();
    let query: QueryBundle = serde_json::from_str(&value.to_string()).unwrap();
    let wire = serde_json::to_string(&query).unwrap();
    let restored: QueryBundle = serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), value);
    let children: Vec<Inline> = serde_json::from_str(&body.to_string()).unwrap();
    assert_eq!(
        mant_ir::inline_plain_text(&children),
        "-alphaBSD\nlabel: https://ex.org"
    );
    assert!(!wire.contains("portable-display"));
    assert!(!wire.contains("\"display\""));
}

// Package-local contract construction: no workspace-only include resource.
fn query_fixture() -> serde_json::Value {
    json!({
        "schema": "mant.query/v0.12", "label": "test",
        "document": {
            "schema": "mant.document/v0.12",
            "producer": {"name": "mant-native", "version": "0.12.0",
                         "engine": {"name": "libmandoc", "version": "1.14.6"}},
            "source": {"format": "man", "path": "/test.1"},
            "meta": {"title": "TEST", "manualSection": "1", "names": ["test"]},
            "sections": [{"id": "section-0",
                          "heading": {"content": [{"type": "text", "value": "DESCRIPTION"}]},
                          "blocks": [{"type": "paragraph", "children": []}], "children": []}]
        }
    })
}
