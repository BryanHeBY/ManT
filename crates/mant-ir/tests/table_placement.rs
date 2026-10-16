//! Logical table extent remains independent from text and viewport placement.
use mant_ir::{
    Block, Document, TableCell, TableGrid, TableRow, TableRowKind, TableRowPlan, TableRuleCellKind,
    bounded_table_rows_with_column_count, table_logical_column_count, validate_document,
};

fn cells() -> Vec<TableRow> {
    vec![TableRow {
        column_borders: vec![],
        kind: TableRowKind::Data,
        cells: vec![TableCell {
            preferred_width_columns: None,
            blocks: vec![],
            kind: mant_ir::TableCellKind::Text,
            break_after: false,
            column_span: 2,
            row_span: 1,
            alignment: None,
        }],
    }]
}

#[test]
fn declared_columns_preserve_empty_tails_without_duplicating_owners() {
    let rows = cells();
    assert_eq!(table_logical_column_count(&rows), 2);
    let grid = TableGrid::with_column_count(&rows, Some(4));
    assert_eq!(grid.column_count, 4);
    assert_eq!(grid.rows[0].len(), 1);
    assert!(std::ptr::eq(
        grid.rows[0][0].cell,
        &raw const rows[0].cells[0]
    ));
    let planned = bounded_table_rows_with_column_count(&rows, Some(4));
    let [TableRowPlan::Dense { slots }] = planned.as_slice() else {
        panic!("bounded four-column grid")
    };
    assert_eq!(slots.len(), 4);
    assert!(slots[0].is_some());
    assert!(slots[1..].iter().all(Option::is_none));
    // Defensive layout retains source cells even before structural validation.
    assert_eq!(TableGrid::with_column_count(&rows, Some(1)).column_count, 2);
    for (count, dense) in [(256, true), (257, false)] {
        let planned = bounded_table_rows_with_column_count(&rows, Some(count));
        assert_eq!(matches!(planned[0], TableRowPlan::Dense { .. }), dense);
        if let TableRowPlan::Dense { slots } = &planned[0] {
            assert_eq!(slots.len(), usize::try_from(count).unwrap());
        }
    }
}

#[test]
fn enormous_declared_counts_use_sparse_storage_and_rule_only_tables_keep_extent() {
    let rows = cells();
    let grid = TableGrid::with_column_count(&rows, Some(u32::MAX));
    assert_eq!(grid.column_count, usize::try_from(u32::MAX).unwrap());
    assert!(grid.slots(0, 256).is_none());
    let planned = bounded_table_rows_with_column_count(&rows, Some(u32::MAX));
    let [TableRowPlan::Sparse { cells }] = planned.as_slice() else {
        panic!("large range must not allocate dense columns")
    };
    assert_eq!(cells.len(), 1);
    let rules = [TableRow {
        column_borders: vec![],
        kind: TableRowKind::HorizontalRule,
        cells: vec![],
    }];
    assert_eq!(TableGrid::new(&rules).column_count, 0);
    assert_eq!(
        TableGrid::with_column_count(&rules, Some(3)).column_count,
        3
    );
    assert!(matches!(
        bounded_table_rows_with_column_count(&rules, Some(u32::MAX)).as_slice(),
        [TableRowPlan::WholeRule { double: false }]
    ));
}

#[test]
fn declared_range_must_cover_spans_and_layout_only_rules() {
    for rows in [
        cells(),
        vec![TableRow {
            column_borders: vec![],
            kind: TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::Horizontal; 2],
            },
            cells: vec![],
        }],
    ] {
        for (count, valid) in [
            (None, true),
            (Some(0), false),
            (Some(1), false),
            (Some(2), true),
        ] {
            let document = Document {
                parser: None,
                source: mant_ir::DocumentSource {
                    format: mant_ir::SourceFormat::Markdown,
                    path: None,
                },
                meta: mant_ir::DocumentMeta::default(),
                heading: None,
                fragment_aliases: vec![],
                diagnostics: vec![],
                sections: vec![],
                blocks: vec![Block::Table {
                    borders: mant_ir::TableBorders::default(),
                    rows: rows.clone(),
                    column_count: count,
                    alignment: None,
                    column_preferences: mant_ir::ColumnPreferences::default(),
                    layout: mant_ir::LayoutHint::default(),
                    source: None,
                }],
            };
            let findings = validate_document(&document);
            assert_eq!(
                !findings.iter().any(|finding| finding.code.as_deref() == Some("ir.invalid-table-column-count")),
                valid,
                "{count:?}: {findings:?}"
            );
            let wire = serde_json::to_string(&document).unwrap();
            assert_eq!(
                serde_json::from_str::<Document>(&wire).is_ok(),
                valid,
                "{wire}"
            );
        }
    }
}
