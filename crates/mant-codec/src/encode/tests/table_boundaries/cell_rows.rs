//! Occupied cell closure and completed empty rows remain separate.
use super::super::Block;
use super::fixtures::{assert_mapped_readback, cell, cell_blocks, literal_readback, owned, table};

#[test]
fn closed_occupied_cells_and_completed_empty_rows_have_distinct_readback() {
    // This pure IR contract follows the exact CVS A/.sp 1/2/Ta B sources:
    // term_vspace (term.c:489) closes an occupied field before any independent
    // empty row. breakAfter records only that closure, never extra whitespace.
    for (closed, completed, expected) in [
        (false, 0, "A | B"),
        (true, 0, "A\nB"),
        (false, 1, "A\n\nB"),
        (true, 1, "A\n\nB"),
        (false, 2, "A\n\n\nB"),
        (true, 2, "A\n\n\nB"),
    ] {
        let block = table(vec![cell("A", closed, completed), cell("B", false, 0)]);
        assert_eq!(literal_readback(&block), expected);
        let wire = serde_json::to_string(&block).unwrap();
        let restored: Block = serde_json::from_str(&wire).unwrap();
        assert_eq!(literal_readback(&restored), expected);
        assert_eq!(restored, block);
    }
}

#[test]
fn cell_closure_keeps_the_existing_empty_data_row_without_an_extra_eof_row() {
    assert_eq!(literal_readback(&table(vec![cell("A", true, 0)])), "A");
    assert_eq!(
        literal_readback(&table(vec![cell("", true, 0), cell("B", false, 0)])),
        "\nB"
    );
    let block = table(vec![cell("α中\u{a0}", true, 0), cell("`BODY`", false, 0)]);
    assert_eq!(literal_readback(&block), "α中\u{a0}\n`BODY`");
    for (value, expected) in [("A\n", "A\n\nB"), ("A\n\n", "A\n\n\nB")] {
        assert_eq!(
            literal_readback(&table(vec![cell(value, true, 0), cell("B", false, 0)])),
            expected
        );
    }
    // An empty cell can close the current occupied table row without adding
    // a new empty one. The pipe still represents that cell's topology.
    assert_eq!(
        literal_readback(&table(vec![
            cell("A", false, 0),
            cell("", true, 0),
            cell("B", false, 0),
        ])),
        "A | \nB"
    );
}

#[test]
fn sparse_cells_retain_the_same_hard_boundary_and_source_order() {
    let mut first = cell("A", true, 0);
    first.column_span = u32::from(u16::MAX);
    let block = table(vec![first, cell("B", false, 0)]);
    assert_eq!(literal_readback(&block), "column 1: A\ncolumn 65536: B");
}

#[test]
fn completed_cell_gaps_share_the_existing_bounded_output_budget() {
    let mut first = cell("A", true, u16::MAX);
    first.blocks.push(Block::VerticalSpace {
        lines: u16::MAX,
        source: None,
    });
    let output = literal_readback(&table(vec![first, cell("B", false, 0)]));
    assert_eq!(
        output,
        format!(
            "A{}B",
            "\n".repeat(usize::from(mant_ir::geometry::MAX_GAP_ROWS) + 1)
        )
    );
}

#[test]
fn completed_final_cell_rows_supply_the_next_data_row_boundary() {
    // The pinned mdoc column fixture puts two completed blank rows between
    // FIRST and AFTER. Its last D1 closes the field in termp_bl_post(); a new
    // It must reuse the row reached by that close rather than add another gap.
    for (completed, boundary) in [(0, "\n"), (1, "\n"), (3, "\n\n\n")] {
        for closed in [false, true] {
            let mut block = table(vec![
                cell_blocks(vec![owned("αFIRST", "first")], false),
                cell("", closed, completed),
            ]);
            let Block::Table { rows, .. } = &mut block else {
                unreachable!();
            };
            rows.push(mant_ir::TableRow {
                cells: vec![cell_blocks(vec![owned("中AFTER", "next")], false)],
                ..rows[0].clone()
            });
            assert_mapped_readback(
                &block,
                &format!("αFIRST | {boundary}中AFTER | "),
                &[("first", "αFIRST"), ("next", "中AFTER")],
            );
        }
    }
}
