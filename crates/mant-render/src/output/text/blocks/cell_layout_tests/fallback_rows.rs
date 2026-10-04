//! A fallback consumes physical receipts, not the count of prepared strings.
use super::*;

#[test]
fn early_and_late_column_fallbacks_preserve_the_same_cell_rows() {
    let cases = [
        (vec![], "A\nC", "A\nC"),
        (vec![paragraph(vec![])], "A\nC", "A\nC"),
        (
            vec![paragraph(vec![Inline::anchor("empty")])],
            "A\nC",
            "A\nC",
        ),
        (vec![literal(vec![])], "A\nC", "A\nC"),
        (vec![literal(vec![text("")])], "A\n\nC", "A\nC"),
        (vec![paragraph(vec![text("  ")])], "A\n  \nC", "A\n  \nC"),
        (
            vec![paragraph(vec![Inline::line_break()])],
            "A\n\nC",
            "A\n\n\nC",
        ),
        (
            vec![Block::VerticalSpace {
                lines: 1,
                source: None,
            }],
            "A\n\nC",
            "A\n\nC",
        ),
    ];
    for (index, (blocks, open, closed)) in cases.into_iter().enumerate() {
        for break_after in [false, true] {
            for paint in [plain as fn(TextPresentation, &str) -> String, ansi] {
                let middle = TableCell {
                    break_after,
                    ..cell(blocks.clone())
                };
                let cells = vec![
                    cell(vec![paragraph(vec![text("A")])]),
                    middle,
                    cell(vec![paragraph(vec![text("C")])]),
                ];
                for late in [false, true] {
                    let mut block = table(&[4096, 4], cells.clone(), 0);
                    let Block::Table {
                        column_preferences, ..
                    } = &mut block
                    else {
                        unreachable!()
                    };
                    // Source-neutral preferences force two distinct plan exits;
                    // the cells, authored rows and decoration remain identical.
                    *column_preferences = mant_ir::ColumnPreferences {
                        widths: if late { vec![4096, 4] } else { vec![4; 257] },
                        ..Default::default()
                    };
                    let wire = serde_json::to_string(&block).unwrap();
                    let restored: Block = serde_json::from_str(&wire).unwrap();
                    let (result, count) =
                        visits::observe(|| renderer(&paint).render_blocks(&[restored], 0));
                    assert_eq!(
                        undecorated(&result),
                        if break_after { closed } else { open },
                        "case={index}/closed={break_after}/late={late}"
                    );
                    assert_eq!(count.blocks, blocks.len() + 3, "each cell is visited once");
                }
            }
        }
    }
}

#[test]
fn empty_cells_at_the_edges_do_not_claim_rows_in_late_fallback() {
    for paint in [plain as fn(TextPresentation, &str) -> String, ansi] {
        for position in [0, 1, 2] {
            let mut cells = vec![cell(vec![]), cell(vec![]), cell(vec![])];
            cells[position] = cell(vec![paragraph(vec![text("BODY")])]);
            let mut block = table(&[4096, 4], cells, 0);
            let Block::Table {
                column_preferences, ..
            } = &mut block
            else {
                unreachable!()
            };
            column_preferences.advance_limit_columns = None;
            let output = renderer(&paint).render_blocks(&[block], 0);
            assert_eq!(undecorated(&output), "BODY");
        }
    }
}
