use super::*;
fn cell(columns: u32, rows: u32) -> TableCell {
    TableCell {
        preferred_width_columns: None,
        break_after: false,
        kind: crate::TableCellKind::Text,
        blocks: Vec::new(),
        column_span: columns,
        row_span: rows,
        alignment: None,
    }
}

#[test]
fn navigation_rows_do_not_include_empty_physical_payloads() {
    let text = |value: &str| Inline::Text {
        value: value.into(),
    };
    let paragraph = |children| Block::Paragraph {
        inline_layout: crate::InlineLayout::default(),
        children,
        layout: crate::LayoutHint::default(),
        source: None,
    };
    let mut row = TableRow {
        column_borders: vec![],
        kind: TableRowKind::Data,
        cells: vec![TableCell {
            preferred_width_columns: None,
            blocks: vec![paragraph(vec![Inline::Strong {
                children: vec![Inline::Emphasis {
                    children: vec![Inline::anchor("target")],
                }],
            }])],
            ..cell(1, 1)
        }],
    };
    assert!(table_row_is_navigation_only(&row));
    let navigation = row.cells[0].clone();
    row.cells.push(cell(1, 1));
    assert!(!table_row_is_navigation_only(&row));
    row.cells.reverse();
    assert!(!table_row_is_navigation_only(&row));
    row.cells = vec![navigation];
    row.cells[0].column_span = 2;
    assert!(!table_row_is_navigation_only(&row));
    row.cells[0].column_span = 1;
    row.cells[0].row_span = 2;
    assert!(!table_row_is_navigation_only(&row));
    row.cells[0].row_span = 1;
    for blocks in [
        vec![],
        vec![paragraph(vec![])],
        vec![paragraph(vec![Inline::anchor("target"), text("")])],
        vec![paragraph(vec![Inline::anchor("target"), text(" ")])],
        vec![paragraph(vec![
            Inline::anchor("target"),
            Inline::line_break(),
        ])],
        vec![Block::VerticalSpace {
            lines: 1,
            source: None,
        }],
        vec![Block::Preformatted {
            inline_layout: crate::InlineLayout::default(),
            children: vec![Inline::anchor("target"), text("\n")],
            language: None,
            layout: crate::LayoutHint::default(),
            source: None,
        }],
        vec![Block::Paragraph {
            inline_layout: crate::InlineLayout::default(),
            children: vec![Inline::anchor("target")],
            layout: crate::LayoutHint {
                spacing_before_lines: 1,
                ..Default::default()
            },
            source: None,
        }],
    ] {
        row.cells[0].blocks = blocks;
        assert!(!table_row_is_navigation_only(&row), "{row:#?}");
    }
    row.cells.clear();
    assert!(!table_row_is_navigation_only(&row));
    row.cells.push(TableCell {
        preferred_width_columns: None,
        kind: TableCellKind::HorizontalRule,
        blocks: vec![paragraph(vec![Inline::anchor("target")])],
        ..cell(1, 1)
    });
    assert!(!table_row_is_navigation_only(&row));
}

#[test]
fn explicit_cell_closure_keeps_anchor_data_rows_without_body_or_hint_glyphs() {
    for break_after in [false, true] {
        let row = TableRow {
            column_borders: vec![],
            kind: TableRowKind::Data,
            cells: vec![TableCell {
                break_after,
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Strong {
                        children: vec![Inline::Emphasis {
                            children: vec![Inline::anchor("target")],
                        }],
                    }],
                    inline_layout: crate::InlineLayout {
                        row_hints: vec![crate::RowLayoutHint {
                            row: 0,
                            indent_columns: 12,
                            ..Default::default()
                        }],
                    },
                    layout: crate::LayoutHint::default(),
                    source: None,
                }],
                ..cell(1, 1)
            }],
        };
        assert_eq!(table_row_is_navigation_only(&row), !break_after);
        let wire = serde_json::to_string(&row).unwrap();
        let restored: TableRow = serde_json::from_str(&wire).unwrap();
        assert_eq!(restored, row);
        assert_eq!(table_row_is_navigation_only(&restored), !break_after);
        let [
            Block::Paragraph {
                children,
                inline_layout,
                ..
            },
        ] = restored.cells[0].blocks.as_slice()
        else {
            panic!("original paragraph owner");
        };
        assert_eq!(crate::inline_plain_text(children), "");
        assert_eq!(inline_layout.row_indent(0), 12);
        assert_eq!(wire.contains("\"breakAfter\":true"), break_after);
    }
}

#[test]
fn spans_and_explicit_vertical_continuations_keep_logical_columns() {
    let rows = vec![
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: vec![cell(2, 2), cell(1, 1)],
        },
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: vec![cell(2, 1), cell(1, 1)],
        },
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: vec![cell(1, 1), cell(1, 1), cell(1, 1)],
        },
    ];
    let grid = TableGrid::new(&rows);
    assert_eq!(grid.column_count, 3);
    assert_eq!(grid.rows[0][1].column, 2);
    assert_eq!(grid.rows[1][1].column, 2);
    assert_eq!(grid.rows[2][1].column, 1);
    let slots = grid.slots(0, 3).unwrap();
    assert!(slots[0].is_some() && slots[1].is_none() && slots[2].is_some());
    assert!(grid.slots(0, 2).is_none());
}

#[test]
fn extreme_and_invalid_spans_do_not_allocate_dense_storage() {
    let rows = vec![TableRow {
        column_borders: vec![],
        kind: crate::TableRowKind::Data,
        cells: vec![cell(u32::from(u16::MAX), u32::from(u16::MAX)), cell(0, 0)],
    }];
    let grid = TableGrid::new(&rows);
    assert_eq!(grid.column_count, 65_536);
    assert_eq!(grid.rows[0].len(), 2);
    assert!(grid.slots(0, 256).is_none());
}

#[test]
fn bounded_plan_keeps_dense_slots_through_the_exact_column_limit() {
    for (span, dense) in [(255, true), (256, false)] {
        let rows = [TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: vec![cell(span, 1), cell(1, 1)],
        }];
        let plan = bounded_table_rows(&rows);
        match &plan[0] {
            TableRowPlan::Dense { slots } => {
                assert!(dense);
                assert_eq!(slots.len(), MAX_DENSE_TABLE_COLUMNS);
                assert!(slots[1..255].iter().all(Option::is_none));
                assert!(std::ptr::eq(slots[0].unwrap(), &raw const rows[0].cells[0]));
                assert!(std::ptr::eq(
                    slots[255].unwrap(),
                    &raw const rows[0].cells[1]
                ));
            }
            TableRowPlan::Sparse { cells } => {
                assert!(!dense);
                assert_eq!(cells.len(), 2);
                assert_eq!(cells[1].column, 256);
            }
            TableRowPlan::Empty
            | TableRowPlan::WholeRule { .. }
            | TableRowPlan::LayoutRule { .. } => {
                panic!("nonempty data row must retain a cell plan")
            }
        }
    }
}

#[test]
fn sparse_plan_preserves_payloads_without_span_sized_allocations() {
    let rows = [
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: vec![cell(u32::from(u16::MAX), 2), cell(0, 0)],
        },
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: Vec::new(),
        },
    ];
    let plan = bounded_table_rows(&rows);
    let TableRowPlan::Sparse { cells } = &plan[0] else {
        panic!("wide table must be sparse")
    };
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].column, 0);
    assert_eq!(cells[1].column, 65_535);
    assert!(std::ptr::eq(cells[0].cell, &raw const rows[0].cells[0]));
    assert!(std::ptr::eq(cells[1].cell, &raw const rows[0].cells[1]));
    assert!(matches!(&plan[1], TableRowPlan::Empty));
}

#[test]
fn bounded_plan_keeps_empty_rows_and_explicit_vertical_continuations() {
    let rows = [
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: vec![cell(2, 2), cell(1, 1)],
        },
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: vec![cell(2, 1)],
        },
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::Data,
            cells: Vec::new(),
        },
    ];
    let plans = bounded_table_rows(&rows);
    for (row, plan) in plans[..2].iter().enumerate() {
        let TableRowPlan::Dense { slots } = plan else {
            panic!("small table must be dense")
        };
        assert_eq!(slots.len(), 3);
        for (column, slot) in slots.iter().enumerate() {
            assert_eq!(
                slot.is_some(),
                (row == 0 && column != 1) || (row == 1 && column == 0)
            );
        }
    }
    assert!(matches!(plans[2], TableRowPlan::Empty));
    assert!(bounded_table_rows(&[]).is_empty());
}

#[test]
fn bounded_plan_retains_whole_row_rule_strength_without_cells() {
    let rows = [
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::HorizontalRule,
            cells: Vec::new(),
        },
        TableRow {
            column_borders: vec![],
            kind: crate::TableRowKind::DoubleHorizontalRule,
            cells: Vec::new(),
        },
    ];
    assert!(matches!(
        bounded_table_rows(&rows).as_slice(),
        [
            TableRowPlan::WholeRule { double: false },
            TableRowPlan::WholeRule { double: true }
        ]
    ));
}

#[test]
fn table_row_kind_json_round_trip_is_closed_and_backward_compatible() {
    let data = TableRow {
        column_borders: vec![],
        kind: crate::TableRowKind::Data,
        cells: Vec::new(),
    };
    let data_json = serde_json::to_value(&data).unwrap();
    assert!(data_json.get("kind").is_none());
    assert_eq!(
        serde_json::from_value::<TableRow>(serde_json::json!({"cells": []})).unwrap(),
        data
    );

    for (kind, expected) in [
        (
            crate::TableRowKind::HorizontalRule,
            serde_json::json!("horizontal-rule"),
        ),
        (
            crate::TableRowKind::DoubleHorizontalRule,
            serde_json::json!("double-horizontal-rule"),
        ),
        (
            crate::TableRowKind::LayoutRule {
                cells: vec![
                    crate::TableRuleCellKind::Horizontal,
                    crate::TableRuleCellKind::DoubleHorizontal,
                ],
            },
            serde_json::json!({"layout-rule":{"cells":["horizontal","double-horizontal"]}}),
        ),
    ] {
        let row = TableRow {
            column_borders: vec![],
            kind,
            cells: Vec::new(),
        };
        let encoded = serde_json::to_value(&row).unwrap();
        assert_eq!(encoded["kind"], expected);
        assert_eq!(serde_json::from_value::<TableRow>(encoded).unwrap(), row);
    }
    assert!(
        serde_json::from_value::<TableRow>(serde_json::json!({
            "kind": "future-rule",
            "cells": []
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<TableRow>(serde_json::json!({
            "kind": {"layout-rule": {
                "cells": ["horizontal"],
                "future": true
            }},
            "cells": []
        }))
        .is_err()
    );
}

#[test]
fn table_cell_kind_json_round_trip_is_closed_and_backward_compatible() {
    let text = cell(1, 1);
    let text_json = serde_json::to_value(&text).unwrap();
    assert!(text_json.get("kind").is_none());
    assert_eq!(
        serde_json::from_value::<TableCell>(serde_json::json!({
            "blocks": [],
            "alignment": null
        }))
        .unwrap(),
        text
    );

    for kind in [
        crate::TableCellKind::HorizontalRule,
        crate::TableCellKind::DoubleHorizontalRule,
        crate::TableCellKind::IsolatedHorizontalRule,
        crate::TableCellKind::IsolatedDoubleHorizontalRule,
    ] {
        let rule = TableCell { kind, ..cell(1, 1) };
        let encoded = serde_json::to_value(&rule).unwrap();
        assert_eq!(serde_json::from_value::<TableCell>(encoded).unwrap(), rule);
    }
    assert!(
        serde_json::from_value::<TableCell>(serde_json::json!({
            "kind": "future-rule",
            "blocks": []
        }))
        .is_err()
    );
}
