//! Wide logical spans preserve positions without expanding covered cells.
use mant_ir::{
    Block, Document, DocumentMeta, DocumentSource, LayoutHint, SourceFormat, TableCell, TableGrid,
    TableRow, TableRowKind, TableRowPlan, bounded_table_rows, validate_document,
};
use serde_json::json;

fn cell(span: u32, value: &str) -> TableCell {
    serde_json::from_str(
        &json!({
            "columnSpan":span,
            "blocks":[{"type":"paragraph","children":[{"type":"text","value":value}]}]
        })
        .to_string(),
    )
    .unwrap()
}

fn document(rows: Vec<TableRow>) -> Document {
    Document {
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Markdown,
            path: None,
        },
        meta: DocumentMeta::default(),
        heading: None,
        fragment_aliases: vec![],
        diagnostics: vec![],
        sections: vec![],
        blocks: vec![Block::Table {
            rows,
            borders: mant_ir::TableBorders::default(),
            alignment: None,
            column_count: None,
            column_preferences: mant_ir::ColumnPreferences::default(),
            layout: LayoutHint::default(),
            source: None,
        }],
    }
}

#[test]
fn spans_across_the_old_u16_boundary_keep_the_following_original_cell_column() {
    for span in [65534_u32, 65535, 65536, u32::MAX - 1] {
        let rows = vec![TableRow {
            kind: TableRowKind::Data,
            column_borders: vec![],
            cells: vec![cell(span, "LEFT"), cell(1, "RIGHT")],
        }];
        let document = document(rows);
        let wire = serde_json::to_string(&document).unwrap();
        let restored: Document = serde_json::from_str(&wire).unwrap();
        assert_eq!(restored, document);
        let Block::Table { rows, .. } = &restored.blocks[0] else {
            panic!("table")
        };
        let grid = TableGrid::new(rows);
        assert_eq!(grid.column_count, usize::try_from(span).unwrap() + 1);
        assert_eq!(grid.rows[0].len(), 2);
        assert_eq!(grid.rows[0][1].column, usize::try_from(span).unwrap());
        assert!(grid.slots(0, 256).is_none());
        let plans = bounded_table_rows(rows);
        let [TableRowPlan::Sparse { cells }] = plans.as_slice() else {
            panic!("sparse")
        };
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[1].column, usize::try_from(span).unwrap());
    }
}

#[test]
fn maximum_individual_span_values_round_trip_without_allocating_continuations() {
    for field in ["columnSpan", "rowSpan"] {
        for span in [65534_u32, 65535, 65536, u32::MAX] {
            let mut value = json!({"blocks":[]});
            value[field] = json!(span);
            let decoded: TableCell = serde_json::from_str(&value.to_string()).unwrap();
            assert_eq!(
                serde_json::from_str::<TableCell>(&serde_json::to_string(&decoded).unwrap())
                    .unwrap(),
                decoded
            );
            assert_eq!(
                if field == "columnSpan" {
                    decoded.column_span
                } else {
                    decoded.row_span
                },
                span
            );
        }
        for invalid in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(4_294_967_296_u64),
            json!("65536"),
            json!(null),
        ] {
            let mut value = json!({"blocks":[]});
            value[field] = invalid;
            assert!(
                serde_json::from_str::<TableCell>(&value.to_string()).is_err(),
                "{value}"
            );
        }
    }
    let rows = vec![TableRow {
        kind: TableRowKind::Data,
        column_borders: vec![],
        cells: vec![cell(u32::MAX, "MAX")],
    }];
    assert_eq!(TableGrid::new(&rows).rows[0].len(), 1);
    assert!(
        matches!(bounded_table_rows(&rows).as_slice(), [TableRowPlan::Sparse { cells }] if cells.len() == 1)
    );
}

#[test]
fn aggregate_column_overflow_is_rejected_before_32_bit_positions_can_saturate() {
    let rows = vec![TableRow {
        kind: TableRowKind::Data,
        column_borders: vec![],
        cells: vec![cell(u32::MAX, "LEFT"), cell(1, "RIGHT")],
    }];
    // Defensive plans still retain both owners for trusted malformed Rust
    // values; public decoding and validation reject the unrepresentable range.
    assert_eq!(TableGrid::new(&rows).rows[0].len(), 2);
    assert!(
        matches!(bounded_table_rows(&rows).as_slice(), [TableRowPlan::Sparse { cells }] if cells.len() == 2)
    );
    let document = document(rows);
    assert!(
        validate_document(&document)
            .iter()
            .any(|finding| finding.code.as_deref() == Some("ir.invalid-table-column-range"))
    );
    assert!(serde_json::from_str::<Document>(&serde_json::to_string(&document).unwrap()).is_err());
}

#[test]
fn inserting_rule_rows_preserves_data_row_span_ownership() {
    let document: Document = serde_json::from_str(
        &serde_json::to_string(&document(vec![
            TableRow { kind: TableRowKind::Data, column_borders: vec![], cells: vec![
                serde_json::from_str(r#"{"rowSpan":3,"blocks":[{"type":"paragraph","children":[{"type":"text","value":"OWNER"}]}]}"#).unwrap()
            ] },
            TableRow { kind: TableRowKind::HorizontalRule, column_borders: vec![], cells: vec![] },
            TableRow { kind: TableRowKind::Data, column_borders: vec![], cells: vec![cell(1, "")] },
            TableRow { kind: TableRowKind::DoubleHorizontalRule, column_borders: vec![], cells: vec![] },
            TableRow { kind: TableRowKind::Data, column_borders: vec![], cells: vec![cell(1, "")] },
        ])).unwrap()
    ).unwrap();
    let Block::Table { rows, .. } = &document.blocks[0] else {
        panic!("table")
    };
    assert_eq!(rows[0].cells[0].row_span, 3);
    assert_eq!(
        rows.iter()
            .filter(|row| matches!(row.kind, TableRowKind::Data))
            .count(),
        3
    );
    assert_eq!(TableGrid::new(rows).rows.len(), 5);
    assert_eq!(validate_document(&document).len(), 0);
}
