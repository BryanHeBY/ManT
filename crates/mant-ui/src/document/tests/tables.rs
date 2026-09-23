//! Existing regressions grouped by tables behavior; expected values remain independent.
use super::*;

#[test]
#[allow(clippy::too_many_lines)] // Full checked store, empty/rule cells, and both layout modes share one fixture.
fn empty_and_rule_cells_keep_typed_reveal_points_when_columns_stack() {
    use mant_ir::{
        ContentOwnerKind, ContentRootKind, ContentStoreBuilder, PointBoundary, Provenance,
    };

    // CVS tbl_term.c::term_tbl prints empty data cells and rule cells on the
    // current physical row; neither requires visible semantic text.
    let mut builder = ContentStoreBuilder::new();
    let points = (0..4)
        .map(|_| {
            let owner = builder.push_owner(ContentOwnerKind::TableCell, Provenance::Unknown);
            let root = builder.push_root(owner, ContentRootKind::Cell, Provenance::Unknown);
            builder.push_point(
                root,
                PointBoundary::BetweenAtoms { atom_boundary: 0 },
                0,
                Provenance::Unknown,
            )
        })
        .collect::<Vec<_>>();
    let wide_owner = builder.push_owner(ContentOwnerKind::TableCell, Provenance::Unknown);
    let wide_root = builder.push_root(wide_owner, ContentRootKind::Cell, Provenance::Unknown);
    let wide_text = builder.push_text(
        wide_root,
        "LONGWORD".to_owned(),
        None,
        mant_ir::ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let cell = |point, kind| TableCell {
        kind,
        point: Some(point),
        blocks: Vec::new(),
        column_span: 1,
        row_span: 1,
        alignment: None,
        source: None,
    };
    let mut empty_right = cell(points[0], mant_ir::TableCellKind::Text);
    empty_right.alignment = Some(mant_ir::TableAlignment::Right);
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sources[0].identity = SourceIdentity::Anonymous {
        name: "ui-table-points".to_owned(),
    };
    document.flow_mut().expect("Flow fixture").content_store = builder.finish();
    document.flow_mut().expect("Flow fixture").sections.clear();
    document.flow_mut().expect("Flow fixture").blocks = vec![Block::Table {
        fixed_view: None,
        rows: vec![
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    TableCell {
                        kind: mant_ir::TableCellKind::Text,
                        point: None,
                        blocks: vec![Block::Paragraph {
                            children: vec![Inline::Text { content: wide_text }],
                            layout: LayoutHint::default(),
                            source: None,
                        }],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                        source: None,
                    },
                    TableCell {
                        kind: mant_ir::TableCellKind::Text,
                        point: None,
                        blocks: Vec::new(),
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                        source: None,
                    },
                ],
            },
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    empty_right,
                    cell(points[1], mant_ir::TableCellKind::HorizontalRule),
                ],
            },
            TableRow {
                kind: mant_ir::TableRowKind::LayoutRule {
                    cells: vec![
                        mant_ir::TableRuleCellKind::Horizontal,
                        mant_ir::TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                cells: vec![
                    cell(points[2], mant_ir::TableCellKind::HorizontalRule),
                    cell(points[3], mant_ir::TableCellKind::DoubleHorizontalRule),
                ],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    assert!(mant_ir::validate_document(document).is_empty());

    let view = DocumentView::new(&query);
    let columns = view.render(40);
    let empty = columns.point_location(points[0]).expect("empty cell");
    let rule = columns.point_location(points[1]).expect("rule cell");
    assert_eq!(empty.0, rule.0);
    assert_eq!(empty.1, 0, "right alignment must not move cell identity");
    assert!(empty.1 < rule.1);
    let layout_single = columns.point_location(points[2]).expect("single rule");
    let layout_double = columns.point_location(points[3]).expect("double rule");
    assert_eq!(layout_single.0, layout_double.0);
    assert!(layout_single.0 > empty.0);
    assert!(layout_single.1 < layout_double.1);

    let stacked = view.render(1);
    let empty = stacked
        .point_location(points[0])
        .expect("stacked empty cell");
    let rule = stacked
        .point_location(points[1])
        .expect("stacked rule cell");
    assert!(empty.0 < rule.0);
    assert_eq!(empty.1, rule.1);
}

#[test]
fn origin_preserving_stack_keeps_empty_and_rule_cell_points() {
    use mant_ir::{
        ContentOwnerKind, ContentRootKind, ContentStoreBuilder, PointBoundary, Provenance,
    };

    // CVS tbl_term.c::term_tbl retains both cells under .in -2n and .in
    // 5000n; the UI origin-preserving fallback must retain their positions.
    for origin in [-2, 5000] {
        let mut builder = ContentStoreBuilder::new();
        let points = (0..2)
            .map(|_| {
                let owner = builder.push_owner(ContentOwnerKind::TableCell, Provenance::Unknown);
                let root = builder.push_root(owner, ContentRootKind::Cell, Provenance::Unknown);
                builder.push_point(
                    root,
                    PointBoundary::BetweenAtoms { atom_boundary: 0 },
                    0,
                    Provenance::Unknown,
                )
            })
            .collect::<Vec<_>>();
        let cell = |point, kind| TableCell {
            kind,
            point: Some(point),
            blocks: Vec::new(),
            column_span: 1,
            row_span: 1,
            alignment: None,
            source: None,
        };
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sources[0].identity = SourceIdentity::Anonymous {
            name: "ui-origin-table-points".to_owned(),
        };
        document.flow_mut().expect("Flow fixture").content_store = builder.finish();
        document.flow_mut().expect("Flow fixture").sections.clear();
        document.flow_mut().expect("Flow fixture").blocks = vec![Block::Table {
            fixed_view: None,
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    cell(points[0], mant_ir::TableCellKind::Text),
                    cell(points[1], mant_ir::TableCellKind::HorizontalRule),
                ],
            }],
            layout: LayoutHint {
                indent_columns: origin,
                ..LayoutHint::default()
            },
            source: None,
        }];
        assert!(mant_ir::validate_document(document).is_empty());
        assert!(mant_ir::geometry::table_requires_origin_preserving_stack(
            match &document.flow_mut().expect("Flow fixture").blocks[0] {
                Block::Table { rows, .. } => rows,
                _ => unreachable!(),
            },
            origin
        ));

        let rendered = DocumentView::new(&query).render(40);
        let empty = rendered
            .point_location(points[0])
            .expect("empty cell point");
        let rule = rendered.point_location(points[1]).expect("rule cell point");
        assert_eq!(rule.0, empty.0 + 1);
        assert_eq!(rule.1, empty.1);
        assert!(rule.1 < 40, "reveal column must be viewport-readable");
        assert!(rendered.text.lines[rule.0].to_string().contains('─'));
    }
}

#[test]
#[allow(clippy::too_many_lines)] // The checked nested store and both table levels form one coordinate fixture.
fn nested_table_point_follows_right_aligned_payload_not_parent_cell_boundary() {
    use mant_ir::{
        ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, PointBoundary,
        Provenance,
    };

    // This is a renderer-coordinate invariant for composed IR; pinned CVS
    // treats nested .TS inside a T{...T} cell as literal input, not a table.
    let mut builder = ContentStoreBuilder::new();
    let parent_owner = builder.push_owner(ContentOwnerKind::TableCell, Provenance::Unknown);
    let parent_root = builder.push_root(parent_owner, ContentRootKind::Cell, Provenance::Unknown);
    let parent_point = builder.push_point(
        parent_root,
        PointBoundary::BetweenAtoms { atom_boundary: 0 },
        0,
        Provenance::Unknown,
    );
    let child_owner = builder.push_owner(ContentOwnerKind::TableCell, Provenance::Unknown);
    let child_root = builder.push_root(child_owner, ContentRootKind::Cell, Provenance::Unknown);
    let child_point = builder.push_point(
        child_root,
        PointBoundary::BetweenAtoms { atom_boundary: 0 },
        0,
        Provenance::Unknown,
    );
    let child_text = builder.push_text(
        child_root,
        "X".to_owned(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let wide_owner = builder.push_owner(ContentOwnerKind::TableCell, Provenance::Unknown);
    let wide_root = builder.push_root(wide_owner, ContentRootKind::Cell, Provenance::Unknown);
    let wide_text = builder.push_text(
        wide_root,
        "LONGWORD".to_owned(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let paragraph = |content| Block::Paragraph {
        children: vec![Inline::Text { content }],
        layout: LayoutHint::default(),
        source: None,
    };
    let child = TableCell {
        kind: mant_ir::TableCellKind::Text,
        point: Some(child_point),
        blocks: vec![paragraph(child_text)],
        column_span: 1,
        row_span: 1,
        alignment: None,
        source: None,
    };
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sources[0].identity = SourceIdentity::Anonymous {
        name: "ui-nested-table-points".to_owned(),
    };
    document.flow_mut().expect("Flow fixture").content_store = builder.finish();
    document.flow_mut().expect("Flow fixture").sections.clear();
    document.flow_mut().expect("Flow fixture").blocks = vec![Block::Table {
        fixed_view: None,
        rows: vec![
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    point: Some(parent_point),
                    blocks: vec![Block::Table {
                        fixed_view: None,
                        rows: vec![TableRow {
                            kind: mant_ir::TableRowKind::Data,
                            cells: vec![child],
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    column_span: 1,
                    row_span: 1,
                    alignment: Some(mant_ir::TableAlignment::Right),
                    source: None,
                }],
            },
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    point: None,
                    blocks: vec![paragraph(wide_text)],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
                }],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    assert!(mant_ir::validate_document(document).is_empty());

    let rendered = DocumentView::new(&query).render(40);
    let parent = rendered
        .point_location(parent_point)
        .expect("parent boundary");
    let child = rendered
        .point_location(child_point)
        .expect("inner boundary");
    assert_eq!(parent.0, child.0);
    assert_eq!(parent.1, 0);
    assert_eq!(child.1, 7);
    assert_eq!(
        rendered.text.lines[child.0].to_string().find('X'),
        Some(child.1)
    );
}

#[test]
fn signed_table_cells_preserve_real_origins_links_and_anchors() {
    for (table_indent, child_indent, expected_column) in
        [(-2, 3, 1), (3, -2, 1), (4096, 3, 4096), (4090, 10, 4096)]
    {
        let cell = |id: &str, text: &str| TableCell {
            kind: mant_ir::TableCellKind::Text,
            point: None,
            blocks: vec![Block::Paragraph {
                children: vec![
                    crate::test_content::anchor_with_aliases(
                        id,
                        vec![format!("Exact.{id}").into()],
                    ),
                    crate::test_content::link(
                        mant_ir::LinkTarget::Section {
                            id: "description".into(),
                        },
                        None,
                        vec![crate::test_content::text(text)],
                    ),
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
            source: None,
        };
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.flow_mut().expect("Flow fixture").blocks = vec![Block::Table {
            fixed_view: None,
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
        document.flow_mut().expect("Flow fixture").sections.clear();
        let view = DocumentView::new(&query);
        let rendered = view.render((expected_column + 20).max(80));
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
                view.link_target_at(&rendered, row, expected_column),
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
        children: vec![crate::test_content::text(value.to_owned())],
        layout: LayoutHint::default(),
        source: None,
    };
    bundle
        .document
        .as_mut()
        .expect("document")
        .flow_mut()
        .expect("Flow fixture")
        .sections[0]
        .blocks = vec![Block::Table {
        fixed_view: None,
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    point: None,
                    blocks: vec![paragraph("alpha beta gamma")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
                },
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    point: None,
                    blocks: vec![paragraph("right hand")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
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
        children: vec![crate::test_content::text(value.to_owned())],
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |value: &str| TableCell {
        kind: mant_ir::TableCellKind::Text,
        point: None,
        blocks: vec![paragraph(value)],
        column_span: 1,
        row_span: 1,
        alignment: None,
        source: None,
    };
    let mut bundle = bundle();
    bundle
        .document
        .as_mut()
        .expect("document")
        .flow_mut()
        .expect("Flow fixture")
        .sections[0]
        .blocks = vec![Block::Table {
        fixed_view: None,
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
        children: vec![crate::test_content::text(value.to_owned())],
        layout: LayoutHint::default(),
        source: None,
    };
    let data = |value: &str| TableRow {
        kind: mant_ir::TableRowKind::Data,
        cells: vec![TableCell {
            kind: mant_ir::TableCellKind::Text,
            point: None,
            blocks: vec![paragraph(value)],
            column_span: 1,
            row_span: 1,
            alignment: None,
            source: None,
        }],
    };
    let mut bundle = bundle();
    bundle
        .document
        .as_mut()
        .expect("document")
        .flow_mut()
        .expect("Flow fixture")
        .sections[0]
        .blocks = vec![Block::Table {
        fixed_view: None,
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
    bundle
        .document
        .as_mut()
        .expect("document")
        .flow_mut()
        .expect("Flow fixture")
        .sections[0]
        .blocks = vec![Block::Table {
        fixed_view: None,
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    kind: mant_ir::TableCellKind::HorizontalRule,
                    point: None,
                    blocks: Vec::new(),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
                },
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    point: None,
                    blocks: vec![Block::Paragraph {
                        children: vec![crate::test_content::text("VISIBLE".to_owned())],
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
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
    bundle
        .document
        .as_mut()
        .expect("document")
        .flow_mut()
        .expect("Flow fixture")
        .sections[0]
        .blocks = vec![Block::Table {
        fixed_view: None,
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    kind: mant_ir::TableCellKind::HorizontalRule,
                    point: None,
                    blocks: Vec::new(),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
                },
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    point: None,
                    blocks: vec![Block::Paragraph {
                        children: vec![crate::test_content::text("VISIBLE".to_owned())],
                        layout: LayoutHint {
                            indent_columns: -1,
                            ..LayoutHint::default()
                        },
                        source: None,
                    }],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
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
        children: vec![crate::test_content::text(value.to_owned())],
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |value: &str| TableCell {
        kind: mant_ir::TableCellKind::Text,
        point: None,
        blocks: vec![paragraph(value)],
        column_span: 1,
        row_span: 1,
        alignment: None,
        source: None,
    };
    let mut bundle = bundle();
    bundle
        .document
        .as_mut()
        .expect("document")
        .flow_mut()
        .expect("Flow fixture")
        .sections[0]
        .blocks = vec![Block::Table {
        fixed_view: None,
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
                cells: vec![
                    TableCell {
                        kind: mant_ir::TableCellKind::HorizontalRule,
                        point: None,
                        blocks: Vec::new(),
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                        source: None,
                    },
                    TableCell {
                        kind: mant_ir::TableCellKind::DoubleHorizontalRule,
                        point: None,
                        blocks: Vec::new(),
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                        source: None,
                    },
                ],
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
