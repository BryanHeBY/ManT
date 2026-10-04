use super::*;

#[test]
fn author_blank_rows_separate_gap_budgets_in_actual_text_output() {
    let blocks = [
        Block::VerticalSpace {
            lines: 3000,
            source: None,
        },
        paragraph("\u{a0}", 0),
        Block::VerticalSpace {
            lines: 3000,
            source: None,
        },
        paragraph("BODY", 0),
    ];
    assert!(!mant_ir::geometry::has_bounded_gap(&blocks));
    let output = super::super::super::plain_renderer().render_blocks(&blocks, 0);
    let rows = output.lines().collect::<Vec<_>>();
    assert_eq!(rows.len(), 6002);
    assert_eq!(rows[3000], "\u{a0}");
    assert_eq!(rows.last(), Some(&"BODY"));
}

#[test]
fn an_empty_closed_data_row_consumes_its_incoming_gap_in_stacked_output() {
    for closed in [false, true] {
        let blocks = [
            Block::VerticalSpace {
                lines: 3000,
                source: None,
            },
            Block::Table {
                column_preferences: mant_ir::ColumnPreferences::default(),
                rows: vec![mant_ir::TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: vec![TableCell {
                        break_after: closed,
                        kind: mant_ir::TableCellKind::Text,
                        blocks: Vec::new(),
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    }],
                }],
                layout: LayoutHint {
                    indent_columns: -1,
                    ..LayoutHint::default()
                },
                source: None,
            },
            Block::VerticalSpace {
                lines: 3000,
                source: None,
            },
            paragraph("BODY", 0),
        ];
        assert!(!mant_ir::geometry::has_bounded_gap(&blocks));
        let output = super::super::super::plain_renderer().render_blocks(&blocks, 0);
        let rows = output.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), 6002);
        assert_eq!(rows.last(), Some(&"BODY"));
    }
}

#[test]
fn navigation_only_tables_do_not_reset_the_parent_gap_budget() {
    let renderer = super::super::super::plain_renderer();
    for origin in [-1, 0, 2] {
        let mut target = paragraph("", 0);
        let Block::Paragraph { children, .. } = &mut target else {
            unreachable!();
        };
        *children = vec![Inline::anchor("target")];
        let table = Block::Table {
            column_preferences: mant_ir::ColumnPreferences::default(),
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![target],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                }],
            }],
            layout: LayoutHint {
                indent_columns: origin,
                ..LayoutHint::default()
            },
            source: None,
        };
        let blocks = [
            Block::VerticalSpace {
                lines: 3000,
                source: None,
            },
            table,
            Block::VerticalSpace {
                lines: 3000,
                source: None,
            },
            paragraph("BODY", 0),
        ];
        assert!(mant_ir::geometry::has_bounded_gap(&blocks));
        let output = renderer.render_blocks(&blocks, 0);
        assert_eq!(output, format!("{}BODY", "\n".repeat(4096)));
    }
}

#[test]
fn origin_preserving_cells_keep_leading_gaps_in_the_parent_budget() {
    let mut body = paragraph("BODY", 3);
    let Block::Paragraph { layout, .. } = &mut body else {
        unreachable!();
    };
    layout.spacing_before_lines = 3000;
    let table = Block::Table {
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![TableCell {
                break_after: false,
                kind: mant_ir::TableCellKind::Text,
                blocks: vec![body],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        layout: LayoutHint {
            indent_columns: -2,
            spacing_before_lines: 3000,
            ..LayoutHint::default()
        },
        source: None,
    };
    assert!(mant_ir::geometry::has_bounded_gap(std::slice::from_ref(
        &table
    )));
    let output = super::super::super::plain_renderer().render_blocks(&[table], 0);
    assert_eq!(output, format!("{} BODY", "\n".repeat(4096)));
}

#[test]
fn ordered_labels_and_item_spacing_keep_hard_row_origins() {
    let renderer = super::super::super::plain_renderer();
    for compact in [false, true] {
        let list = Block::List {
            kind: ListKind::Ordered { start: Some(9) },
            compact,
            items: [
                ("FIRST\nNEXT", None),
                ("SECOND", Some(0)),
                ("THIRD", Some(2)),
                ("FOURTH", None),
            ]
            .into_iter()
            .map(|(text, spacing_before_lines)| ListItem {
                layout: mant_ir::ListItemLayout {
                    spacing_before_lines,
                },
                source: None,
                entry: None,
                blocks: vec![paragraph(text, 0)],
            })
            .collect(),
            layout: LayoutHint::default(),
            source: None,
        };
        let inherited_separator = if compact { "\n" } else { "\n\n" };
        assert_eq!(
            renderer.render_blocks(&[list], 2),
            format!(
                "  9. FIRST\n     NEXT\n  10. SECOND\n\n\n  11. THIRD{inherited_separator}  12. FOURTH"
            )
        );
    }
}

#[test]
fn resolved_gaps_cross_transparent_containers_and_precede_whole_items() {
    let renderer = super::super::super::plain_renderer();
    for rows in [0, 1, 2] {
        let mut body = paragraph("BODY\nNEXT", 0);
        if let Block::Paragraph { layout, .. } = &mut body {
            layout.spacing_before_lines = rows;
        }
        let mut list = plain_list(vec![body], 0);
        if let Block::List { kind, .. } = &mut list {
            *kind = ListKind::Bullet;
        }
        assert_eq!(
            renderer.render_blocks(&[list], 0),
            format!("{}• BODY\n  NEXT", "\n".repeat(usize::from(rows)))
        );
    }
    let mut container = plain_list(
        vec![
            Block::VerticalSpace {
                lines: 3000,
                source: None,
            },
            paragraph("AFTER", 0),
        ],
        0,
    );
    if let Block::List { layout, .. } = &mut container {
        layout.spacing_before_lines = 3000;
    }
    let blocks = [paragraph("BEFORE", 0), container];
    assert!(mant_ir::geometry::has_bounded_gap(&blocks));
    assert_eq!(
        renderer.render_blocks(&blocks, 0),
        format!("BEFORE{}AFTER", "\n".repeat(4097))
    );
}

#[test]
fn subtree_translation_is_applied_once_at_each_visible_leaf() {
    let blocks = vec![
        paragraph("PROSE", 0),
        plain_list(
            vec![
                paragraph("CHILD", 0),
                plain_list(vec![paragraph("DEEP", 1)], 2),
                Block::DefinitionList {
                    declaration_groups: vec![],
                    compact: false,
                    items: vec![DefinitionItem {
                        head_body_relation: mant_ir::HeadBodyRelation::Separate,
                        terms: vec![
                            vec![Inline::Text {
                                value: "TERM".into(),
                            }]
                            .into(),
                        ],
                        description: vec![paragraph("BODY", -2)],
                        source: None,
                        entry: None,
                        layout: mant_ir::DefinitionLayout::default(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                },
                Block::Table {
                    column_preferences: mant_ir::ColumnPreferences::default(),
                    rows: vec![mant_ir::TableRow {
                        kind: mant_ir::TableRowKind::Data,
                        cells: vec![TableCell {
                            break_after: false,
                            kind: mant_ir::TableCellKind::Text,
                            blocks: vec![paragraph("CELL", 1)],
                            column_span: 1,
                            row_span: 1,
                            alignment: None,
                        }],
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                },
            ],
            3,
        ),
    ];
    let original = blocks.clone();
    let renderer = super::super::super::plain_renderer();
    let baseline = renderer.render_blocks(&blocks, 0);
    for shift in [0, 2, 5] {
        assert_eq!(
            renderer.render_blocks(&blocks, shift),
            indent_lines(&baseline, padding(shift))
        );
    }
    assert_eq!(
        blocks, original,
        "presentation cannot compensate by mutating IR"
    );
    assert!(baseline.lines().any(|line| line == "      DEEP"));
    assert!(baseline.lines().any(|line| line == "     BODY"));
    assert_eq!(
        renderer.render_blocks(&[plain_list(vec![paragraph("OUTDENT", 3)], -2)], 0),
        " OUTDENT"
    );
}
