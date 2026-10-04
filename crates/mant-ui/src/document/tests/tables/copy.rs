//! Table copy regressions with unchanged source and geometry expectations.
use super::super::*;
use super::fixtures::{assert_padding_hit_coordinates, declared_preferences};

#[test]
fn declared_column_selection_and_anchor_ranges_follow_shared_origins() {
    // The equivalent `Bl -column 12345678 b` source was run with pristine
    // CVS before asserting the twelve-cell field origin. Here the native
    // device fact is constructed directly, testing the consumer boundary.
    let mut query = bundle();
    let cell = |label: &str, anchor: Option<&str>, target: bool| {
        let mut children = Vec::new();
        if let Some(anchor) = anchor {
            children.push(Inline::anchor_at(anchor, None));
        }
        let text = vec![Inline::Text {
            value: label.to_owned(),
        }];
        if target {
            children.push(Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "description".into(),
                },
                title: None,
                children: text,
            });
        } else {
            children.extend(text);
        }
        TableCell {
            break_after: false,
            kind: mant_ir::TableCellKind::Text,
            blocks: vec![Block::Paragraph {
                inline_layout: mant_ir::InlineLayout::default(),
                children,
                layout: LayoutHint::default(),
                source: None,
            }],
            column_span: 1,
            row_span: 1,
            alignment: None,
        }
    };
    let document = query.document.as_mut().unwrap();
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                cell("A", None, false),
                cell("SECOND", Some("second-cell"), true),
            ],
        }],
        column_preferences: declared_preferences(&[8, 1]),
        layout: LayoutHint::default(),
        source: None,
    }];
    document.sections.clear();
    let view = DocumentView::new(&query);
    for width in [80, 8, 80] {
        let rendered = view.render(width);
        let found = &rendered.search("SECOND")[0];
        assert_eq!(rendered.anchor_row("second-cell"), Some(found.row));
        assert!(
            rendered
                .link_target_at(found.row, found.start_column)
                .is_some()
        );
        let selected = RenderedSelection {
            anchor: TextPosition {
                row: found.row,
                column: found.start_column,
            },
            focus: TextPosition {
                row: found.row,
                column: found.start_column + 5,
            },
        };
        assert_eq!(rendered.selected_text(selected), "SECOND");
        if width == 80 {
            assert_eq!(found.start_column, 12);
        }
    }
}

#[test]
fn public_ir_trailing_padding_keeps_link_search_copy_and_actual_cells_aligned() {
    // This is valid source-neutral IR, not a claim that native term_field
    // prints all authored trailing blanks. Preserve its full text while the
    // accepted extent decides wrapping; the actual cursor cannot move back.
    for prefix in ["X", "中", "e\u{301}", "😀"] {
        for wrapper in ["text", "strong", "emphasis", "code", "link", "literal"] {
            let value = format!("{prefix}          ");
            let text = vec![Inline::Text {
                value: value.clone(),
            }];
            let first = match wrapper {
                "text" | "literal" => text,
                "strong" => vec![Inline::Strong { children: text }],
                "emphasis" => vec![Inline::Emphasis { children: text }],
                "code" => vec![Inline::Code { value }],
                "link" => vec![Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://first.example/".into(),
                    },
                    title: None,
                    children: text,
                }],
                _ => unreachable!(),
            };
            let target = LinkTarget::Section("description".into());
            let next = vec![
                Inline::anchor_at("next-cell", None),
                Inline::Link {
                    target: mant_ir::LinkTarget::Section {
                        id: "description".into(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "CLICK".into(),
                    }],
                },
            ];
            let mut query = bundle();
            let document = query.document.as_mut().unwrap();
            document.blocks = vec![Block::Table {
                rows: vec![TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: [first, next]
                        .into_iter()
                        .enumerate()
                        .map(|(index, children)| TableCell {
                            break_after: false,
                            kind: mant_ir::TableCellKind::Text,
                            blocks: vec![if index == 0 && wrapper == "literal" {
                                // Exercise the real Code surface: only its
                                // renderer-owned final fill span is retired.
                                // All ten author blanks keep their ownership.
                                Block::Preformatted {
                                    inline_layout: mant_ir::InlineLayout::default(),
                                    children,
                                    language: None,
                                    layout: LayoutHint::default(),
                                    source: None,
                                }
                            } else {
                                Block::Paragraph {
                                    inline_layout: mant_ir::InlineLayout::default(),
                                    children,
                                    layout: LayoutHint::default(),
                                    source: None,
                                }
                            }],
                            column_span: 1,
                            row_span: 1,
                            alignment: None,
                        })
                        .collect(),
                }],
                column_preferences: declared_preferences(&[3, 3]),
                layout: LayoutHint::default(),
                source: None,
            }];
            document.sections.clear();
            let json = mant_render::render_query_json(&query, false).unwrap();
            let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
            let view = DocumentView::new(&decoded.into());
            for width in [80, 16, 8, 80] {
                let rendered = view.render(width);
                let hits = rendered.search("CLICK");
                assert_eq!(hits.len(), 1, "{prefix}/{wrapper} at {width}");
                let hit = &hits[0];
                assert_eq!(rendered.anchor_row("next-cell"), Some(hit.row));
                assert_padding_hit_coordinates(&rendered, hit, &target);
            }
        }
    }
}

#[test]
fn derived_literal_receipts_and_nested_author_spaces_survive_copy() {
    let cell = |blocks| TableCell {
        break_after: false,
        blocks,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    for (value, rows) in [("A\n", 1), ("A\n\n", 2), ("A\n \n", 2)] {
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![
            Block::Table {
                rows: vec![TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: vec![cell(vec![Block::Preformatted {
                        children: vec![Inline::Text {
                            value: value.into(),
                        }],
                        inline_layout: mant_ir::InlineLayout::default(),
                        language: None,
                        layout: LayoutHint::default(),
                        source: None,
                    }])],
                }],
                column_preferences: mant_ir::ColumnPreferences::default(),
                layout: LayoutHint::default(),
                source: None,
            },
            paragraph("AFTER"),
        ];
        let rendered = DocumentView::new(&query).render(80);
        assert_eq!(
            rendered.search("AFTER")[0].row - rendered.search("A")[0].row,
            rows
        );
    }
    let author = Block::Paragraph {
        children: vec![
            Inline::anchor("author-space"),
            Inline::Text {
                value: "   \u{a0}".into(),
            },
        ],
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    };
    let inner = Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![cell(vec![author]), cell(vec![paragraph("RIGHT")])],
        }],
        column_preferences: mant_ir::ColumnPreferences {
            widths: vec![500, 1],
            ..Default::default()
        },
        layout: LayoutHint::default(),
        source: None,
    };
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![cell(vec![inner])],
        }],
        column_preferences: mant_ir::ColumnPreferences {
            widths: vec![1],
            ..Default::default()
        },
        layout: LayoutHint {
            indent_columns: 5,
            ..Default::default()
        },
        source: None,
    }];
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    let rendered = DocumentView::new(&restored).render(u16::MAX);
    let row = rendered.anchor_row("author-space").unwrap();
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition { row, column: 5 },
            focus: TextPosition { row, column: 8 },
        }),
        "   \u{a0}"
    );
    assert_eq!(rendered.search("RIGHT")[0].start_column, 507);
}
