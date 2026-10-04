//! Existing regressions grouped by tables behavior; expected values remain independent.
use super::*;

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
fn table_cells_use_shared_content_driven_columns_and_independent_wrapping() {
    let mut bundle = bundle();
    let paragraph = |value: &str| Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph("alpha beta gamma")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
                TableCell {
                    break_after: false,
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

fn assert_padding_hit_coordinates(
    rendered: &RenderedDocument,
    hit: &RenderedSearchMatch,
    target: &LinkTarget,
) {
    let fragments = std::iter::once((hit.row, hit.start_column, hit.end_column)).chain(
        hit.additional_fragments
            .iter()
            .map(|fragment| (fragment.row, fragment.start_column, fragment.end_column)),
    );
    let mut copied = String::new();
    for (row, start, end) in fragments {
        let text = rendered.text.lines[row].to_string();
        // Search, clickable range and rendered glyph columns agree even when
        // Unicode makes a byte index differ from a display-cell coordinate.
        let fragment = rendered.selected_text(RenderedSelection {
            anchor: TextPosition { row, column: start },
            focus: TextPosition {
                row,
                column: end - 1,
            },
        });
        let byte = text.find(&fragment).unwrap();
        assert_eq!(start, mant_ir::geometry::text_width(&text[..byte]));
        assert_eq!(rendered.link_target_at(row, start), Some(target));
        assert_eq!(rendered.link_target_at(row, end - 1), Some(target));
        copied.push_str(&fragment);
    }
    assert_eq!(copied, "CLICK");
}

fn declared_preferences(widths: &[u16]) -> mant_ir::ColumnPreferences {
    if widths.is_empty() {
        return mant_ir::ColumnPreferences::default();
    }
    mant_ir::ColumnPreferences {
        widths: widths.to_vec(),
        gap_columns: match widths.len() {
            n if n < 5 => 4,
            5 => 3,
            _ => 1,
        },
        advance_limit_columns: Some(256),
        extra_width_columns: Some(10),
    }
}

#[test]
fn content_derived_tables_consume_custom_gap_preferences() {
    for gap in [0, 1, 2, 6] {
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        let cell = |text: &str| TableCell {
            break_after: false,
            blocks: vec![paragraph(text)],
            kind: mant_ir::TableCellKind::Text,
            column_span: 1,
            row_span: 1,
            alignment: None,
        };
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("LEFT_TOKEN"), cell("RIGHT_TOKEN")],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                gap_columns: gap,
                ..Default::default()
            },
            layout: LayoutHint::default(),
            source: None,
        }];
        let wire = mant_render::render_query_json(&query, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored = restored.into();
        let rendered = DocumentView::new(&restored).render(80);
        let left = rendered.search("LEFT_TOKEN");
        let right = rendered.search("RIGHT_TOKEN");
        assert_eq!(left.len(), 1);
        assert_eq!(right.len(), 1);
        assert_eq!(left[0].row, right[0].row);
        assert_eq!(
            right[0].start_column - left[0].start_column,
            10 + usize::from(gap)
        );
        let selected = rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: left[0].row,
                column: left[0].start_column,
            },
            focus: TextPosition {
                row: right[0].row,
                column: right[0].end_column,
            },
        });
        assert!(selected.contains("LEFT_TOKEN"));
        assert!(selected.contains("RIGHT_TOKEN"));
    }
}

#[test]
fn full_query_padding_fallback_preserves_all_cells_at_maximum_viewport() {
    for count in [256, 257] {
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        let words = (0..count)
            .map(|index| format!("CELL_{index:03}"))
            .collect::<Vec<_>>();
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: words
                    .iter()
                    .map(|word| TableCell {
                        break_after: false,
                        blocks: vec![paragraph(word)],
                        kind: mant_ir::TableCellKind::Text,
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    })
                    .collect(),
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![u16::MAX; count],
                advance_limit_columns: Some(256),
                extra_width_columns: Some(10),
                ..Default::default()
            },
            layout: LayoutHint::default(),
            source: None,
        }];
        let wire = mant_render::render_query_json(&query, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored = restored.into();
        let rendered = DocumentView::new(&restored).render(u16::MAX);
        let mut previous = None;
        for word in words {
            let matches = rendered.search(&word);
            assert_eq!(matches.len(), 1, "count={count}, {word}");
            let hit = &matches[0];
            assert_eq!(hit.start_column, 0, "count={count}, {word}");
            if let Some(row) = previous {
                assert_eq!(hit.row, row + 1);
            }
            previous = Some(hit.row);
        }
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

fn assert_field_roundtrip(
    query: &ResolvedContent,
    shared: bool,
    column: usize,
    (indent, correction, nested): (i32, i32, bool),
) {
    let wire = mant_render::render_query_json(query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    let rendered = DocumentView::new(&restored).render(u16::MAX);
    let first = rendered.search("LEFT");
    let body = rendered.search("FIELD_B");
    assert_eq!(body.len(), 1);
    assert_eq!(first.len(), 1);
    let body = &body[0];
    assert_eq!(body.start_column, column, "{indent}/{correction}/{nested}");
    assert_eq!(
        body.row == first[0].row,
        shared,
        "{indent}/{correction}/{nested}"
    );
    assert_eq!(rendered.anchor_row("field-owner"), Some(body.row));
    assert!(rendered.link_target_at(body.row, column).is_some());
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: body.row,
                column
            },
            focus: TextPosition {
                row: body.row,
                column: body.end_column
            },
        }),
        "FIELD_B"
    );
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

fn generic_linked_cell(value: &str, indent: i32, alignment: mant_ir::TableAlignment) -> TableCell {
    TableCell {
        break_after: false,
        blocks: vec![Block::Paragraph {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://e.example/generic".into(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: value.into(),
                }],
            }],
            inline_layout: mant_ir::InlineLayout::default(),
            layout: LayoutHint {
                indent_columns: indent,
                ..Default::default()
            },
            source: None,
        }],
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: Some(alignment),
    }
}

#[test]
fn nested_table_receipts_keep_parent_origins_and_completed_rows() {
    let cell = |blocks| TableCell {
        break_after: false,
        blocks,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    for (parent, offset, expected) in [(4090, 0, 4090), (5, -3, 2), (-2, 3, 1)] {
        let inner = Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    cell(vec![paragraph("LEFT")]),
                    cell(vec![paragraph("RIGHT")]),
                ],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![500, 1],
                ..Default::default()
            },
            layout: LayoutHint {
                indent_columns: offset,
                ..Default::default()
            },
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
                indent_columns: parent,
                ..Default::default()
            },
            source: None,
        }];
        let rendered = DocumentView::new(&query).render(u16::MAX);
        assert_eq!(rendered.search("LEFT")[0].start_column, expected);
        if parent == 4090 {
            assert_eq!(rendered.search("RIGHT")[0].start_column, expected);
        }
    }
    for widths in [vec![], vec![8]] {
        let inner = Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell(vec![
                    paragraph("FIRST"),
                    Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    },
                ])],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths,
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
                cells: vec![cell(vec![inner]), cell(vec![paragraph("SECOND")])],
            }],
            column_preferences: mant_ir::ColumnPreferences {
                widths: vec![1, 1],
                ..Default::default()
            },
            layout: LayoutHint::default(),
            source: None,
        }];
        let rendered = DocumentView::new(&query).render(80);
        assert_eq!(
            rendered.search("SECOND")[0].row - rendered.search("FIRST")[0].row,
            2
        );
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

#[test]
fn nested_table_open_tail_receipts_propagate_across_every_cached_layer() {
    // Exact nested literal source ran all five pristine profiles before this
    // assertion: termp_bd_post/term_flushln expose an open field continuation;
    // term_vspace separately completes a blank row. This model fixes the
    // reading contract without copying native macro spacing into layout.
    let cell = |blocks| TableCell {
        break_after: false,
        blocks,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    for depth in [1, 2, 4] {
        for (value, distance) in [("FIRST\n", 1), ("FIRST\n\n", 2), ("FIRST\n \n", 2)] {
            let mut inner = Block::Preformatted {
                children: vec![
                    Inline::Text {
                        value: value.into(),
                    },
                    Inline::anchor("nested-tail"),
                ],
                inline_layout: mant_ir::InlineLayout::default(),
                language: None,
                layout: LayoutHint::default(),
                source: None,
            };
            for _ in 0..depth {
                inner = Block::Table {
                    rows: vec![TableRow {
                        kind: mant_ir::TableRowKind::Data,
                        cells: vec![cell(vec![inner])],
                    }],
                    column_preferences: mant_ir::ColumnPreferences {
                        widths: vec![8],
                        ..Default::default()
                    },
                    layout: LayoutHint::default(),
                    source: None,
                };
            }
            let mut query = bundle();
            let document = query.document.as_mut().unwrap();
            document.sections.clear();
            document.blocks = vec![Block::Table {
                rows: vec![TableRow {
                    kind: mant_ir::TableRowKind::Data,
                    cells: vec![cell(vec![inner]), cell(vec![paragraph("SECOND")])],
                }],
                column_preferences: mant_ir::ColumnPreferences {
                    widths: vec![8, 8],
                    ..Default::default()
                },
                layout: LayoutHint::default(),
                source: None,
            }];
            let rendered = DocumentView::new(&query).render(80);
            let first = rendered.search("FIRST")[0].clone();
            let second = rendered.search("SECOND")[0].clone();
            assert_eq!(second.row - first.row, distance, "depth={depth}, {value:?}");
            assert_eq!(rendered.anchor_row("nested-tail"), Some(second.row));
        }
    }
}
