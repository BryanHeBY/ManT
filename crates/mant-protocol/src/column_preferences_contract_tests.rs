//! Source-neutral table preferences in the unreleased v0.12 wire contract.

use crate::{DocumentResponse, QueryBundle, inline_contract_tests::query_fixture};
use mant_ir::{Block, ColumnPreferences, Inline, TableRow};
use serde_json::{Value, json};

fn table_query(preferences: Option<Value>) -> Value {
    let mut table = json!({"type":"table","rows":[{"cells":[
        {"blocks":[{"type":"paragraph","children":[
            {"type":"strong","children":[{"type":"text","value":" 中e\u{301}\n"}]}
        ]}]},
        {"blocks":[{"type":"paragraph","children":[
            {"type":"link","target":{"kind":"external","uri":"https://example.org"},
             "title":"kept title","children":[{"type":"text","value":"BODY "}]}
        ]}]}
    ]}]});
    if let Some(preferences) = preferences {
        table["columnPreferences"] = preferences;
    }
    let mut query = query_fixture();
    query["document"]["sections"][0]["blocks"] = json!([table]);
    query
}

fn table(blocks: &[Block]) -> (&ColumnPreferences, &[TableRow]) {
    let [
        Block::Table {
            column_preferences,
            rows,
            ..
        },
    ] = blocks
    else {
        panic!("one table owner")
    };
    (column_preferences, rows)
}

fn assert_cells(rows: &[TableRow]) {
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 2);
    for cell in &rows[0].cells {
        assert_eq!((cell.column_span, cell.row_span), (1, 1));
    }
    let Block::Paragraph { children, .. } = &rows[0].cells[0].blocks[0] else {
        panic!("first cell paragraph")
    };
    assert!(matches!(children.as_slice(), [Inline::Strong { .. }]));
    assert_eq!(mant_ir::inline_plain_text(children), " 中e\u{301}\n");
    let Block::Paragraph { children, .. } = &rows[0].cells[1].blocks[0] else {
        panic!("second cell paragraph")
    };
    assert!(
        matches!(children.as_slice(), [Inline::Link { target, title: Some(title), .. }]
        if target.to_uri().as_deref() == Some("https://example.org") && title == "kept title")
    );
    assert_eq!(mant_ir::inline_plain_text(children), "BODY ");
}

#[test]
fn table_preferences_round_trip_actual_query_and_independent_document_json() {
    for expected in [
        ColumnPreferences::default(),
        ColumnPreferences {
            widths: vec![8, 1],
            gap_columns: 4,
            advance_limit_columns: Some(256),
            extra_width_columns: Some(10),
        },
        ColumnPreferences {
            widths: vec![u16::MAX, 0],
            gap_columns: u16::MAX,
            advance_limit_columns: Some(u16::MAX),
            extra_width_columns: Some(u16::MAX),
        },
        ColumnPreferences {
            widths: vec![],
            gap_columns: 0,
            advance_limit_columns: Some(0),
            extra_width_columns: Some(0),
        },
        ColumnPreferences {
            gap_columns: 3,
            ..ColumnPreferences::default()
        },
    ] {
        let preferences = json!({"widths":expected.widths,"gapColumns":expected.gap_columns,
            "advanceLimitColumns":expected.advance_limit_columns,"extraWidthColumns":expected.extra_width_columns});
        let source = table_query(Some(preferences));
        let parsed: QueryBundle = serde_json::from_str(&source.to_string()).unwrap();
        let (preferences, rows) = table(&parsed.document.as_ref().unwrap().sections[0].blocks);
        assert_eq!(preferences, &expected);
        assert_cells(rows);
        let wire = serde_json::to_string(&parsed).unwrap();
        let restored: QueryBundle = serde_json::from_str(&wire).unwrap();
        assert_eq!(restored, parsed);
        let value: Value = serde_json::from_str(&wire).unwrap();
        let encoded = &value["document"]["sections"][0]["blocks"][0];
        assert!(encoded.get("columnWidths").is_none());
        assert_eq!(
            encoded.get("columnPreferences").is_none(),
            expected.is_empty()
        );
        if expected.advance_limit_columns == Some(0) {
            assert_eq!(encoded["columnPreferences"]["advanceLimitColumns"], 0);
            assert_eq!(encoded["columnPreferences"]["extraWidthColumns"], 0);
        }
        let document: DocumentResponse =
            serde_json::from_str(&source["document"].to_string()).unwrap();
        assert_eq!(table(&document.sections[0].blocks).0, &expected);
        let document_wire = serde_json::to_string(&document).unwrap();
        assert_eq!(
            serde_json::from_str::<DocumentResponse>(&document_wire).unwrap(),
            document
        );
    }
}

#[test]
fn absent_empty_and_null_optional_constraints_have_the_same_canonical_defaults() {
    for preferences in [
        None,
        Some(json!({})),
        Some(json!({"widths":[],"gapColumns":2})),
        Some(json!({"advanceLimitColumns":null,"extraWidthColumns":null})),
    ] {
        let source = table_query(preferences);
        let parsed: QueryBundle = serde_json::from_str(&source.to_string()).unwrap();
        assert_eq!(
            table(&parsed.document.as_ref().unwrap().sections[0].blocks).0,
            &ColumnPreferences::default()
        );
        let value: Value = serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
        assert!(
            value["document"]["sections"][0]["blocks"][0]
                .get("columnPreferences")
                .is_none()
        );
    }
}

#[test]
fn current_preferences_reject_unknown_null_required_and_out_of_range_fields() {
    for preferences in [
        json!(null),
        json!([]),
        json!("default"),
        json!({"nativeGap":4}),
        json!({"widths":null}),
        json!({"widths":{}}),
        json!({"widths":[null]}),
    ] {
        let wire = table_query(Some(preferences)).to_string();
        assert!(
            serde_json::from_str::<QueryBundle>(&wire).is_err(),
            "accepted invalid preferences: {wire}"
        );
    }
    for field in [
        "widths",
        "gapColumns",
        "advanceLimitColumns",
        "extraWidthColumns",
    ] {
        for invalid in [json!(-1), json!(65536), json!(1.5), json!("1"), json!(true)] {
            let mut preferences = json!({});
            preferences[field] = if field == "widths" {
                json!([invalid])
            } else {
                invalid
            };
            let wire = table_query(Some(preferences)).to_string();
            assert!(
                serde_json::from_str::<QueryBundle>(&wire).is_err(),
                "{field}: {wire}"
            );
        }
    }
    let wire = table_query(Some(json!({"gapColumns":null}))).to_string();
    assert!(
        serde_json::from_str::<QueryBundle>(&wire).is_err(),
        "{wire}"
    );
}

#[test]
fn table_preferences_require_objects_in_actual_query_and_document_json() {
    for sequence in [
        json!([]),
        json!([[]]),
        json!([[], 2]),
        json!([[], 2, null, null]),
        json!([[8, 1], 4, 256, 10]),
        json!([{"widths":[8,1],"gapColumns":4}]),
    ] {
        assert!(
            serde_json::from_str::<ColumnPreferences>(&sequence.to_string()).is_err(),
            "accepted positional preferences: {sequence}"
        );
        let source = table_query(Some(sequence));
        let wire = source.to_string();
        assert!(
            serde_json::from_str::<QueryBundle>(&wire).is_err(),
            "{wire}"
        );
        let document_wire = source["document"].to_string();
        assert!(
            serde_json::from_str::<DocumentResponse>(&document_wire).is_err(),
            "{document_wire}"
        );
    }
}

#[test]
fn retired_width_arrays_and_mixed_table_shapes_are_rejected() {
    for preferences in [
        None,
        Some(json!({})),
        Some(json!({"widths":[8,1],"gapColumns":4})),
    ] {
        for old in [json!([]), json!([8, 1]), Value::Null] {
            let mut source = table_query(preferences.clone());
            source["document"]["sections"][0]["blocks"][0]["columnWidths"] = old;
            assert!(serde_json::from_str::<QueryBundle>(&source.to_string()).is_err());
            assert!(
                serde_json::from_str::<DocumentResponse>(&source["document"].to_string()).is_err()
            );
        }
    }
}

#[test]
fn raw_current_table_wire_rejects_duplicate_preferences_and_each_preference_field() {
    let accepted = r#"{"type":"table","rows":[],"columnPreferences":{"widths":[8,1],"gapColumns":4,"advanceLimitColumns":256,"extraWidthColumns":10}}"#;
    assert!(serde_json::from_str::<Block>(accepted).is_ok());
    for wire in [
        accepted.replace("\"widths\":[8,1]", "\"widths\":[8,1],\"widths\":[3]"),
        accepted.replace("\"gapColumns\":4", "\"gapColumns\":4,\"gapColumns\":0"),
        accepted.replace(
            "\"advanceLimitColumns\":256",
            "\"advanceLimitColumns\":256,\"advanceLimitColumns\":0",
        ),
        accepted.replace(
            "\"extraWidthColumns\":10",
            "\"extraWidthColumns\":10,\"extraWidthColumns\":0",
        ),
        accepted.replace("\"rows\":[]", "\"rows\":[],\"columnPreferences\":{}"),
    ] {
        assert!(serde_json::from_str::<Block>(&wire).is_err(), "{wire}");
    }
}
