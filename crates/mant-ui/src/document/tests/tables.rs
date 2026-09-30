//! Existing regressions grouped by tables behavior; expected values remain independent.
use super::*;

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
            column_widths: Vec::new(),
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
        column_widths: Vec::new(),
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
        column_widths: Vec::new(),
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
        column_widths: Vec::new(),
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
        column_widths: Vec::new(),
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
        column_widths: Vec::new(),
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
        column_widths: Vec::new(),
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
