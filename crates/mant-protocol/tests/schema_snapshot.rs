//! Locks the structural wire contract independently from `mant-ir` evolution.

use mant_protocol::{
    NATIVE_API_VERSION, doctor_report_json_schema, query_json_schema_catalog,
    tldr_cache_update_json_schema,
};
use serde_json::Value;

const V0_10_SNAPSHOT: &str = include_str!("../../../tests/contracts/protocol-schemas-v0.10.json");

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
fn v0_10_structural_schemas_change_only_with_an_explicit_protocol_version() {
    assert_eq!(NATIVE_API_VERSION, "0.10");

    let mut expected: Value = serde_json::from_str(V0_10_SNAPSHOT).expect("v0.10 schema snapshot");
    let mut actual = serde_json::to_value(query_json_schema_catalog()).expect("generated schemas");
    remove_non_structural_metadata(&mut expected);
    remove_non_structural_metadata(&mut actual);

    assert_eq!(
        actual, expected,
        "the v0.10 structural contract changed; restore compatibility or advance every affected schema discriminator before regenerating the snapshot"
    );
}

#[test]
fn v0_10_catalog_exposes_only_logical_document_identities() {
    let catalog = serde_json::to_value(mant_protocol::document_catalog_json_schema())
        .expect("catalog schema");
    let summary = &catalog["$defs"]["DocumentSummary"];
    assert!(summary["properties"].get("sourcePath").is_none());
    assert_eq!(summary["required"], serde_json::json!(["address"]));
}

#[test]
fn doctor_v1_has_an_independent_discriminator_and_stable_result_fields() {
    let schema = serde_json::to_value(doctor_report_json_schema()).expect("doctor schema");
    assert_eq!(schema["$id"], "urn:mant:doctor:v1");
    assert_eq!(
        schema["properties"]["schema"]["$ref"],
        "#/$defs/DoctorSchema"
    );
    assert_eq!(
        schema["$defs"]["DoctorSchema"]["oneOf"][0]["const"],
        "mant.doctor/v1"
    );
    assert_eq!(
        schema["required"],
        serde_json::json!([
            "schema",
            "producer",
            "outcome",
            "environment",
            "checks",
            "summary"
        ])
    );
}

#[test]
fn tldr_update_v1_has_an_independent_discriminator_and_stable_result_fields() {
    let schema = serde_json::to_value(tldr_cache_update_json_schema()).expect("tldr update schema");
    assert_eq!(schema["$id"], "urn:mant:tldr-update:v1");
    assert_eq!(
        schema["$defs"]["TldrCacheUpdateSchema"]["oneOf"][0]["const"],
        "mant.tldr-update/v1"
    );
    assert_eq!(schema["required"], serde_json::json!(["schema", "action"]));
}
