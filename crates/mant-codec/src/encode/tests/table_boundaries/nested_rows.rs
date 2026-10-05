//! Nested containers retain their accepted row and field boundaries.
use super::super::{Block, Inline, LayoutHint, ListItem, ListKind, paragraph};
use super::fixtures::{assert_readback, cell, cell_blocks, literal, table, wrap};

#[test]
fn literal_and_nonfinal_paragraph_tails_close_before_independent_gap_rows() {
    // Pure IR: a nonfinal Paragraph retires one bare open tail at its frame
    // boundary. Preformatted preserves that row; resolved gaps are independent.
    for is_literal in [false, true] {
        for gap in [1, 2] {
            for break_after in [false, true] {
                let first = if is_literal {
                    literal("A\n")
                } else {
                    paragraph(vec![Inline::Text {
                        value: "A\n".into(),
                    }])
                };
                let blocks = vec![
                    first,
                    Block::VerticalSpace {
                        lines: gap,
                        source: None,
                    },
                ];
                let expected = format!(
                    "A{}B",
                    "\n".repeat(usize::from(gap) + 1 + usize::from(is_literal))
                );
                assert_readback(
                    &table(vec![
                        cell_blocks(blocks.clone(), break_after),
                        cell("B", false, 0),
                    ]),
                    &expected,
                );
                assert_readback(
                    &table(vec![cell_blocks(blocks, break_after)]),
                    expected.strip_suffix('B').unwrap(),
                );
            }
        }
    }
}

#[test]
fn nested_tables_lists_and_definitions_keep_closed_and_completed_receipts() {
    // Separate labels own a hard row, including inside a portable table.
    // Fresh pristine nested Bl -ohang / It TERM / No A confirms TERM\nA
    // (mdoc_term.c::termp_it_post -> term_newln); no colon is authored.
    for kind in ["table", "list", "definition"] {
        for depth in [1, 2, 4] {
            for (closed, gap, boundary) in [
                (false, 0, " | "),
                (true, 0, "\n"),
                (false, 1, "\n\n"),
                (true, 1, "\n\n"),
                (false, 2, "\n\n\n"),
                (true, 2, "\n\n\n"),
            ] {
                let mut first = table(vec![cell("A", closed, gap)]);
                for _ in 0..depth {
                    first = wrap(first, kind);
                }
                let prefix = if kind == "definition" {
                    "TERM\n".repeat(depth)
                } else {
                    String::new()
                };
                let expected = format!("{prefix}A{boundary}B");
                let eof = if gap > 0 {
                    format!("{prefix}A{}", "\n".repeat(usize::from(gap) + 1))
                } else {
                    format!("{prefix}A")
                };
                assert_readback(&table(vec![cell_blocks(vec![first.clone()], false)]), &eof);
                assert_readback(
                    &table(vec![cell_blocks(vec![first], false), cell("B", false, 0)]),
                    &expected,
                );
            }
        }
    }
}

#[test]
fn nested_open_tails_and_explicit_empty_data_rows_keep_the_existing_spelling() {
    for depth in [1, 2, 4] {
        for (value, closed, expected) in [
            ("A\n", false, "A\n | B"),
            ("A\n", true, "A\n\nB"),
            ("", true, "\nB"),
        ] {
            let mut first = table(vec![cell(value, closed, 0)]);
            for _ in 0..depth {
                first = wrap(first, "list");
            }
            assert_readback(
                &table(vec![cell_blocks(vec![first], false), cell("B", false, 0)]),
                expected,
            );
        }
    }
}

#[test]
fn ordinary_nested_separators_remain_when_no_hard_boundary_was_accepted() {
    let list = Block::List {
        kind: ListKind::Plain,
        items: vec![ListItem {
            blocks: vec![
                paragraph(vec![Inline::Text { value: "A".into() }]),
                paragraph(vec![Inline::Text { value: "B".into() }]),
            ],
            layout: mant_ir::ListItemLayout::default(),
            entry: None,
            source: None,
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    };
    let nested = wrap(list, "definition");
    assert_readback(
        &table(vec![
            cell_blocks(
                vec![nested, paragraph(vec![Inline::Text { value: "C".into() }])],
                false,
            ),
            cell("D", false, 0),
        ]),
        "TERM\nA, B; C | D",
    );
}

#[test]
fn list_and_definition_frames_close_paragraph_tails_without_closing_literal_tails() {
    for kind in ["list", "definition"] {
        for depth in [1, 2, 4] {
            for is_literal in [false, true] {
                let mut first = if is_literal {
                    literal("A\n")
                } else {
                    paragraph(vec![Inline::Text {
                        value: "A\n".into(),
                    }])
                };
                for _ in 0..depth {
                    first = wrap(first, kind);
                }
                let prefix = if kind == "definition" {
                    "TERM\n".repeat(depth)
                } else {
                    String::new()
                };
                let tail = if is_literal { "\n | " } else { "\n" };
                assert_readback(
                    &table(vec![
                        cell_blocks(vec![first.clone()], false),
                        cell("B", false, 0),
                    ]),
                    &format!("{prefix}A{tail}B"),
                );
                let gap_tail = if is_literal { "\n\n\n" } else { "\n\n" };
                assert_readback(
                    &table(vec![
                        cell_blocks(
                            vec![
                                first,
                                Block::VerticalSpace {
                                    lines: 1,
                                    source: None,
                                },
                            ],
                            false,
                        ),
                        cell("B", false, 0),
                    ]),
                    &format!("{prefix}A{gap_tail}B"),
                );
            }
        }
    }
}
