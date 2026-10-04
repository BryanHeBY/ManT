//! Table rules regressions with unchanged source and geometry expectations.
use super::super::*;
use super::fixtures::generic_linked_cell;

#[test]
fn empty_and_ruled_table_rows_keep_distinct_terminal_surfaces() {
    let paragraph = |value: &str| Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let data = |value: &str| TableRow {
        kind: mant_ir::TableRowKind::Data,
        cells: vec![TableCell {
            break_after: false,
            kind: mant_ir::TableCellKind::Text,
            blocks: vec![paragraph(value)],
            column_span: 1,
            row_span: 1,
            alignment: None,
        }],
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        column_preferences: mant_ir::ColumnPreferences::default(),
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
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::HorizontalRule,
                    blocks: Vec::new(),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
                TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![Block::Paragraph {
                        inline_layout: mant_ir::InlineLayout::default(),
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
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::HorizontalRule,
                    blocks: Vec::new(),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
                TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![Block::Paragraph {
                        inline_layout: mant_ir::InlineLayout::default(),
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
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |value: &str| TableCell {
        break_after: false,
        kind: mant_ir::TableCellKind::Text,
        blocks: vec![paragraph(value)],
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        column_preferences: mant_ir::ColumnPreferences::default(),
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
fn content_derived_extreme_gap_data_and_rule_output_remains_bounded() {
    use mant_ir::TableAlignment;
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: (0..256)
                    .map(|_| generic_linked_cell("X", 0, TableAlignment::Left))
                    .collect(),
            },
            TableRow {
                cells: vec![],
                kind: mant_ir::TableRowKind::LayoutRule {
                    cells: vec![mant_ir::TableRuleCellKind::Horizontal; 256],
                },
            },
        ],
        column_preferences: mant_ir::ColumnPreferences {
            gap_columns: u16::MAX,
            ..Default::default()
        },
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&query).render(u16::MAX);
    assert_eq!(rendered.search("X").len(), 256);
    assert_eq!(
        rendered
            .text
            .lines
            .iter()
            .filter(|line| line.to_string() == "─")
            .count(),
        256
    );
    assert!(
        rendered
            .text
            .lines
            .iter()
            .map(|line| line.to_string().len())
            .sum::<usize>()
            < 2048
    );
}
