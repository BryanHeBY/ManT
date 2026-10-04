//! Cell boundaries preserve hard rows without inventing a second text root.
use super::*;

fn cell(value: &str, break_after: bool, completed: u16) -> TableCell {
    let mut blocks = if value.is_empty() {
        Vec::new()
    } else {
        vec![paragraph(vec![Inline::Text {
            value: value.into(),
        }])]
    };
    if completed > 0 {
        blocks.push(Block::VerticalSpace {
            lines: completed,
            source: None,
        });
    }
    TableCell {
        break_after,
        kind: mant_ir::TableCellKind::Text,
        blocks,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

fn table(cells: Vec<TableCell>) -> Block {
    Block::Table {
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn cell_blocks(blocks: Vec<Block>, break_after: bool) -> TableCell {
    TableCell {
        blocks,
        ..cell("", break_after, 0)
    }
}

fn gap(rows: u16) -> Block {
    Block::VerticalSpace {
        lines: rows,
        source: None,
    }
}

fn owned(value: &str, id: &str) -> Block {
    Block::List {
        kind: ListKind::Plain,
        items: vec![ListItem {
            blocks: vec![paragraph(vec![Inline::Text {
                value: value.into(),
            }])],
            entry: Some(EntryFacts {
                id: id.into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: Vec::new(),
                forms: Vec::new(),
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                value_domain: None,
            }),
            layout: mant_ir::ListItemLayout::default(),
            source: None,
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn literal(value: &str) -> Block {
    Block::Preformatted {
        children: vec![Inline::Text {
            value: value.into(),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn wrap(block: Block, kind: &str) -> Block {
    match kind {
        "table" => table(vec![cell_blocks(vec![block], false)]),
        "list" => Block::List {
            kind: ListKind::Plain,
            items: vec![ListItem {
                blocks: vec![block],
                layout: mant_ir::ListItemLayout::default(),
                entry: None,
                source: None,
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        },
        "definition" => Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                terms: vec![
                    vec![Inline::Text {
                        value: "TERM".into(),
                    }]
                    .into(),
                ],
                description: vec![block],
                head_body_relation: mant_ir::HeadBodyRelation::Separate,
                layout: mant_ir::DefinitionLayout::default(),
                entry: None,
                source: None,
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        },
        _ => unreachable!(),
    }
}

fn literal_readback(block: &Block) -> String {
    let markdown = render_blocks_fragment(
        std::slice::from_ref(block),
        MarkdownFragmentOptions::default(),
    )
    .join("\n\n");
    let parsed = parse_content(&markdown, None).unwrap();
    let [Block::Preformatted { children, .. }] =
        parsed.document.as_ref().unwrap().blocks.as_slice()
    else {
        panic!("one literal table readback: {markdown}");
    };
    mant_ir::inline_plain_text(children)
}

fn assert_readback(block: &Block, expected: &str) {
    assert_eq!(literal_readback(block), expected);
    let wire = serde_json::to_string(block).unwrap();
    let restored: Block = serde_json::from_str(&wire).unwrap();
    assert_eq!(&restored, block);
    assert_eq!(literal_readback(&restored), expected, "{wire}");
}

fn assert_mapped_readback(block: &Block, expected: &str, owners: &[(&str, &str)]) {
    let wire = serde_json::to_string(block).unwrap();
    let restored: Block = serde_json::from_str(&wire).unwrap();
    assert_eq!(&restored, block);
    for block in [block, &restored] {
        assert_eq!(literal_readback(block), expected);
        let blocks = std::slice::from_ref(block);
        let original = mant_ir::content_entries(blocks);
        let rendered =
            super::blocks::render_blocks_with_entries(blocks, MarkdownOptions::default(), true);
        assert_eq!(rendered.entries.len(), owners.len());
        for (id, text) in owners {
            let mapped = rendered
                .entries
                .iter()
                .find(|entry| entry.owner.facts().unwrap().id.as_str() == *id)
                .unwrap();
            let source = original
                .iter()
                .find(|entry| entry.owner().facts().unwrap().id.as_str() == *id)
                .unwrap();
            assert!(std::ptr::eq(
                mapped.owner.facts().unwrap(),
                source.owner().facts().unwrap()
            ));
            // Fixed fence prefix is four bytes. Independent expected payload
            // positions use UTF-8 bytes, never scalar/display-cell counts.
            let start = 4 + expected.find(*text).unwrap();
            assert_eq!(mapped.start..mapped.end, start..start + text.len());
            assert_eq!(&rendered.text[mapped.start..mapped.end], *text);
        }
    }
}

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
    first.column_span = u16::MAX;
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
