//! Locks the structural wire contract independently from `mant-ir` evolution.

use mant_protocol::{NATIVE_API_VERSION, query_json_schema_catalog};
use serde_json::Value;

const V7_SNAPSHOT: &str = include_str!("../../../tests/contracts/protocol-schemas-v7.json");

fn remove_non_structural_metadata(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("description");
            object.remove("title");
            for value in object.values_mut() {
                remove_non_structural_metadata(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                remove_non_structural_metadata(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[test]
fn v7_structural_schemas_change_only_with_an_explicit_protocol_version() {
    assert_eq!(NATIVE_API_VERSION, "7");

    let mut expected: Value = serde_json::from_str(V7_SNAPSHOT).expect("v7 schema snapshot");
    let mut actual = serde_json::to_value(query_json_schema_catalog()).expect("generated schemas");
    remove_non_structural_metadata(&mut expected);
    remove_non_structural_metadata(&mut actual);

    assert_eq!(
        actual, expected,
        "the v7 structural contract changed; restore compatibility or advance every affected schema discriminator before regenerating the snapshot"
    );
}

#[test]
fn v7_catalog_exposes_only_logical_document_identities() {
    let catalog = serde_json::to_value(mant_protocol::document_catalog_json_schema())
        .expect("catalog schema");
    let summary = &catalog["$defs"]["DocumentSummary"];
    assert!(summary["properties"].get("sourcePath").is_none());
    assert_eq!(
        summary["required"],
        serde_json::json!(["address", "catalogPath"])
    );
}
