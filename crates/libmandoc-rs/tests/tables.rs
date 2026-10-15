//! Native tbl metadata must distinguish printable payload from layout controls.
use libmandoc_rs::{
    Node, Parser, TableAlignment, TableCell, TableCellKind, TableRowKind, TableRuleCellKind,
};

fn table_rows(node: &Node) -> Vec<&Node> {
    node.table_row_kind
        .iter()
        .map(|_| node)
        .chain(node.children.iter().flat_map(table_rows))
        .collect()
}

#[test]
fn whole_table_options_follow_span_identity_and_final_layout() {
    // Fixed CVS -Ttree/-Tutf8/-Tlint checked this exact input. tbl_opts.c
    // aliases centre to center, tbl_layout.c::cell_alloc grows the shared
    // opts.cols, and tbl_term.c uses those final options from the first span.
    let source = b".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.TS\ncentre;\nl.\n_\nAAA\n.T&\nl l l.\nBBB\tCCC\tDDD\n.TE\n.TS\nl.\nLAST\n.TE\n";
    let parsed = Parser::default().parse_bytes("table.1", source).unwrap();
    let rows = table_rows(&parsed.document.root);
    assert_eq!(rows.len(), 4);
    for row in &rows[..3] {
        assert_eq!(row.table_alignment, Some(TableAlignment::Center));
        assert_eq!(row.table_column_count, Some(3));
    }
    assert_eq!(rows[0].table_row_kind, Some(TableRowKind::HorizontalRule));
    assert!(rows[0].flags.table_start);
    assert!(!rows[1].flags.table_start);
    assert!(!rows[2].flags.table_start);
    assert_eq!(rows[3].table_alignment, None);
    assert_eq!(rows[3].table_column_count, Some(1));
    assert!(rows[3].flags.table_start);
    assert_eq!(parsed.document.root.table_alignment, None);
    assert_eq!(parsed.document.root.table_column_count, None);

    #[cfg(feature = "serde")]
    {
        let json = serde_json::to_string(&parsed.document.root).unwrap();
        let restored: Node = serde_json::from_str(&json).unwrap();
        let restored = table_rows(&restored);
        assert_eq!(restored.len(), rows.len());
        for (actual, expected) in restored.iter().zip(rows) {
            assert_eq!(actual.table_alignment, expected.table_alignment);
            assert_eq!(actual.table_column_count, expected.table_column_count);
        }
    }
}

#[test]
fn declared_empty_columns_do_not_create_synthetic_cells() {
    // Exact fixed CVS -Ttree/-Tutf8/-Tlint: three declared columns, one
    // actual tbl_dat. out.c::tblcalc retains the unoccupied trailing columns.
    let source =
        b".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.TS\ncenter;\nl l l.\nAAA\n.TE\n";
    let parsed = Parser::default().parse_bytes("table.1", source).unwrap();
    let rows = table_rows(&parsed.document.root);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].table_alignment, Some(TableAlignment::Center));
    assert_eq!(rows[0].table_column_count, Some(3));
    assert_eq!(rows[0].table_cells.len(), 1);
    assert_eq!(rows[0].table_cells[0].text.as_deref(), Some("AAA"));
}

#[test]
fn rule_only_tables_retain_options_without_a_data_row_carrier() {
    // Exact fixed CVS -Ttree/-Tascii/-Tutf8 retains both rule spans with
    // "3 center" options. -Tlint reports "tbl without any data cells";
    // the recoverable diagnostic does not discard the native table extent.
    let source =
        b".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.TS\ncenter;\nl l l.\n_\n=\n.TE\n";
    let parsed = Parser::default().parse_bytes("table.1", source).unwrap();
    let rows = table_rows(&parsed.document.root);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row.table_alignment, Some(TableAlignment::Center));
        assert_eq!(row.table_column_count, Some(3));
        assert_eq!(row.table_cells.len(), 0);
    }
    assert_eq!(rows[0].table_row_kind, Some(TableRowKind::HorizontalRule));
    assert_eq!(
        rows[1].table_row_kind,
        Some(TableRowKind::DoubleHorizontalRule)
    );
    assert!(rows[0].flags.table_start);
    assert!(!rows[1].flags.table_start);
}

#[test]
fn shorter_layout_rules_keep_their_strengths_after_column_growth() {
    // Exact fixed CVS -Ttree/-Tascii/-Tutf8/-Tlint: the first layout rule
    // remains mixed-strength, while its shared final opts declares 3 cols.
    // tbl_term.c walks layout cells separately from the table's column count.
    let source = b".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.TS\ncenter;\n_ =\nl l.\nA\tB\n.T&\nl l l.\nC\tD\tE\n.TE\n";
    let parsed = Parser::default().parse_bytes("table.1", source).unwrap();
    let rows = table_rows(&parsed.document.root);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].table_row_kind,
        Some(TableRowKind::LayoutRule {
            cells: vec![
                TableRuleCellKind::Horizontal,
                TableRuleCellKind::DoubleHorizontal,
            ],
        })
    );
    for row in &rows {
        assert_eq!(row.table_column_count, Some(3));
        assert_eq!(row.table_alignment, Some(TableAlignment::Center));
    }
}

fn cells(node: &Node) -> Vec<&TableCell> {
    node.table_cells
        .iter()
        .chain(node.children.iter().flat_map(cells))
        .collect()
}

fn escapes(node: &Node) -> Vec<Option<u8>> {
    let mut values = if node.table_cells.is_empty() {
        Vec::new()
    } else {
        vec![node.table_escape]
    };
    values.extend(node.children.iter().flat_map(escapes));
    values
}

#[test]
fn table_boundaries_survive_leading_rules_and_layout_restarts() {
    fn rows(node: &Node) -> Vec<(TableRowKind, bool)> {
        let mut result = Vec::new();
        if let Some(kind) = &node.table_row_kind {
            result.push((kind.clone(), node.flags.table_start));
        }
        result.extend(node.children.iter().flat_map(rows));
        result
    }
    let source = b".TH PROBE 1\n.SH DESCRIPTION\n.TS\nl.\nFIRST\n.T&\nr.\nSECOND\n.TE\n.TS\nl.\n_\nTHIRD\nFOURTH\n.TE\n";
    let parsed = Parser::default().parse_bytes("table.1", source).unwrap();
    assert_eq!(
        rows(&parsed.document.root),
        [
            (TableRowKind::Data, true),
            (TableRowKind::Data, false),
            (TableRowKind::HorizontalRule, true),
            (TableRowKind::Data, false),
            (TableRowKind::Data, false),
        ]
    );
}

#[test]
fn layout_only_rule_rows_retain_per_column_strength() {
    fn rows(node: &Node) -> Vec<TableRowKind> {
        let mut result = node.table_row_kind.iter().cloned().collect::<Vec<_>>();
        result.extend(node.children.iter().flat_map(rows));
        result
    }
    let source = b".TH PROBE 1\n.SH DESCRIPTION\n.TS\n_\nl.\nSINGLE\n.TE\n.TS\n=\nl.\nDOUBLE\n.TE\n.TS\n_ =\nl l.\nLEFT\tRIGHT\n.TE\n.TS\n_.\nIGNORED\n.TE\n.TS\n=.\nIGNORED\n.TE\n.TS\n_ =.\nLEFT\tRIGHT\n.TE\n";
    let parsed = Parser::default().parse_bytes("table.1", source).unwrap();
    assert_eq!(
        rows(&parsed.document.root),
        [
            TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::Horizontal],
            },
            TableRowKind::Data,
            TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::DoubleHorizontal],
            },
            TableRowKind::Data,
            TableRowKind::LayoutRule {
                cells: vec![
                    TableRuleCellKind::Horizontal,
                    TableRuleCellKind::DoubleHorizontal,
                ],
            },
            TableRowKind::Data,
            TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::Horizontal],
            },
            TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::DoubleHorizontal],
            },
            TableRowKind::LayoutRule {
                cells: vec![
                    TableRuleCellKind::Horizontal,
                    TableRuleCellKind::DoubleHorizontal,
                ],
            },
        ]
    );
}

#[test]
fn layout_rules_override_payload_and_data_rules_retain_their_kind() {
    for (layout, payload, expected) in [
        ("_", "HIDDEN", TableCellKind::HorizontalRule),
        ("=", "HIDDEN", TableCellKind::DoubleHorizontalRule),
        ("l", "_", TableCellKind::HorizontalRule),
        ("l", "=", TableCellKind::DoubleHorizontalRule),
        ("l", r"\_", TableCellKind::IsolatedHorizontalRule),
        ("l", r"\=", TableCellKind::IsolatedDoubleHorizontalRule),
        ("l", r"\&_", TableCellKind::Text),
    ] {
        let source = format!(
            ".TH PROBE 1\n.SH DESCRIPTION\n.TS\nl {layout} l.\nLEFT\t{payload}\tRIGHT\n.TE\n"
        );
        let parsed = Parser::default()
            .parse_bytes("table.1", source.as_bytes())
            .unwrap();
        let cells = cells(&parsed.document.root);
        assert_eq!(cells.len(), 3, "{source}");
        assert_eq!(cells[1].kind, expected, "{source}");
        assert_eq!(cells[0].kind, TableCellKind::Text);
        assert_eq!(cells[2].kind, TableCellKind::Text);
    }
}

#[test]
fn table_rows_retain_the_native_executed_escape_state() {
    let source = b".TH PROBE 1\n.SH DESCRIPTION\n.de UNUSED\n.ec @\n..\n.TS\nl.\nDEFAULT \\\" hidden\n.TE\n.ec @\n.TS\nl.\nALTERNATE @\" hidden\n.TE\n.eo\n.TS\nl.\nDISABLED \\\" visible\n.TE\n";
    let parsed = Parser::default().parse_bytes("table.1", source).unwrap();
    assert_eq!(
        escapes(&parsed.document.root),
        [Some(b'\\'), Some(b'@'), Some(0)]
    );
}
