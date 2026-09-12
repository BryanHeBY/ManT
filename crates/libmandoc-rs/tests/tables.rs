//! Native tbl metadata must distinguish printable payload from layout controls.
use libmandoc_rs::{Node, Parser, TableCell, TableCellKind, TableRowKind, TableRuleCellKind};

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
