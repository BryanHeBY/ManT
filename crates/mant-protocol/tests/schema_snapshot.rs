//! Locks the structural wire contract independently from `mant-ir` evolution.

use mant_protocol::{
    NATIVE_API_VERSION, doctor_report_json_schema, query_json_schema_catalog,
    tldr_cache_update_json_schema,
};
use serde_json::Value;

const V0_13_SNAPSHOT: &str = include_str!("../../../tests/contracts/protocol-schemas-v0.13.json");
const V0_12_SNAPSHOT: &str = include_str!("../../../tests/contracts/protocol-schemas-v0.12.json");

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
fn current_structural_schemas_match_the_registered_contract() {
    assert_eq!(NATIVE_API_VERSION, "0.13");

    let mut expected: Value = serde_json::from_str(V0_13_SNAPSHOT).expect("v0.13 schema snapshot");
    let mut actual = serde_json::to_value(query_json_schema_catalog()).expect("generated schemas");
    remove_non_structural_metadata(&mut expected);
    remove_non_structural_metadata(&mut actual);

    assert_eq!(
        actual, expected,
        "the structural contract changed; review compatibility, version published contracts when necessary, and update current consumers and the matching snapshot; never overwrite published snapshots"
    );
}

#[test]
fn published_v0_12_row_hints_remain_a_distinct_frozen_contract() {
    let released: Value = serde_json::from_str(V0_12_SNAPSHOT).unwrap();
    let current: Value = serde_json::from_str(V0_13_SNAPSHOT).unwrap();
    for family in ["query", "excerpt", "explanation", "scope-query"] {
        assert_eq!(released[family]["$id"], format!("urn:mant:{family}:v0.12"));
        assert_eq!(current[family]["$id"], format!("urn:mant:{family}:v0.13"));
        let old = &released[family]["$defs"]["RowLayoutHint"];
        assert_eq!(old["additionalProperties"], false);
        assert_eq!(old["required"], serde_json::json!(["row", "indentColumns"]));
        assert_eq!(old["properties"].as_object().unwrap().len(), 2);
        let new = &current[family]["$defs"]["RowLayoutHint"];
        assert_eq!(new["additionalProperties"], false);
        assert_eq!(new["required"], serde_json::json!(["row", "indentColumns"]));
        assert_eq!(new["properties"].as_object().unwrap().len(), 4);
        assert_eq!(
            new["properties"]["firstLineIndentColumns"]["minimum"],
            -65535
        );
        assert_eq!(
            new["properties"]["firstLineIndentColumns"]["maximum"],
            65535
        );
        assert!(new["properties"].get("alignment").is_some());
    }
}

#[test]
fn table_frame_and_logical_extent_belong_only_to_unreleased_v0_13() {
    fn table_properties(family: &Value) -> &Value {
        &family["$defs"]["Block"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|variant| variant["properties"]["type"]["const"] == "table")
            .expect("table block schema")["properties"]
    }
    let released: Value = serde_json::from_str(V0_12_SNAPSHOT).unwrap();
    let current = serde_json::to_value(query_json_schema_catalog()).unwrap();
    for family in ["query", "excerpt", "explanation", "scope-query"] {
        let old = table_properties(&released[family]);
        assert!(old.get("alignment").is_none());
        assert!(old.get("columnCount").is_none());
        let new = table_properties(&current[family]);
        assert!(
            new["alignment"]["anyOf"]
                .as_array()
                .unwrap()
                .iter()
                .any(|branch| branch["$ref"] == "#/$defs/TableAlignment")
        );
        assert_eq!(new["columnCount"]["minimum"], 1);
        assert_eq!(new["columnCount"]["maximum"], u32::MAX);
    }
}

#[test]
fn borders_and_soft_cell_widths_are_closed_v0_13_facts() {
    let released: Value = serde_json::from_str(V0_12_SNAPSHOT).unwrap();
    let current = serde_json::to_value(query_json_schema_catalog()).unwrap();
    for family in ["query", "excerpt", "explanation", "scope-query"] {
        let old = &released[family]["$defs"];
        assert!(old.get("TableBorders").is_none());
        assert!(old["TableRow"]["properties"].get("columnBorders").is_none());
        assert!(
            old["TableCell"]["properties"]
                .get("preferredWidthColumns")
                .is_none()
        );
        let new = &current[family]["$defs"];
        assert_eq!(new["TableBorders"]["additionalProperties"], false);
        assert_eq!(
            new["TableBorders"]["properties"].as_object().unwrap().len(),
            3
        );
        assert_eq!(new["TableColumnBorder"]["additionalProperties"], false);
        assert_eq!(
            new["TableColumnBorder"]["properties"]["column"]["maximum"],
            u32::MAX
        );
        assert_eq!(
            new["TableRow"]["properties"]["columnBorders"]["items"]["$ref"],
            "#/$defs/TableColumnBorder"
        );
        assert_eq!(
            new["TableCell"]["properties"]["preferredWidthColumns"]["minimum"],
            1
        );
        assert_eq!(
            new["TableCell"]["properties"]["preferredWidthColumns"]["maximum"],
            u16::MAX
        );
        for field in ["columnSpan", "rowSpan"] {
            assert_eq!(new["TableCell"]["properties"][field]["minimum"], 1);
            assert_eq!(new["TableCell"]["properties"][field]["maximum"], u32::MAX);
            assert_eq!(old["TableCell"]["properties"][field]["format"], "uint16");
        }
    }
}

#[test]
fn v0_13_catalog_exposes_only_logical_document_identities() {
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
