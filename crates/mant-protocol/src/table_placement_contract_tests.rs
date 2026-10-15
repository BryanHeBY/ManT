//! Whole-table placement and declared logical extent in actual response JSON.
use crate::{DocumentResponse, QueryBundle, inline_contract_tests::query_fixture};
use mant_ir::{Block, TableAlignment};
use serde_json::{Value, json};

fn query(alignment: Option<Value>, columns: Option<Value>) -> Value {
    let mut table = json!({"type":"table","rows":[{"cells":[{
        "columnSpan":2,"blocks":[{"type":"paragraph","children":[{
            "type":"link","target":{"kind":"external","uri":"https://example.test"},
            "children":[{"type":"text","value":"LABEL"}]
        }]}]
    }]}]});
    if let Some(alignment) = alignment {
        table["alignment"] = alignment;
    }
    if let Some(columns) = columns {
        table["columnCount"] = columns;
    }
    let mut query = query_fixture();
    query["document"]["sections"][0]["blocks"] = json!([table]);
    query
}

fn decode(source: &Value) -> bool {
    let query = serde_json::from_str::<QueryBundle>(&source.to_string());
    let document = serde_json::from_str::<DocumentResponse>(&source["document"].to_string());
    assert_eq!(query.is_ok(), document.is_ok(), "{source}");
    query.is_ok()
}

#[test]
fn table_placement_round_trips_and_explicit_left_remains_distinct_from_automatic_flow() {
    for (alignment, expected) in [
        (None, None),
        (Some(Value::Null), None),
        (Some(json!("left")), Some(TableAlignment::Left)),
        (Some(json!("center")), Some(TableAlignment::Center)),
        (Some(json!("right")), Some(TableAlignment::Right)),
    ] {
        for columns in [
            None,
            Some(Value::Null),
            Some(json!(2)),
            Some(json!(4)),
            Some(json!(u32::MAX)),
        ] {
            let expected_count = columns
                .as_ref()
                .and_then(Value::as_u64)
                .map(|count| u32::try_from(count).unwrap());
            let source = query(alignment.clone(), columns);
            let parsed: QueryBundle = serde_json::from_str(&source.to_string()).unwrap();
            let Block::Table {
                alignment,
                column_count,
                rows,
                ..
            } = &parsed.document.as_ref().unwrap().sections[0].blocks[0]
            else {
                panic!("table owner")
            };
            assert_eq!(*alignment, expected);
            assert_eq!(*column_count, expected_count);
            assert_eq!(rows[0].cells.len(), 1);
            assert_eq!(rows[0].cells[0].column_span, 2);
            let wire = serde_json::to_string(&parsed).unwrap();
            assert_eq!(serde_json::from_str::<QueryBundle>(&wire).unwrap(), parsed);
            let canonical: Value = serde_json::from_str(&wire).unwrap();
            let table = &canonical["document"]["sections"][0]["blocks"][0];
            assert_eq!(
                table.get("alignment"),
                expected.map(|value| json!(value)).as_ref()
            );
            assert_eq!(
                table.get("columnCount"),
                expected_count.map(|count| json!(count)).as_ref()
            );
            let document: DocumentResponse =
                serde_json::from_str(&source["document"].to_string()).unwrap();
            assert_eq!(
                serde_json::from_str::<DocumentResponse>(
                    &serde_json::to_string(&document).unwrap()
                )
                .unwrap(),
                document
            );
        }
    }
}

#[test]
fn placement_and_counts_reject_unknown_values_types_and_undersized_ranges() {
    for alignment in [
        json!(false),
        json!(0),
        json!([]),
        json!({}),
        json!("centre"),
        json!("auto"),
    ] {
        assert!(!decode(&query(Some(alignment), None)));
    }
    for columns in [
        json!(0),
        json!(1),
        json!(-1),
        json!(2.5),
        json!(4_294_967_296_u64),
        json!("3"),
        json!(false),
        json!([]),
        json!({}),
    ] {
        assert!(!decode(&query(None, Some(columns))));
    }
    let mut source = query(None, Some(json!(2)));
    source["document"]["sections"][0]["blocks"][0]["rows"] = json!([{
        "kind":{"layout-rule":{"cells":["horizontal","double-horizontal","horizontal"]}},
        "cells":[]
    }]);
    assert!(!decode(&source));
    source["document"]["sections"][0]["blocks"][0]["columnCount"] = json!(3);
    assert!(decode(&source));
}

#[test]
fn actual_json_rejects_duplicate_table_fields_including_null_first_values() {
    for field in ["alignment", "columnCount"] {
        for value in [
            "null",
            if field == "alignment" {
                "\"left\""
            } else {
                "3"
            },
        ] {
            let mut source = query(None, None);
            source["document"]["sections"][0]["blocks"][0][field] =
                serde_json::from_str(value).unwrap();
            for wire in [source.to_string(), source["document"].to_string()] {
                let duplicate = wire.replacen(
                    &format!("\"{field}\":{value}"),
                    &format!("\"{field}\":{value},\"{field}\":{value}"),
                    1,
                );
                assert_ne!(wire, duplicate);
                if wire.contains("mant.query/") {
                    assert!(
                        serde_json::from_str::<QueryBundle>(&duplicate).is_err(),
                        "{duplicate}"
                    );
                } else {
                    assert!(
                        serde_json::from_str::<DocumentResponse>(&duplicate).is_err(),
                        "{duplicate}"
                    );
                }
            }
        }
    }
}
