//! Authored edges and soft cell widths survive real process-protocol JSON.
use crate::{DocumentResponse, QueryBundle, inline_contract_tests::query_fixture};
use mant_ir::{Block, TableBorderStyle, TableGrid};
use serde_json::{Value, json};

fn query() -> Value {
    let mut source = query_fixture();
    source["document"]["sections"][0]["blocks"] = json!([{
        "type":"table","columnCount":4,
        "borders":{"outer":"single","innerRows":"double","innerColumns":"single"},
        "rows":[{"columnBorders":[{"column":0,"style":"single"},{"column":2,"style":"double"},{"column":4,"style":"single"}],
            "cells":[{"columnSpan":2,"preferredWidthColumns":34,"blocks":[{
                "type":"paragraph","children":[{"type":"link","target":{"kind":"external","uri":"https://example.test"},"children":[{"type":"text","value":"LABEL"}]}]
            }]},{"blocks":[]}]}]
    }]);
    source
}

fn table(source: &mut Value) -> &mut Value {
    &mut source["document"]["sections"][0]["blocks"][0]
}

fn assert_rejects(source: &Value) {
    assert!(
        serde_json::from_str::<QueryBundle>(&source.to_string()).is_err(),
        "{source}"
    );
    assert!(
        serde_json::from_str::<DocumentResponse>(&source["document"].to_string()).is_err(),
        "{source}"
    );
}

#[test]
fn table_edges_and_soft_widths_round_trip_without_replacing_content_owners() {
    let source = query();
    let parsed: QueryBundle = serde_json::from_str(&source.to_string()).unwrap();
    let Block::Table {
        rows,
        borders,
        column_count,
        ..
    } = &parsed.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("table owner")
    };
    assert_eq!(*column_count, Some(4));
    assert_eq!(borders.outer, Some(TableBorderStyle::Single));
    assert_eq!(borders.inner_rows, Some(TableBorderStyle::Double));
    assert_eq!(
        rows[0].column_border_style(borders, 2, 4),
        Some(TableBorderStyle::Double)
    );
    assert_eq!(
        rows[0].column_border_style(borders, 1, 4),
        Some(TableBorderStyle::Single)
    );
    assert_eq!(rows[0].cells.len(), 2);
    assert_eq!(rows[0].cells[0].preferred_width_columns, Some(34));
    let Block::Paragraph { children, .. } = &rows[0].cells[0].blocks[0] else {
        panic!("original inline root")
    };
    assert_eq!(mant_ir::inline_plain_text(children), "LABEL");
    let wire = serde_json::to_string(&parsed).unwrap();
    assert_eq!(serde_json::from_str::<QueryBundle>(&wire).unwrap(), parsed);
    let document: DocumentResponse = serde_json::from_str(&source["document"].to_string()).unwrap();
    assert_eq!(
        serde_json::from_str::<DocumentResponse>(&serde_json::to_string(&document).unwrap())
            .unwrap(),
        document
    );
}

#[test]
fn empty_defaults_and_null_optional_strokes_do_not_invent_edges_or_cell_widths() {
    for borders in [
        json!({}),
        json!({"outer":null,"innerRows":null,"innerColumns":null}),
    ] {
        let mut source = query();
        let table = table(&mut source);
        table["borders"] = borders;
        table["rows"][0]["columnBorders"] = json!([]);
        table["rows"][0]["cells"][0]["preferredWidthColumns"] = Value::Null;
        let parsed: QueryBundle = serde_json::from_str(&source.to_string()).unwrap();
        let canonical = serde_json::to_value(parsed).unwrap();
        let table = &canonical["document"]["sections"][0]["blocks"][0];
        assert!(table.get("borders").is_none());
        assert!(table["rows"][0].get("columnBorders").is_none());
        assert!(
            table["rows"][0]["cells"][0]
                .get("preferredWidthColumns")
                .is_none()
        );
    }
}

#[test]
fn border_ranges_stay_sparse_even_at_the_largest_declared_column() {
    let mut source = query();
    table(&mut source)["columnCount"] = json!(u32::MAX);
    table(&mut source)["rows"][0]["columnBorders"] = json!([{"column":u32::MAX,"style":"double"}]);
    let parsed: QueryBundle = serde_json::from_str(&source.to_string()).unwrap();
    let Block::Table {
        rows,
        column_count,
        borders,
        ..
    } = &parsed.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("table")
    };
    let grid = TableGrid::with_column_count(rows, *column_count);
    assert_eq!(grid.rows[0].len(), 2);
    assert!(grid.slots(0, 256).is_none());
    assert_eq!(
        rows[0].column_border_style(
            borders,
            usize::try_from(u32::MAX).unwrap(),
            grid.column_count
        ),
        Some(TableBorderStyle::Double)
    );
}

#[test]
fn wire_rejects_unordered_duplicate_and_out_of_range_column_edges() {
    for edges in [
        json!([{"column":5,"style":"single"}]),
        json!([{"column":2,"style":"single"},{"column":1,"style":"double"}]),
        json!([{"column":2,"style":"single"},{"column":2,"style":"double"}]),
        json!([{"column":-1,"style":"single"}]),
        json!([{"column":4_294_967_296_u64,"style":"single"}]),
        json!([[0, "single"]]),
        json!([{"column":0,"style":"dashed"}]),
        json!([{"column":0,"style":"single","native":true}]),
        Value::Null,
    ] {
        let mut source = query();
        table(&mut source)["rows"][0]["columnBorders"] = edges;
        assert_rejects(&source);
    }
}

#[test]
fn wire_rejects_nonpositive_soft_widths_and_nonobject_border_preferences() {
    for width in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(65536),
        json!("34"),
        json!([]),
        json!({}),
        json!(false),
    ] {
        let mut source = query();
        table(&mut source)["rows"][0]["cells"][0]["preferredWidthColumns"] = width;
        assert_rejects(&source);
    }
    for borders in [
        Value::Null,
        json!([]),
        json!(["single", null, null]),
        json!({"outer":"solid"}),
        json!({"frame":"single"}),
    ] {
        let mut source = query();
        table(&mut source)["borders"] = borders;
        assert_rejects(&source);
    }
    for row in [json!(["data", [], []]), json!([]), json!(null)] {
        let mut source = query();
        table(&mut source)["rows"][0] = row;
        assert_rejects(&source);
    }
}

#[test]
fn actual_json_rejects_duplicate_edge_and_width_fields_including_null() {
    for (field, value) in [
        ("outer", "null"),
        ("outer", "\"single\""),
        ("preferredWidthColumns", "null"),
        ("preferredWidthColumns", "34"),
    ] {
        let mut source = query();
        let parsed: Value = serde_json::from_str(value).unwrap();
        if field == "outer" {
            table(&mut source)["borders"][field] = parsed;
        } else {
            table(&mut source)["rows"][0]["cells"][0][field] = parsed;
        }
        let wire = source.to_string();
        let duplicate = wire.replacen(
            &format!("\"{field}\":{value}"),
            &format!("\"{field}\":{value},\"{field}\":{value}"),
            1,
        );
        assert_ne!(duplicate, wire);
        assert!(
            serde_json::from_str::<QueryBundle>(&duplicate).is_err(),
            "{duplicate}"
        );
    }
}

#[test]
fn nested_edge_defaults_and_row_arrays_reject_duplicate_named_fields() {
    let source = query();
    for (needle, duplicate) in [
        ("\"column\":0", "\"column\":0,\"column\":0"),
        (
            "\"style\":\"single\"",
            "\"style\":\"single\",\"style\":\"double\"",
        ),
    ] {
        for wire in [source.to_string(), source["document"].to_string()] {
            let duplicate = wire.replacen(needle, duplicate, 1);
            assert_ne!(wire, duplicate);
            if wire.contains("mant.query/") {
                assert!(serde_json::from_str::<QueryBundle>(&duplicate).is_err());
            } else {
                assert!(serde_json::from_str::<DocumentResponse>(&duplicate).is_err());
            }
        }
    }
    for field in ["innerRows", "innerColumns"] {
        let mut source = query();
        table(&mut source)["borders"][field] = Value::Null;
        let wire = source.to_string();
        let duplicate = wire.replacen(
            &format!("\"{field}\":null"),
            &format!("\"{field}\":null,\"{field}\":\"single\""),
            1,
        );
        assert!(serde_json::from_str::<QueryBundle>(&duplicate).is_err());
    }
    let mut source = query();
    table(&mut source)["rows"][0]["columnBorders"] = json!([]);
    let wire = source.to_string();
    let duplicate = wire.replacen(
        "\"columnBorders\":[]",
        "\"columnBorders\":[],\"columnBorders\":[]",
        1,
    );
    assert!(serde_json::from_str::<QueryBundle>(&duplicate).is_err());
}

#[test]
fn wide_spans_keep_original_columns_through_query_and_document_json() {
    for span in [65534_u32, 65535, 65536, u32::MAX - 1] {
        let mut source = query();
        let value = table(&mut source);
        value["columnCount"] = json!(span + 1);
        value["rows"][0]["cells"][0]["columnSpan"] = json!(span);
        value["rows"][0]["columnBorders"] = json!([
            {"column":0,"style":"single"},
            {"column":span,"style":"double"},
            {"column":span+1,"style":"single"}
        ]);
        let parsed: QueryBundle = serde_json::from_str(&source.to_string()).unwrap();
        let document: DocumentResponse =
            serde_json::from_str(&source["document"].to_string()).unwrap();
        let Block::Table {
            rows, column_count, ..
        } = &document.sections[0].blocks[0]
        else {
            panic!("table")
        };
        let grid = TableGrid::with_column_count(rows, *column_count);
        assert_eq!(grid.rows[0][1].column, usize::try_from(span).unwrap());
        assert_eq!(rows[0].cells[0].column_span, span);
        assert_eq!(
            serde_json::from_str::<QueryBundle>(&serde_json::to_string(&parsed).unwrap()).unwrap(),
            parsed
        );
    }
    let mut source = query();
    table(&mut source)["rows"][0]["cells"][0]["columnSpan"] = json!(u32::MAX);
    table(&mut source)["columnCount"] = json!(u32::MAX);
    assert_rejects(&source);
}

#[test]
fn strokes_follow_the_schema_string_shape_in_all_border_positions() {
    for stroke in [
        json!({"single":null}),
        json!({"double":null}),
        json!(["single"]),
        json!([]),
        json!(0),
        json!(false),
        json!("Single"),
    ] {
        assert!(serde_json::from_str::<TableBorderStyle>(&stroke.to_string()).is_err());
        for field in ["outer", "innerRows", "innerColumns"] {
            let mut source = query();
            table(&mut source)["borders"][field] = stroke.clone();
            assert_rejects(&source);
        }
        let mut source = query();
        table(&mut source)["rows"][0]["columnBorders"][0]["style"] = stroke;
        assert_rejects(&source);
    }
    for (wire, expected) in [
        ("single", TableBorderStyle::Single),
        ("double", TableBorderStyle::Double),
    ] {
        assert_eq!(
            serde_json::from_str::<TableBorderStyle>(&json!(wire).to_string()).unwrap(),
            expected
        );
    }
}
