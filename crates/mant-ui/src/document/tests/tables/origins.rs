//! Table origins regressions with unchanged source and geometry expectations.
use super::super::*;
use super::fixtures::{assert_field_roundtrip, generic_linked_cell};

#[test]
fn signed_table_cells_preserve_real_origins_links_and_anchors() {
    for (table_indent, child_indent, expected_column) in
        [(-2, 3, 1), (3, -2, 1), (4096, 3, 4096), (4090, 10, 4096)]
    {
        let cell = |id: &str, text: &str| TableCell {
            break_after: false,
            kind: mant_ir::TableCellKind::Text,
            blocks: vec![Block::Paragraph {
                inline_layout: mant_ir::InlineLayout::default(),
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
            column_preferences: mant_ir::ColumnPreferences::default(),
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
fn actual_field_parent_and_child_origins_keep_query_links_search_and_copy() {
    // The adjacent offset/Bd source ran pristine ASCII/UTF-8/HTML/tree/lint
    // before this counter. mdoc_term.c::termp_bd_pre composes with the active
    // field parent. The 4096 bound below belongs to generic reading geometry.
    for (indent, correction, nested, shared, column) in [
        (0, 0, false, true, 4096),
        (2, 0, false, false, 4092),
        (0, 2, false, false, 4092),
        (-2, 0, false, false, 4088),
        (-10, 0, true, false, 4090),
    ] {
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        let body = Block::Paragraph {
            children: vec![
                Inline::anchor("field-owner"),
                Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://e.example/field".into(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "FIELD_B".into(),
                    }],
                },
            ],
            inline_layout: mant_ir::InlineLayout {
                row_hints: if correction == 0 {
                    vec![]
                } else {
                    vec![mant_ir::RowLayoutHint {
                        row: 0,
                        indent_columns: correction,
                    }]
                },
            },
            layout: LayoutHint {
                indent_columns: indent,
                ..Default::default()
            },
            source: None,
        };
        let body = if nested {
            Block::List {
                kind: ListKind::Plain,
                compact: true,
                items: vec![mant_ir::ListItem {
                    blocks: vec![body],
                    layout: mant_ir::ListItemLayout::default(),
                    source: None,
                    entry: None,
                }],
                layout: LayoutHint {
                    indent_columns: 10,
                    ..Default::default()
                },
                source: None,
            }
        } else {
            body
        };
        let cell = |block| TableCell {
            break_after: false,
            blocks: vec![block],
            kind: mant_ir::TableCellKind::Text,
            column_span: 1,
            row_span: 1,
            alignment: None,
        };
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell(paragraph("LEFT")), cell(body)],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![4, 1],
                ..Default::default()
            },
            layout: LayoutHint {
                indent_columns: 4090,
                ..Default::default()
            },
            source: None,
        }];
        assert_field_roundtrip(&query, shared, column, (indent, correction, nested));
    }
}

#[test]
fn generic_alignment_checks_actual_origins_without_losing_source_mapping() {
    use mant_ir::TableAlignment;
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    generic_linked_cell("LEFT", 0, TableAlignment::Left),
                    generic_linked_cell("RIGHT_LINK", 2, TableAlignment::Right),
                ],
            },
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    generic_linked_cell("OTHER", 0, TableAlignment::Left),
                    generic_linked_cell("LONG_UNRELATED_VALUE", 0, TableAlignment::Left),
                ],
            },
        ],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint {
            indent_columns: 4088,
            ..Default::default()
        },
        source: None,
    }];
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    let rendered = DocumentView::new(&restored).render(u16::MAX);
    let left = rendered.search("LEFT")[0].clone();
    let right = rendered.search("RIGHT_LINK")[0].clone();
    assert!(right.row > left.row);
    assert_eq!(right.start_column, 4090);
    assert!(
        rendered
            .link_target_at(right.row, right.start_column)
            .is_some()
    );
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: right.row,
                column: right.start_column
            },
            focus: TextPosition {
                row: right.row,
                column: right.end_column
            },
        }),
        "RIGHT_LINK"
    );
}
