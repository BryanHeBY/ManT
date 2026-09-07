//! Native tbl metadata must distinguish printable payload from layout controls.
use libmandoc_rs::{Node, Parser, TableCell, TableCellKind};

fn cells(node: &Node) -> Vec<&TableCell> {
    node.table_cells
        .iter()
        .chain(node.children.iter().flat_map(cells))
        .collect()
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
