//! The owned tbl solver is bounded and usable without a renderer feature.
use libmandoc_rs::{Node, Parser, TableLayoutKind, measure_table_columns};

fn rows(node: &Node) -> Vec<&Node> {
    node.table_row_kind
        .iter()
        .map(|_| node)
        .chain(node.children.iter().flat_map(rows))
        .collect()
}

fn parsed() -> Node {
    // This exact source's pristine five profiles preceded the fixture. The
    // final bare z has no strlen width; tblcalc retains the declared column.
    Parser::default()
        .parse_bytes(
            "measure.1",
            include_bytes!("fixtures/table-execution/tx01a.1"),
        )
        .unwrap()
        .document
        .root
}

#[test]
fn native_widths_keep_declared_columns_and_their_layout_identity() {
    let root = parsed();
    let rows = rows(&root);
    assert_eq!(rows[0].table_column_count, Some(2));
    assert_eq!(
        rows[0].table_layout.as_ref().unwrap().cells[0].kind,
        TableLayoutKind::Left
    );
    let columns = measure_table_columns(&rows, 5 * 24, 78 * 24, |text| {
        if text == r"\z" { 0 } else { text.len() * 24 }
    })
    .unwrap();
    // out.c::tblcalc: every declared column is at least 1n; the ordinary
    // inter-column spacing is 3n. The solver never invents a semantic cell.
    assert_eq!(
        columns
            .iter()
            .map(|c| (c.width_basic_units, c.spacing_columns))
            .collect::<Vec<_>>(),
        [(24, 3), (24, 3)]
    );
    assert_eq!(rows[0].table_cells.len(), 2);
}

#[test]
fn malformed_measurements_and_callback_panics_return_errors_across_ffi() {
    let root = parsed();
    let original = rows(&root);
    assert!(measure_table_columns(&original, 0, 78 * 24, |_| panic!("callback")).is_err());
    assert!(measure_table_columns(&original, 0, 78 * 24, |_| usize::MAX).is_err());
    let mut row = original[0].clone();
    row.table_column_count = Some(u32::MAX);
    assert!(measure_table_columns(&[&row], 0, 78 * 24, |_| unreachable!()).is_err());
    row = original[0].clone();
    std::sync::Arc::make_mut(row.table_layout.as_mut().unwrap()).width_edge_columns = 3;
    assert!(measure_table_columns(&[&row], 0, 78 * 24, |_| unreachable!()).is_err());
}

#[test]
fn span_solver_structural_work_is_bounded_before_any_native_measurement() {
    let root = parsed();
    let mut row = rows(&root)[0].clone();
    row.table_column_count = Some(1024);
    let mut cell = row.table_cells[0].clone();
    cell.column = 0;
    row.table_cells = (2..258)
        .map(|span| {
            let mut cell = cell.clone();
            cell.column_span = span;
            cell
        })
        .collect();
    let error = measure_table_columns(&[&row], 0, 78 * 24, |_| {
        unreachable!("budget must precede C")
    })
    .unwrap_err();
    assert!(error.contains("span-work budget"), "{error}");
}
