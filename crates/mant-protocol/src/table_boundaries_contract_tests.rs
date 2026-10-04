//! A table-cell boundary closes occupied output without adding body content.

use crate::{DocumentResponse, QueryBundle, inline_contract_tests::query_fixture};
use mant_ir::{Block, Inline, TableCell};
use serde_json::{Value, json};

fn boundary_query(boundary: Option<Value>) -> Value {
    let mut first = json!({"blocks":[{"type":"paragraph","children":[
        {"type":"strong","children":[{"type":"text","value":"A"}]}
    ],"inlineLayout":{"rowHints":[{"row":0,"indentColumns":3}]}}]});
    if let Some(boundary) = boundary {
        first["breakAfter"] = boundary;
    }
    let mut query = query_fixture();
    query["document"]["sections"][0]["blocks"] = json!([{
        "type":"table","columnPreferences":{"widths":[8,4],"gapColumns":4},
        "rows":[{"cells":[first,{"blocks":[{"type":"paragraph","children":[
            {"type":"link","target":{"kind":"external","uri":"https://example.org"},
             "title":"kept title","children":[{"type":"text","value":"B"}]}
        ]}]}]}]
    }]);
    query
}

fn first_cell(blocks: &[Block]) -> &TableCell {
    let [Block::Table { rows, .. }] = blocks else {
        panic!("one table")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 2);
    let [Block::Paragraph { children, .. }] = rows[0].cells[1].blocks.as_slice() else {
        panic!("second original paragraph")
    };
    assert!(
        matches!(children.as_slice(), [Inline::Link { target, title: Some(title), .. }]
        if target.to_uri().as_deref() == Some("https://example.org") && title == "kept title")
    );
    assert_eq!(mant_ir::inline_plain_text(children), "B");
    assert_eq!(mant_ir::logical_row_count(children), 1);
    &rows[0].cells[0]
}

fn assert_body(cell: &TableCell) {
    let [
        Block::Paragraph {
            children,
            inline_layout,
            ..
        },
    ] = cell.blocks.as_slice()
    else {
        panic!("one original paragraph")
    };
    assert!(matches!(children.as_slice(), [Inline::Strong { .. }]));
    assert_eq!(mant_ir::inline_plain_text(children), "A");
    assert_eq!(mant_ir::logical_row_count(children), 1);
    assert_eq!(inline_layout.row_indent(0), 3);
    assert_eq!((cell.column_span, cell.row_span), (1, 1));
}

#[test]
fn occupied_row_boundaries_round_trip_actual_query_and_independent_document_json() {
    for boundary in [None, Some(json!(false)), Some(json!(true))] {
        let expected = boundary.as_ref().is_some_and(|value| value == true);
        let source = boundary_query(boundary);
        let wire = source.to_string();
        let parsed: QueryBundle = serde_json::from_str(&wire).unwrap();
        let cell = first_cell(&parsed.document.as_ref().unwrap().sections[0].blocks);
        assert_eq!(cell.break_after, expected);
        assert_body(cell);
        let canonical = serde_json::to_string(&parsed).unwrap();
        assert_eq!(
            serde_json::from_str::<QueryBundle>(&canonical).unwrap(),
            parsed
        );
        let value: Value = serde_json::from_str(&canonical).unwrap();
        let encoded = &value["document"]["sections"][0]["blocks"][0]["rows"][0]["cells"][0];
        assert_eq!(
            encoded.get("breakAfter"),
            expected.then_some(&Value::Bool(true))
        );
        let document: DocumentResponse =
            serde_json::from_str(&source["document"].to_string()).unwrap();
        let independent = first_cell(&document.sections[0].blocks);
        assert_eq!(independent.break_after, expected);
        assert_body(independent);
        assert_eq!(
            serde_json::from_str::<DocumentResponse>(&serde_json::to_string(&document).unwrap())
                .unwrap(),
            document
        );
    }
}

fn assert_actual_wire_rejects(source: &Value) {
    let wire = source.to_string();
    assert!(
        serde_json::from_str::<QueryBundle>(&wire).is_err(),
        "{wire}"
    );
    let document = source["document"].to_string();
    assert!(
        serde_json::from_str::<DocumentResponse>(&document).is_err(),
        "{document}"
    );
}

#[test]
fn boundary_fields_require_booleans_and_closed_table_cells() {
    let baseline = boundary_query(Some(json!(true)));
    assert!(serde_json::from_str::<QueryBundle>(&baseline.to_string()).is_ok());
    for invalid in [
        Value::Null,
        json!(0),
        json!(1),
        json!(-1),
        json!("true"),
        json!([]),
        json!({}),
    ] {
        assert_actual_wire_rejects(&boundary_query(Some(invalid)));
    }
    let mut unknown = baseline;
    unknown["document"]["sections"][0]["blocks"][0]["rows"][0]["cells"][0]["closedRow"] =
        json!(true);
    assert_actual_wire_rejects(&unknown);
}

#[test]
fn table_cell_owners_reject_positional_arrays_in_actual_json() {
    for invalid in [
        json!([]),
        json!(["text", [], false, 1, 1, null]),
        json!(["text", [], true, 1, 1, "left"]),
        Value::Null,
        json!("cell"),
    ] {
        let mut source = boundary_query(None);
        source["document"]["sections"][0]["blocks"][0]["rows"][0]["cells"][0] = invalid;
        assert_actual_wire_rejects(&source);
    }
}

#[test]
fn raw_boundary_wire_rejects_duplicate_true_and_false_fields() {
    for boundary in [true, false] {
        let source = boundary_query(Some(json!(boundary)));
        for wire in [source.to_string(), source["document"].to_string()] {
            let decoded = if wire.contains("mant.query/") {
                serde_json::from_str::<QueryBundle>(&wire).map(|_| ())
            } else {
                serde_json::from_str::<DocumentResponse>(&wire).map(|_| ())
            };
            assert!(decoded.is_ok(), "{wire}");
            let duplicate = wire.replacen(
                &format!("\"breakAfter\":{boundary}"),
                &format!("\"breakAfter\":{boundary},\"breakAfter\":{}", !boundary),
                1,
            );
            let decoded = if duplicate.contains("mant.query/") {
                serde_json::from_str::<QueryBundle>(&duplicate).map(|_| ())
            } else {
                serde_json::from_str::<DocumentResponse>(&duplicate).map(|_| ())
            };
            assert!(decoded.is_err(), "{duplicate}");
        }
    }
}
