//! Gap requests are consumed once at actual cell and data-row boundaries.
use super::super::{Block, Inline, paragraph};
use super::fixtures::{
    assert_mapped_readback, assert_readback, cell, cell_blocks, gap, literal, owned, table, wrap,
};

#[test]
fn transparent_gap_boundaries_share_one_output_budget() {
    let inner = wrap(
        Block::VerticalSpace {
            lines: u16::MAX,
            source: None,
        },
        "list",
    );
    let first = cell_blocks(
        vec![
            paragraph(vec![Inline::Text { value: "A".into() }]),
            Block::VerticalSpace {
                lines: u16::MAX,
                source: None,
            },
            inner,
        ],
        false,
    );
    assert_readback(
        &table(vec![first, cell("B", false, 0)]),
        &format!(
            "A{}B",
            "\n".repeat(usize::from(mant_ir::geometry::MAX_GAP_ROWS) + 1)
        ),
    );
}

#[test]
fn explicit_cell_closure_consumes_the_gap_budget_before_the_next_cell() {
    // Independent public facts: false keeps one 3000+3000 boundary capped
    // at 4096. True consumes the first 3000 rows before the next request;
    // the occupied α row contributes one delimiter, never another blank row.
    for break_after in [false, true] {
        for depth in [0, 1, 2] {
            let mut first = owned("α", "first");
            for _ in 0..depth {
                first = wrap(first, "list");
            }
            let block = table(vec![
                cell_blocks(vec![first, gap(3000)], break_after),
                cell_blocks(vec![gap(3000), owned("中BODY", "second")], false),
            ]);
            let rows = if break_after { 6001 } else { 4097 };
            let expected = format!("α{}中BODY", "\n".repeat(rows));
            assert_mapped_readback(&block, &expected, &[("first", "α"), ("second", "中BODY")]);
        }
    }
}

#[test]
fn empty_cell_gaps_remain_pending_until_an_explicit_or_whole_row_boundary() {
    for break_after in [false, true] {
        let block = table(vec![
            cell_blocks(vec![gap(3000)], break_after),
            cell_blocks(vec![gap(3000), owned("中BODY", "second")], false),
        ]);
        // There is no preceding occupied glyph row to close. The requests
        // themselves own the empty data rows, and closure adds no blank row.
        let rows = if break_after { 6000 } else { 4096 };
        let expected = format!("{}中BODY", "\n".repeat(rows));
        assert_mapped_readback(&block, &expected, &[("second", "中BODY")]);
        assert_readback(
            &table(vec![cell_blocks(vec![gap(3000)], break_after)]),
            &"\n".repeat(3000),
        );
    }
    for depth in [1, 2] {
        let mut first = table(vec![cell_blocks(
            vec![owned("α", "first"), gap(3000)],
            false,
        )]);
        for _ in 1..depth {
            first = wrap(first, "table");
        }
        let block = table(vec![
            cell_blocks(vec![first], false),
            cell_blocks(vec![gap(3000), owned("中BODY", "second")], false),
        ]);
        // Whole data rows also consume their completed request budget. A
        // surrounding false cell cannot reopen that nested physical row.
        let expected = format!("α{}中BODY", "\n".repeat(6001));
        assert_mapped_readback(&block, &expected, &[("first", "α"), ("second", "中BODY")]);
    }
}

#[test]
fn open_literal_rows_stay_distinct_from_the_gap_budget_at_cell_closure() {
    for is_literal in [false, true] {
        for break_after in [false, true] {
            for depth in [0, 1, 2] {
                let mut first = if is_literal {
                    literal("α\n")
                } else {
                    paragraph(vec![Inline::Text {
                        value: "α\n".into(),
                    }])
                };
                for _ in 0..depth {
                    first = wrap(first, "list");
                }
                let first = cell_blocks(vec![first, gap(3000)], break_after);
                let block = table(vec![
                    first.clone(),
                    cell_blocks(vec![gap(3000), owned("中BODY", "second")], false),
                ]);
                let rows = if break_after { 6001 } else { 4097 };
                let expected = format!("α{}中BODY", "\n".repeat(rows + usize::from(is_literal)));
                assert_mapped_readback(&block, &expected, &[("second", "中BODY")]);
                // EOF consumes only the first request, not a future gap or
                // an additional EndRow. Literal retains its authored open LF.
                assert_readback(
                    &table(vec![first]),
                    &format!("α{}", "\n".repeat(3001 + usize::from(is_literal))),
                );
            }
        }
    }
}

#[test]
fn navigation_only_rows_leave_the_same_pending_cell_budget_untouched() {
    let navigation = table(vec![cell_blocks(
        vec![paragraph(vec![Inline::Strong {
            children: vec![Inline::anchor("navigation")],
        }])],
        false,
    )]);
    let block = table(vec![
        cell_blocks(
            vec![owned("α", "first"), gap(3000), navigation, gap(3000)],
            false,
        ),
        cell_blocks(vec![owned("中BODY", "second")], false),
    ]);
    let expected = format!("α{}中BODY", "\n".repeat(4097));
    assert_mapped_readback(&block, &expected, &[("first", "α"), ("second", "中BODY")]);
}
