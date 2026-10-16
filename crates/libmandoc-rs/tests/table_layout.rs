//! Native layout descriptors remain shared and independent of data owners.
use libmandoc_rs::{Node, Parser, TableBorderStyle};
use std::{path::Path, sync::Arc};

fn rows(node: &Node) -> Vec<&Node> {
    node.table_row_kind
        .iter()
        .map(|_| node)
        .chain(node.children.iter().flat_map(rows))
        .collect()
}

fn fixture(name: &str) -> Option<Node> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/table_layout")
        .join(format!("{name}.1"));
    if !path.exists() {
        // Published packages omit the repository-only original corpus.
        return None;
    }
    Some(
        Parser::default()
            .parse_bytes(&path, &std::fs::read(&path).unwrap())
            .unwrap()
            .document
            .root,
    )
}

#[test]
fn frames_and_layout_edges_survive_restarts_and_empty_columns() {
    // Every source was run through pristine -Ttree/-Tutf8/-Thtml/-Tlint
    // before these assertions. tbl_opts.c aliases frame and box; tbl_term.c
    // uses row.vert before the first slot and slot.vert after each slot.
    for (name, expected) in [
        ("frame-alias", TableBorderStyle::Single),
        ("doublebox", TableBorderStyle::Double),
    ] {
        let Some(root) = fixture(name) else { return };
        for row in rows(&root) {
            assert_eq!(row.table_frame, Some(expected));
            assert_eq!(row.table_grid, None);
        }
    }
    let Some(root) = fixture("vertical-boundaries") else {
        return;
    };
    let rows = rows(&root);
    let first = rows[0].table_layout.as_ref().unwrap();
    let second = rows[1].table_layout.as_ref().unwrap();
    assert_eq!(first.left_border, Some(TableBorderStyle::Single));
    assert_eq!(first.cells[0].right_border, Some(TableBorderStyle::Double));
    assert_eq!(second.left_border, Some(TableBorderStyle::Double));
    assert_eq!(second.cells[0].right_border, Some(TableBorderStyle::Single));
    assert!(!Arc::ptr_eq(first, second));
    let Some(root) = fixture("trailing-columns") else {
        return;
    };
    let rows = self::rows(&root);
    assert_eq!(rows[0].table_layout.as_ref().unwrap().cells.len(), 3);
    assert_eq!(rows[0].table_cells.len(), 1);
}

#[test]
fn explicit_widths_are_shared_layout_facts_not_hard_wrapped_text() {
    // tbl_layout.c::mods evaluates w(expr) into BU. out.c::tblcalc treats
    // these as minimum widths, while tblcalc_literal measures T{} words.
    let Some(root) = fixture("width-units") else {
        return;
    };
    let layout = rows(&root)[0].table_layout.as_ref().unwrap();
    assert_eq!(
        layout
            .cells
            .iter()
            .map(|cell| cell.minimum_width_basic_units)
            .collect::<Vec<_>>(),
        [240, 240]
    );
    let Some(root) = fixture("boxed-escape-paragraphs") else {
        return;
    };
    let rows = rows(&root);
    let paragraphs: Vec<_> = rows
        .iter()
        .filter(|row| row.table_cells.iter().any(|cell| cell.text_block))
        .collect();
    assert_eq!(paragraphs.len(), 4);
    for row in paragraphs {
        let layout = row.table_layout.as_ref().unwrap();
        assert_eq!(layout.cells[1].minimum_width_basic_units, 34 * 24);
        assert_eq!(layout.cells[2].minimum_width_basic_units, 34 * 24);
        assert!(
            row.table_cells[1]
                .text
                .as_ref()
                .unwrap()
                .contains("DESC_END_")
        );
        assert!(
            row.table_cells[2]
                .text
                .as_ref()
                .unwrap()
                .contains("MEAN_END_")
        );
    }
}

#[test]
fn repeated_layouts_share_storage_without_synthetic_data_cells() {
    let Some(root) = fixture("shared-layout") else {
        return;
    };
    let rows = rows(&root);
    assert_eq!(rows.len(), 3);
    let layout = rows[0].table_layout.as_ref().unwrap();
    assert_eq!(layout.cells.len(), 8);
    for row in &rows {
        assert!(Arc::ptr_eq(layout, row.table_layout.as_ref().unwrap()));
        assert_eq!(row.table_cells.len(), 1);
    }
    #[cfg(feature = "serde")]
    {
        let json = serde_json::to_string(&root).unwrap();
        let restored: Node = serde_json::from_str(&json).unwrap();
        assert_eq!(root, restored);
    }
}

#[test]
fn column_spans_cross_the_former_u16_ceiling_without_truncation() {
    // Exact generated sources first checked with pristine -Ttree/-Tlint:
    // tbl_dat.hspans counts following slots; the following cell is at span.
    // Avoid rendering a 65k-column native frame merely to verify topology.
    for span in [65_534_u32, 65_535, 65_536] {
        let source = format!(
            ".TH PROBE 1 \"October 16, 2026\"\n.SH DESCRIPTION\n.TS\nl{} l.\nSPAN\tRIGHT\n.TE\n",
            " s".repeat(span as usize - 1)
        );
        let parsed = Parser::default()
            .parse_bytes("span.1", source.as_bytes())
            .unwrap();
        let rows = rows(&parsed.document.root);
        assert_eq!(rows[0].table_column_count, Some(span + 1));
        assert_eq!(rows[0].table_cells[0].column_span, span);
        assert_eq!(rows[0].table_cells[1].column, span);
        assert_eq!(rows[0].table_cells[1].text.as_deref(), Some("RIGHT"));
    }
}
