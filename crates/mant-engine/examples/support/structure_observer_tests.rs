//! Exact pristine-derived spans; mutations change observations, never gold.
use super::*;

fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(include_str!("structure_observer_cases.json"))
        .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}

fn parsed(source: &str) -> (AstStructure, AstTopology, Document) {
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let (expected, topology) = ast_profile(&report.document.root);
    let document =
        mant_codec::parse_roff_bytes(std::path::Path::new("audit.1"), source.as_bytes()).unwrap();
    (expected, topology, document)
}

fn row_kind(kind: &mant_ir::TableRowKind) -> &'static str {
    match kind {
        mant_ir::TableRowKind::Data => "data",
        mant_ir::TableRowKind::HorizontalRule => "horizontal-rule",
        mant_ir::TableRowKind::DoubleHorizontalRule => "double-horizontal-rule",
        mant_ir::TableRowKind::LayoutRule { .. } => "layout-rule",
    }
}

#[test]
fn all_native_table_spans_keep_their_owner_order_and_kind() {
    // Every exact source ran pristine ASCII/UTF8/HTML/tree/lint first.
    // tbl_data allocates spans for standalone rules, layout rules and empty
    // data rows; term_tbl renders them even when they have no native cells.
    for case in cases()
        .into_iter()
        .filter(|case| case.get("rowKinds").is_some())
    {
        let (expected, native, document) = parsed(case["source"].as_str().unwrap());
        let (observed, lowered) = ir_profile(&document);
        let expected_kinds = case["rowKinds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            native
                .table_rows
                .iter()
                .map(|row| row_kind(&row.kind))
                .collect::<Vec<_>>(),
            expected_kinds,
            "{}",
            case["id"]
        );
        assert_eq!(expected.table_rows, observed.table_rows, "{}", case["id"]);
        let mut violations = Vec::new();
        compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
        assert_eq!(violations, Vec::<String>::new(), "{}", case["id"]);
    }
}

fn two_tables() -> (AstTopology, IrTopology) {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "two-tables")
        .unwrap();
    let (_, native, document) = parsed(case["source"].as_str().unwrap());
    (native, ir_profile(&document).1)
}

#[test]
fn deleting_a_rule_cannot_shift_cells_or_be_hidden_by_another_table() {
    let (native, mut lowered) = two_tables();
    lowered.table_rows.remove(1);
    let mut violations = Vec::new();
    compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected 3 rows, observed 2"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected kind HorizontalRule"))
    );
}

#[test]
fn missing_tables_and_repeated_source_origins_remain_observable() {
    let (native, mut lowered) = two_tables();
    let origin = (
        lowered.table_rows[0].table_source_line,
        lowered.table_rows[0].table_source_column,
    );
    for row in lowered.table_rows.iter_mut().skip(3) {
        row.table_source_line = origin.0;
        row.table_source_column = origin.1;
    }
    let mut violations = Vec::new();
    compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("ambiguous repeated table source identity"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected table, observed none"))
    );
}

#[test]
fn layout_rule_strength_and_empty_data_are_not_interchangeable() {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "layout-rule")
        .unwrap();
    let (_, native, document) = parsed(case["source"].as_str().unwrap());
    let (_, mut lowered) = ir_profile(&document);
    lowered.table_rows[1].kind = mant_ir::TableRowKind::LayoutRule {
        cells: vec![mant_ir::TableRuleCellKind::Horizontal; 2],
    };
    let mut violations = Vec::new();
    compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected kind LayoutRule"))
    );
    lowered.table_rows[1].kind = mant_ir::TableRowKind::Data;
    violations.clear();
    compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected kind LayoutRule"))
    );
}

#[test]
fn bounded_item_census_keeps_actual_owner_witnesses() {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "item-census")
        .unwrap();
    let source = case["source"].as_str().unwrap();
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let document =
        mant_codec::parse_roff_bytes(std::path::Path::new("audit.1"), source.as_bytes()).unwrap();
    let census = serde_json::to_value(structure_items::item_census(
        &report.document.root,
        &document,
    ))
    .unwrap();
    assert_eq!(census["nativeTotal"], 3);
    assert_eq!(census["irTotal"], 3);
    assert_eq!(census["nativeOmitted"], 0);
    assert_eq!(census["irOmitted"], 0);
    let rows = census["native"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["origin"]["line"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [3, 5, 7]
    );
    for (row, body) in rows.iter().zip(["FirstBody", "BulletBody", "SecondBody"]) {
        assert_eq!(row["body"], body);
        assert!(!row["ancestorIds"].as_array().unwrap().is_empty());
    }
}
