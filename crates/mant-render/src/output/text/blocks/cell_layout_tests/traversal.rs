use super::*;

#[test]
fn nested_columns_visit_each_block_and_fragment_once_for_plain_and_ansi() {
    // RV01's exact depth 0/1/2/4/8/12/16 sources were run with pristine
    // CVS before this assertion, including lint0 for all supported seeds.
    // termp_it_pre reads declaration widths; it does not re-render children.
    // This source-neutral counterpart directly counts the recursive entries,
    // independently of machine speed and without raising the supported depth.
    for depth in [0, 1, 2, 4, 8, 12, 16] {
        let block = nested(depth);
        let expected = std::iter::repeat_n("HEAD", depth)
            .chain(["LEAF"])
            .collect::<Vec<_>>()
            .join("\n");
        for decorate in [plain as fn(TextPresentation, &str) -> String, ansi] {
            let (output, counts) = visits::observe(|| {
                renderer(&decorate).render_blocks(std::slice::from_ref(&block), 0)
            });
            assert_eq!(undecorated(&output), expected, "depth={depth}");
            assert_eq!(counts.blocks, 2 * depth + 1, "depth={depth}");
            assert_eq!(counts.inline_fragments, depth + 1, "depth={depth}");
        }
    }
}

#[test]
fn sibling_subtrees_are_layout_once_even_in_topology_and_budget_fallbacks() {
    for variant in [
        "declared",
        "signed",
        "span",
        "rule",
        "width-budget",
        "cell-budget",
        "tbl",
    ] {
        let mut widths = vec![3, 3];
        let mut cells = vec![cell(vec![nested(4)]), cell(vec![nested(2)])];
        let origin = if variant == "signed" { -2 } else { 0 };
        match variant {
            "span" => cells[0].column_span = 2,
            "rule" => cells.push(TableCell {
                kind: TableCellKind::IsolatedHorizontalRule,
                ..cell(vec![])
            }),
            "width-budget" => widths = vec![3; 257],
            "cell-budget" => {
                cells.extend((2..257).map(|_| cell(vec![paragraph(vec![text("EXTRA")])])));
            }
            "tbl" => widths.clear(),
            _ => {}
        }
        let extras = usize::from(variant == "cell-budget") * 255;
        let block = table(&widths, cells, origin);
        let (plain_output, plain_counts) =
            visits::observe(|| renderer(&plain).render_blocks(std::slice::from_ref(&block), 0));
        let (styled_output, styled_counts) =
            visits::observe(|| renderer(&ansi).render_blocks(&[block], 0));
        assert_eq!(
            undecorated(&styled_output),
            plain_output,
            "variant={variant}"
        );
        assert_eq!(plain_counts.blocks, 15 + extras, "variant={variant}");
        assert_eq!(
            plain_counts.inline_fragments,
            8 + extras,
            "variant={variant}"
        );
        assert_eq!(styled_counts, plain_counts, "variant={variant}");
        assert_eq!(plain_output.matches("LEAF").count(), 2, "variant={variant}");
    }
}

#[test]
fn defensive_placement_fallback_reuses_prepared_rows_without_a_tree_access() {
    use mant_ir::geometry::{ColumnFieldWidth, DeclaredColumns};
    let cells = vec![
        super::super::tables::PreparedCell::new(
            Flow::text(LayoutText::decorated(
                "LEFT",
                ansi(TextPresentation::default(), "LEFT"),
            )),
            false,
        ),
        super::super::tables::PreparedCell::new(
            Flow::text(LayoutText::decorated(
                "RIGHT",
                ansi(TextPresentation::default(), "RIGHT"),
            )),
            false,
        ),
    ];
    // This inconsistent measurement cannot come from from_text. A defensive
    // plan failure nevertheless receives only already prepared text results,
    // with no renderer/IR reference from which to repeat cell traversal.
    let widths = vec![
        vec![ColumnFieldWidth {
            content: 5,
            output: 4,
            completed: false,
        }],
        vec![ColumnFieldWidth::from_text("RIGHT")],
    ];
    let (output, counts) = visits::observe(|| {
        let columns = DeclaredColumns::new(&mant_ir::ColumnPreferences {
            widths: vec![3, 3],
            gap_columns: 4,
            advance_limit_columns: Some(256),
            extra_width_columns: Some(10),
        })
        .unwrap();
        BlockRenderer::placed_column_row(columns.place_at(&widths, 2), cells, &widths, 2)
            .finish(false)
    });
    assert_eq!(undecorated(&output), "  LEFT\n  RIGHT");
    assert_eq!(counts, visits::Counts::default());
}
