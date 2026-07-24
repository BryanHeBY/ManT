//! Verifies that the discoverable contracts are generated from Rust types.

use mant_ast::{
    query_bundle_json_schema, query_excerpt_json_schema, query_json_schema_catalog,
    query_outline_json_schema, query_request_json_schema, query_search_json_schema,
};
use serde_json::Value;

fn value(schema: schemars::Schema) -> Value {
    serde_json::to_value(schema).expect("serialize generated schema")
}

fn required(schema: &Value) -> Vec<&str> {
    schema["required"]
        .as_array()
        .expect("required fields")
        .iter()
        .map(|field| field.as_str().expect("required field name"))
        .collect()
}

#[test]
fn request_schema_is_closed_versioned_and_deserialization_oriented() {
    let schema = value(query_request_json_schema());
    let encoded = serde_json::to_string(&schema).expect("schema JSON");

    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["title"], "QueryRequest");
    assert_eq!(schema["$id"], "urn:mant:request:v3");
    assert_eq!(schema["additionalProperties"], false);
    assert!(required(&schema).contains(&"schema"));
    assert!(required(&schema).contains(&"input"));
    assert!(required(&schema).contains(&"view"));
    assert!(encoded.contains("mant.request/v3"));
    assert!(encoded.contains("markdown-file"));
    assert!(encoded.contains("outline"));
    assert!(encoded.contains("excerpt"));
    assert!(encoded.contains("search"));
}

#[test]
fn response_schemas_follow_the_serialized_wire_shapes() {
    for (schema, marker) in [
        (query_bundle_json_schema(), "mant.query/v3"),
        (query_outline_json_schema(), "mant.outline/v3"),
        (query_excerpt_json_schema(), "mant.excerpt/v3"),
        (query_search_json_schema(), "mant.search/v2"),
    ] {
        let encoded = serde_json::to_string(&schema).expect("schema JSON");
        assert!(encoded.contains(marker), "missing marker {marker}");
    }

    let query = value(query_bundle_json_schema());
    let encoded_query = serde_json::to_string(&query).expect("query schema JSON");
    let fields = required(&query);
    assert!(fields.contains(&"schema"));
    assert!(fields.contains(&"label"));
    assert!(!fields.contains(&"document"));
    assert!(!fields.contains(&"tldr"));
    assert!(encoded_query.contains("mant.document/v3"));
    assert!(encoded_query.contains("DefinitionIdentity"));

    let outline = serde_json::to_string(&query_outline_json_schema()).expect("outline schema JSON");
    assert!(outline.contains("document-entry"));
    assert!(outline.contains("options"));

    let search = serde_json::to_string(&query_search_json_schema()).expect("search schema JSON");
    assert!(search.contains("startLine"));
    assert!(search.contains("document-entry"));
    assert!(search.contains("nextOffset"));
}

#[test]
fn schema_catalog_exposes_every_public_query_contract() {
    let catalog = query_json_schema_catalog();
    assert_eq!(
        catalog.keys().copied().collect::<Vec<_>>(),
        ["excerpt", "outline", "query", "request", "search"]
    );
}
