//! Existing regressions grouped by tables behavior; expected values remain independent.
use super::*;

#[test]
fn fit_definition_uses_the_allocated_table_cell_width() {
    let definition_cell = |body_indent_columns| TableCell {
        kind: mant_ir::TableCellKind::Text,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            compact: true,
            items: vec![DefinitionItem {
                source: None,
                entry: None,
                terms: vec![vec![Inline::Text {
                    value: "12345678".into(),
                }]],
                description: vec![paragraph("BODY")],
                layout: mant_ir::DefinitionLayout {
                    placement: mant_ir::DefinitionPlacement::Fit,
                    body_indent_columns,
                    min_term_gap_columns: 2,
                    spacing_before_lines: None,
                },
            }],
            layout: LayoutHint::default(),
            source: None,
        }],
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                definition_cell(12),
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph("NEIGHBOUR")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
            ],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&query);
    let narrow = view.render(22);
    let wide = view.render(80);
    assert!(
        narrow.search("BODY")[0].row > narrow.search("12345678")[0].row,
        "{:?}",
        narrow.text
    );
    assert_eq!(wide.search("BODY")[0].row, wide.search("12345678")[0].row);
    assert_eq!(narrow.search("NEIGHBOUR").len(), 1);
    assert_eq!(wide.search("NEIGHBOUR").len(), 1);
}

#[test]
fn signed_table_cells_preserve_real_origins_links_and_anchors() {
    for (table_indent, child_indent, expected_column) in
        [(-2, 3, 1), (3, -2, 1), (4096, 3, 4096), (4090, 10, 4096)]
    {
        let cell = |id: &str, text: &str| TableCell {
            kind: mant_ir::TableCellKind::Text,
            blocks: vec![Block::Paragraph {
                children: vec![
                    Inline::anchor_with_aliases(id, vec![format!("Exact.{id}").into()]),
                    Inline::Link {
                        target: mant_ir::LinkTarget::Section {
                            id: "description".into(),
                        },
                        title: None,
                        children: vec![Inline::Text { value: text.into() }],
                    },
                ],
                layout: LayoutHint {
                    indent_columns: child_indent,
                    ..Default::default()
                },
                source: None,
            }],
            column_span: 2,
            row_span: 1,
            alignment: None,
        };
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("first", "FIRST"), cell("second", "SECOND")],
            }],
            layout: LayoutHint {
                indent_columns: table_indent,
                ..Default::default()
            },
            source: None,
        }];
        document.sections.clear();
        let rendered = DocumentView::new(&query).render((expected_column + 20).max(80));
        let expected_column = usize::from(expected_column);
        let rows = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        for (id, text) in [("first", "FIRST"), ("second", "SECOND")] {
            let found = rendered.search(text);
            assert_eq!(found.len(), 1, "{rows:?}");
            let row = found[0].row;
            assert_eq!(rows[row].find(text), Some(expected_column), "{rows:?}");
            assert_eq!(rendered.anchor_row(id), Some(row));
            assert_eq!(rendered.anchor_row(&format!("Exact.{id}")), Some(row));
            assert_eq!(
                rendered.link_target_at(row, expected_column),
                Some(&LinkTarget::Section("description".into()))
            );
        }
        assert!(rendered.search("FIRST")[0].row < rendered.search("SECOND")[0].row);
    }
}

#[test]
fn table_cells_use_shared_content_driven_columns_and_independent_wrapping() {
    let mut bundle = bundle();
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph("alpha beta gamma")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph("right hand")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
            ],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&bundle).render(24);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rows[1].trim_end(), "   alpha       right", "{rows:#?}");
    assert_eq!(rows[2].trim_end(), "   beta gamma  hand", "{rows:#?}");
    assert_eq!(UnicodeWidthStr::width(rows[1].as_str()), 24);
    let left_match = rendered.search("alpha beta gamma");
    assert_eq!(left_match.len(), 1);
    assert_eq!(left_match[0].row, 1);
    assert_eq!(left_match[0].additional_fragments[0].row, 2);
    assert_eq!(rendered.search("right hand").len(), 1);
}

#[test]
fn short_table_keys_do_not_claim_half_of_a_wide_viewport() {
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |value: &str| TableCell {
        kind: mant_ir::TableCellKind::Text,
        blocks: vec![paragraph(value)],
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("1"), cell("Executable programs and shell commands")],
            },
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("8"), cell("System administration commands")],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rows = DocumentView::new(&bundle)
        .render(80)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rows[1], "   1  Executable programs and shell commands");
    assert_eq!(rows[2].trim_end(), "   8  System administration commands");
    assert!(UnicodeWidthStr::width(rows[2].as_str()) < 50);
}

#[test]
fn empty_and_ruled_table_rows_keep_distinct_terminal_surfaces() {
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let data = |value: &str| TableRow {
        kind: mant_ir::TableRowKind::Data,
        cells: vec![TableCell {
            kind: mant_ir::TableCellKind::Text,
            blocks: vec![paragraph(value)],
            column_span: 1,
            row_span: 1,
            alignment: None,
        }],
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![
            data("BEFORE"),
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: Vec::new(),
            },
            TableRow {
                kind: mant_ir::TableRowKind::HorizontalRule,
                cells: Vec::new(),
            },
            TableRow {
                kind: mant_ir::TableRowKind::DoubleHorizontalRule,
                cells: Vec::new(),
            },
            data("AFTER"),
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rows = DocumentView::new(&bundle)
        .render(24)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let before = rows.iter().position(|row| row.contains("BEFORE")).unwrap();
    assert!(rows[before + 1].is_empty(), "{rows:#?}");
    assert!(
        rows[before + 2].trim().chars().all(|ch| ch == '─'),
        "{rows:#?}"
    );
    assert!(
        rows[before + 3].trim().chars().all(|ch| ch == '═'),
        "{rows:#?}"
    );
    assert!(rows[before + 4].contains("AFTER"), "{rows:#?}");
}

#[test]
fn partial_rule_cells_remain_visible_beside_text_cells() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    kind: mant_ir::TableCellKind::HorizontalRule,
                    blocks: Vec::new(),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![Block::Paragraph {
                        children: vec![Inline::Text {
                            value: "VISIBLE".to_owned(),
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
            ],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rows = DocumentView::new(&bundle)
        .render(40)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let row = rows
        .iter()
        .find(|row| row.contains("VISIBLE"))
        .expect("visible table row");
    assert!(row.contains('─'), "{rows:#?}");
    assert!(
        row.find('─').unwrap() < row.find("VISIBLE").unwrap(),
        "{rows:#?}"
    );
}

#[test]
fn stacked_partial_rule_cells_are_not_dropped() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    kind: mant_ir::TableCellKind::HorizontalRule,
                    blocks: Vec::new(),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![Block::Paragraph {
                        children: vec![Inline::Text {
                            value: "VISIBLE".to_owned(),
                        }],
                        layout: LayoutHint {
                            indent_columns: -1,
                            ..LayoutHint::default()
                        },
                        source: None,
                    }],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
            ],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rows = DocumentView::new(&bundle)
        .render(40)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert!(
        rows.iter()
            .any(|row| !row.trim().is_empty() && row.trim().chars().all(|ch| ch == '─')),
        "{rows:#?}"
    );
    assert!(rows.iter().any(|row| row.contains("VISIBLE")), "{rows:#?}");
}

#[test]
fn rule_rows_do_not_split_table_wide_column_measurement() {
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |value: &str| TableCell {
        kind: mant_ir::TableCellKind::Text,
        blocks: vec![paragraph(value)],
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("A"), cell("FIRST")],
            },
            TableRow {
                kind: mant_ir::TableRowKind::LayoutRule {
                    cells: vec![
                        mant_ir::TableRuleCellKind::Horizontal,
                        mant_ir::TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                cells: Vec::new(),
            },
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("LONG LEFT COLUMN"), cell("SECOND")],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rows = DocumentView::new(&bundle)
        .render(60)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let first = rows.iter().find(|row| row.contains("FIRST")).unwrap();
    let second = rows.iter().find(|row| row.contains("SECOND")).unwrap();
    assert_eq!(first.find("FIRST"), second.find("SECOND"), "{rows:#?}");
    let rule = rows
        .iter()
        .find(|row| row.contains('─') && row.contains('═'))
        .expect("mixed layout rule");
    assert!(
        rule.find('─').unwrap() < rule.find('═').unwrap(),
        "{rows:#?}"
    );
}

#[test]
fn narrow_tables_stack_cells_instead_of_dropping_content() {
    let cells = vec![
        LogicalTableCell::new(vec![LogicalLine::plain(0, "a", Style::default())], None),
        LogicalTableCell::new(vec![LogicalLine::plain(0, "b", Style::default())], None),
    ];
    let layout = Arc::new(LogicalTableLayout::for_rows(std::slice::from_ref(&cells)));
    let line = LogicalLine::table(0, cells, layout);

    let rows = wrap_line(&line, 1);
    assert_eq!(
        rows.iter().map(ToString::to_string).collect::<String>(),
        "ab"
    );
}

fn tabbed_fit_definition(body_indent_columns: i32) -> Block {
    tabbed_fit_definition_with_children(
        body_indent_columns,
        vec![
            Inline::anchor("body-target"),
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "description".into(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: "BODY".into(),
                }],
            },
        ],
    )
}

fn tabbed_fit_definition_with_children(body_indent_columns: i32, children: Vec<Inline>) -> Block {
    Block::DefinitionList {
        declaration_groups: Vec::new(),
        compact: true,
        items: vec![DefinitionItem {
            source: None,
            entry: None,
            terms: vec![vec![Inline::Text {
                value: "a\tb".into(),
            }]],
            description: vec![Block::Paragraph {
                children,
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: mant_ir::DefinitionLayout {
                placement: mant_ir::DefinitionPlacement::Fit,
                body_indent_columns,
                min_term_gap_columns: 1,
                spacing_before_lines: None,
            },
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn text_cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        kind: mant_ir::TableCellKind::Text,
        blocks,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

fn aligned_text_cell(blocks: Vec<Block>, alignment: mant_ir::TableAlignment) -> TableCell {
    TableCell {
        alignment: Some(alignment),
        ..text_cell(blocks)
    }
}

#[test]
fn fit_definition_in_second_column_uses_absolute_tab_origin_and_keeps_payload() {
    // Pinned CVS term.c::term_fill() and term_tab.c::term_tab_next() advance
    // tabs from the device column, not from the start of a tbl cell.
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                text_cell(vec![paragraph("KEY")]),
                text_cell(vec![tabbed_fit_definition(6)]),
            ],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&query).render(20);
    let term_matches = rendered.search("a");
    let body_matches = rendered.search("BODY");
    assert!(!term_matches.is_empty(), "{:?}", rendered.text);
    assert!(!body_matches.is_empty(), "{:?}", rendered.text);
    let term = &term_matches[0];
    let body = &body_matches[0];
    assert_eq!(term.row, body.row, "{:?}", rendered.text);
    assert_eq!(body.start_column, 11, "{:?}", rendered.text);
    assert_eq!(rendered.anchor_row("body-target"), Some(body.row));
    assert_eq!(
        rendered.link_target_at(body.row, body.start_column),
        Some(&LinkTarget::Section("description".into()))
    );
}

#[test]
fn nested_second_column_resolves_tabs_from_the_outer_absolute_origin() {
    let nested = Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                text_cell(vec![paragraph("Q")]),
                text_cell(vec![tabbed_fit_definition(8)]),
            ],
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![text_cell(vec![paragraph("KEY")]), text_cell(vec![nested])],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&query).render(40);
    assert!(
        rendered.search("BODY")[0].row > rendered.search("a")[0].row,
        "{:?}",
        rendered.text
    );
}

#[test]
fn stacked_table_fallback_keeps_the_translated_tab_origin() {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                text_cell(vec![paragraph("LONGKEY8")]),
                text_cell(vec![tabbed_fit_definition(8)]),
            ],
        }],
        layout: LayoutHint {
            indent_columns: 3,
            ..Default::default()
        },
        source: None,
    }];

    let rendered = DocumentView::new(&query).render(14);
    assert_eq!(
        rendered.search("a")[0].row,
        rendered.search("BODY")[0].row,
        "{:?}",
        rendered.text
    );
}

#[test]
fn aligned_fit_definition_resolves_from_its_padded_absolute_origin() {
    // The exact literal-tab probe was checked with the pinned CVS renderer.
    // `term_flushln()` measures and aligns one field, while `term_tab_next()`
    // advances from that field's effective device position. The IR-only Fit
    // carrier below adds the consumer-side conditional-layout threshold.
    for (alignment, expected_b_column, expect_run_in) in [
        (mant_ir::TableAlignment::Right, 16, false),
        (mant_ir::TableAlignment::Center, 8, true),
    ] {
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![Block::Table {
            rows: vec![
                TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: vec![text_cell(vec![paragraph("123456789012345678901")])],
                },
                TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: vec![aligned_text_cell(
                        vec![tabbed_fit_definition(10)],
                        alignment,
                    )],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];

        let rendered = DocumentView::new(&query).render(21);
        let term = &rendered.search("b")[0];
        let body = &rendered.search("BODY")[0];
        assert_eq!(term.start_column, expected_b_column, "{:?}", rendered.text);
        assert_eq!(term.row == body.row, expect_run_in, "{:?}", rendered.text);
        if alignment == mant_ir::TableAlignment::Right {
            assert_eq!(body.start_column + "BODY".len(), 21, "{:?}", rendered.text);
        }
        assert_eq!(rendered.anchor_row("body-target"), Some(body.row));
        assert_eq!(
            rendered.link_target_at(body.row, body.start_column),
            Some(&LinkTarget::Section("description".into()))
        );
    }
}

#[test]
fn later_hard_rows_cannot_choose_the_aligned_fit_branch_or_gap() {
    let definition = tabbed_fit_definition_with_children(
        10,
        vec![
            Inline::anchor("body-target"),
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "description".into(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: "BODY".into(),
                }],
            },
            Inline::LineBreak,
            Inline::anchor("later-target"),
            Inline::Text {
                value: "123456789012345678901".into(),
            },
        ],
    );
    for (alignment, expected_b_column, expect_run_in) in [
        (mant_ir::TableAlignment::Right, 16, false),
        (mant_ir::TableAlignment::Center, 8, true),
    ] {
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![aligned_text_cell(vec![definition.clone()], alignment)],
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        let rendered = DocumentView::new(&query).render(21);
        let term = &rendered.search("b")[0];
        let body = &rendered.search("BODY")[0];
        let later = &rendered.search("123456789012345678901")[0];
        assert_eq!(term.start_column, expected_b_column, "{:?}", rendered.text);
        assert_eq!(term.row == body.row, expect_run_in, "{:?}", rendered.text);
        // The later description row keeps the definition continuation
        // origin.  Its width must not participate in choosing the first-row
        // Fit/Stacked branch or the term-to-description gap.
        assert_eq!(later.start_column, 10, "{:?}", rendered.text);
        assert_eq!(rendered.anchor_row("body-target"), Some(body.row));
        assert_eq!(rendered.anchor_row("later-target"), Some(later.row));
        assert_eq!(
            rendered.link_target_at(body.row, body.start_column),
            Some(&LinkTarget::Section("description".into()))
        );
    }
}

#[test]
fn every_hard_row_gets_its_own_center_or_right_alignment() {
    // Pinned CVS tbl_term.c reapplies TERMP_CENTER/TERMP_RIGHT before each
    // term_flushln(); term.c computes vbl from that output row's own width.
    for (alignment, long_column, short_column) in [
        (mant_ir::TableAlignment::Right, 6, 9),
        (mant_ir::TableAlignment::Center, 3, 4),
    ] {
        let content = Block::Paragraph {
            children: vec![
                Inline::anchor("long-row"),
                Inline::Text {
                    value: "LONG\n".into(),
                },
                Inline::anchor("short-row"),
                Inline::Link {
                    target: mant_ir::LinkTarget::Section {
                        id: "description".into(),
                    },
                    title: None,
                    children: vec![Inline::Text { value: "X".into() }],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        };
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![Block::Table {
            rows: vec![
                TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: vec![text_cell(vec![paragraph("1234567890")])],
                },
                TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: vec![aligned_text_cell(vec![content], alignment)],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];

        let rendered = DocumentView::new(&query).render(10);
        let long = &rendered.search("LONG")[0];
        let short = &rendered.search("X")[0];
        assert_eq!(long.start_column, long_column, "{:?}", rendered.text);
        assert_eq!(short.start_column, short_column, "{:?}", rendered.text);
        assert_eq!(rendered.anchor_row("long-row"), Some(long.row));
        assert_eq!(rendered.anchor_row("short-row"), Some(short.row));
        assert_eq!(
            rendered.link_target_at(short.row, short.start_column),
            Some(&LinkTarget::Section("description".into()))
        );
    }
}
