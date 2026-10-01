use super::*;
use mant_ir::LayoutHint;

fn navigation_table(widths: &[u16], origin: i32, cells: Vec<Vec<Block>>) -> Block {
    Block::Table {
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: cells
                .into_iter()
                .map(|blocks| TableCell {
                    blocks,
                    kind: mant_ir::TableCellKind::Text,
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                })
                .collect(),
        }],
        column_widths: widths.to_vec(),
        layout: LayoutHint {
            indent_columns: origin,
            ..Default::default()
        },
        source: None,
    }
}

#[test]
fn navigation_table_rows_preserve_physical_empty_row_counters() {
    let paragraph = |children| Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    };
    let text = |value: &str| Inline::Text {
        value: value.into(),
    };
    let navigation = paragraph(vec![Inline::Strong {
        children: vec![Inline::Emphasis {
            children: vec![Inline::anchor("target")],
        }],
    }]);
    for widths in [vec![], vec![3, 3], vec![u16::MAX]] {
        for origin in [0, -2] {
            for (cells, extra_rows) in [
                (vec![vec![navigation.clone()]], 0),
                (vec![], 1),
                (vec![vec![]], 1),
                (vec![vec![], vec![]], 1),
                (vec![vec![paragraph(vec![text("")])]], 1),
                (vec![vec![navigation.clone()], vec![]], 1),
                (vec![vec![], vec![navigation.clone()]], 1),
                (
                    vec![vec![Block::Preformatted {
                        children: vec![text("")],
                        language: None,
                        layout: LayoutHint::default(),
                        source: None,
                    }]],
                    1,
                ),
                (
                    vec![vec![Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    }]],
                    1,
                ),
                (
                    vec![vec![Block::VerticalSpace {
                        lines: 0,
                        source: None,
                    }]],
                    1,
                ),
            ] {
                let table = navigation_table(&widths, origin, cells);
                let blocks = [
                    paragraph(vec![text("BEFORE")]),
                    table,
                    paragraph(vec![text("AFTER")]),
                ];
                for decorated in [false, true] {
                    let paint = |_: TextPresentation, value: &str| {
                        if decorated {
                            format!("\x1b[1m{value}\x1b[0m")
                        } else {
                            value.into()
                        }
                    };
                    let renderer = BlockRenderer {
                        names: None,
                        locations: None,
                        decorate: &paint,
                    };
                    let output = renderer
                        .render_blocks(&blocks, 0)
                        .replace("\x1b[1m", "")
                        .replace("\x1b[0m", "");
                    let rows = output.split('\n').collect::<Vec<_>>();
                    let before = rows.iter().position(|row| row.contains("BEFORE")).unwrap();
                    let after = rows.iter().position(|row| row.contains("AFTER")).unwrap();
                    assert_eq!(after - before, 1 + extra_rows, "{output:?}");
                }
            }
        }
    }
}

#[test]
fn hard_row_origins_compose_for_paragraph_and_literal_rows() {
    let renderer = super::super::plain_renderer();
    let children = vec![
        Inline::Text {
            value: "Alpha".into(),
        },
        Inline::line_break_indented(6),
        Inline::Strong {
            children: vec![Inline::Text {
                value: "Beta".into(),
            }],
        },
        Inline::line_break(),
        Inline::Text {
            value: "Gamma".into(),
        },
    ];
    for block in [
        Block::Paragraph {
            children: children.clone(),
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Preformatted {
            children,
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
    ] {
        assert_eq!(
            renderer.render_blocks(&[block], -2),
            "Alpha\n    Beta\nGamma"
        );
    }
}

#[test]
fn literal_whitespace_is_content_even_at_indented_and_document_edges() {
    let renderer = super::super::plain_renderer();
    for value in ["", " ", "\n", "\n\n", "\nALPHA\n\n", "  \n \n"] {
        for origin in [0, 3] {
            let block = Block::Preformatted {
                language: None,
                children: vec![Inline::Text {
                    value: value.into(),
                }],
                layout: LayoutHint::default(),
                source: None,
            };
            let expected = value
                .split('\n')
                .map(|line| {
                    if line.is_empty() {
                        String::new()
                    } else {
                        format!("{}{line}", " ".repeat(origin))
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(
                renderer.render_blocks(&[block], i32::try_from(origin).unwrap()),
                expected
            );
        }
    }
}

#[test]
fn signed_table_cells_compose_parent_origins_before_clipping() {
    let renderer = super::super::plain_renderer();
    for (table_indent, child_indent, expected_column) in
        [(-2, 3, 1), (3, -2, 1), (4096, 3, 4096), (4090, 10, 4096)]
    {
        let cell = |text| TableCell {
            kind: mant_ir::TableCellKind::Text,
            blocks: vec![paragraph(text, child_indent)],
            column_span: 2,
            row_span: 1,
            alignment: None,
        };
        let table = Block::Table {
            column_widths: Vec::new(),
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("FIRST"), cell("SECOND")],
            }],
            layout: LayoutHint {
                indent_columns: table_indent,
                ..Default::default()
            },
            source: None,
        };
        assert_eq!(
            renderer.render_blocks(&[table], 0),
            format!(
                "{}FIRST\n{}SECOND",
                " ".repeat(expected_column),
                " ".repeat(expected_column)
            )
        );
    }
    let nested = plain_list(
        vec![Block::Table {
            column_widths: Vec::new(),
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![plain_list(vec![paragraph("NESTED", 5)], 3)],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                }],
            }],
            layout: LayoutHint {
                indent_columns: 2,
                ..Default::default()
            },
            source: None,
        }],
        4090,
    );
    assert_eq!(
        renderer.render_blocks(&[nested], 0),
        format!("{}NESTED", " ".repeat(4096))
    );
    let table = Block::Table {
        column_widths: Vec::new(),
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: ["FIRST", "SECOND"]
                .map(|text| TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph(text, 0)],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                })
                .into(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let ordinary = renderer.render_blocks(&[table], 0);
    assert!(
        ordinary
            .lines()
            .any(|line| line.contains("FIRST") && line.contains("SECOND")),
        "{ordinary}"
    );
}

#[test]
fn stacked_tables_preserve_partial_whole_layout_rules_and_empty_rows() {
    let renderer = super::super::plain_renderer();
    let table = Block::Table {
        column_widths: Vec::new(),
        rows: vec![
            mant_ir::TableRow {
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
                        blocks: vec![paragraph("VISIBLE", -1)],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                ],
            },
            mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: Vec::new(),
            },
            mant_ir::TableRow {
                kind: mant_ir::TableRowKind::DoubleHorizontalRule,
                cells: Vec::new(),
            },
            mant_ir::TableRow {
                kind: mant_ir::TableRowKind::LayoutRule {
                    cells: vec![
                        mant_ir::TableRuleCellKind::Horizontal,
                        mant_ir::TableRuleCellKind::DoubleHorizontal,
                    ],
                },
                cells: Vec::new(),
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(
        renderer.render_blocks(&[table], 0),
        "---\nVISIBLE\n\n===\n--- | ==="
    );
}

fn paragraph(text: &str, indent: i32) -> Block {
    Block::Paragraph {
        children: vec![Inline::Text { value: text.into() }],
        layout: LayoutHint {
            indent_columns: indent,
            ..Default::default()
        },
        source: None,
    }
}

#[test]
fn paragraph_preserves_a_formatter_generated_leading_line_break() {
    let renderer = super::super::plain_renderer();
    let block = Block::Paragraph {
        children: vec![
            Inline::line_break(),
            Inline::Text {
                value: "BODY".into(),
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(renderer.render_blocks(&[block], 0), "\nBODY");
}

#[test]
fn table_cells_preserve_formatter_generated_line_breaks() {
    let renderer = super::super::plain_renderer();
    let table = Block::Table {
        column_widths: Vec::new(),
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![TableCell {
                kind: mant_ir::TableCellKind::Text,
                blocks: vec![Block::Paragraph {
                    children: vec![
                        Inline::Text { value: "A".into() },
                        Inline::line_break(),
                        Inline::Text {
                            value: "B C".into(),
                        },
                    ],
                    layout: LayoutHint::default(),
                    source: None,
                }],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(renderer.render_blocks(&[table], 0), "A\nB C");
}

#[test]
fn table_cells_preserve_leading_and_trailing_physical_rows() {
    let renderer = super::super::plain_renderer();
    for (children, expected) in [
        (
            vec![
                Inline::line_break(),
                Inline::Text {
                    value: "BODY".into(),
                },
            ],
            "\nBODY",
        ),
        (
            vec![
                Inline::Text {
                    value: "BODY".into(),
                },
                Inline::line_break(),
            ],
            "BODY",
        ),
    ] {
        let table = Block::Table {
            column_widths: Vec::new(),
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![Block::Paragraph {
                        children,
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                }],
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        assert_eq!(renderer.render_blocks(&[table], 0), expected);
    }
}

#[test]
fn an_empty_table_row_remains_a_physical_row() {
    let renderer = super::super::plain_renderer();
    let table = Block::Table {
        column_widths: Vec::new(),
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![TableCell {
                kind: mant_ir::TableCellKind::Text,
                blocks: vec![Block::Paragraph {
                    children: Vec::new(),
                    layout: LayoutHint::default(),
                    source: None,
                }],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let surrounding = [
        Block::Paragraph {
            children: vec![Inline::Text {
                value: "BEFORE".into(),
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        table,
        Block::Paragraph {
            children: vec![Inline::Text {
                value: "AFTER".into(),
            }],
            layout: LayoutHint::default(),
            source: None,
        },
    ];
    assert_eq!(renderer.render_blocks(&surrounding, 0), "BEFORE\n\nAFTER");
}

fn plain_list(blocks: Vec<Block>, indent: i32) -> Block {
    Block::List {
        kind: ListKind::Plain,
        compact: true,
        items: vec![ListItem {
            layout: mant_ir::ListItemLayout::default(),
            blocks,
            source: None,
            entry: None,
        }],
        layout: LayoutHint {
            indent_columns: indent,
            ..Default::default()
        },
        source: None,
    }
}

#[test]
fn resolved_gaps_cross_transparent_containers_and_precede_whole_items() {
    let renderer = super::super::plain_renderer();
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
                        terms: vec![vec![Inline::Text {
                            value: "TERM".into(),
                        }]],
                        description: vec![paragraph("BODY", -2)],
                        source: None,
                        entry: None,
                        layout: mant_ir::DefinitionLayout::default(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                },
                Block::Table {
                    column_widths: Vec::new(),
                    rows: vec![mant_ir::TableRow {
                        kind: mant_ir::TableRowKind::Data,
                        cells: vec![TableCell {
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
    let renderer = super::super::plain_renderer();
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

#[test]
fn distinct_term_roots_and_hard_lines_do_not_acquire_commas() {
    let blocks = [Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            terms: ["-a", "--all"]
                .map(|value| {
                    vec![Inline::Text {
                        value: value.into(),
                    }]
                })
                .into(),
            description: vec![paragraph("FIRST\nCONTINUATION", 0)],
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                head_body_relation: mant_ir::HeadBodyRelation::from(true),
                ..Default::default()
            },
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    assert_eq!(
        super::super::plain_renderer().render_blocks(&blocks, 0),
        "-a\n--all FIRST\n    CONTINUATION"
    );
}

#[test]
fn zero_width_terms_and_clipped_markers_do_not_move_body_text() {
    let renderer = super::super::plain_renderer();
    let mut block = plain_list(vec![paragraph("BODY", 4)], -5);
    let Block::List { kind, .. } = &mut block else {
        unreachable!()
    };
    *kind = ListKind::Bullet;
    // Pinned mdoc_term.c::termp_it_pre uses a bullet glyph for Bl -bullet;
    // its UTF-8 spelling was checked with the one-item list probe.
    assert_eq!(renderer.render_blocks(&[block], 0), "•\n BODY");
    let block = Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            source: None,
            entry: None,
            terms: vec![
                vec![Inline::anchor_at("target", None)],
                vec![Inline::Text {
                    value: "TERM".into(),
                }],
            ],
            description: vec![paragraph("BODY", 0)],
            layout: mant_ir::DefinitionLayout {
                head_body_relation: mant_ir::HeadBodyRelation::from(true),
                ..Default::default()
            },
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(renderer.render_blocks(&[block], 0), "TERM BODY");
}

#[test]
fn empty_head_prefix_rows_preserve_the_recorded_run_in_body_origin() {
    // Exact .Bl -hang -width 12n / .No \\z / .sp 1 or 2 / .An -split /
    // .An Bob / .No BODY ran pristine before this assertion: the empty
    // endline is real, but did not print or wrap the label (term.c:489-497).
    // Its accepted "ob" therefore retains BODY column 14, not only gap 1.
    for (prefix, prefix_rows, expected_gap) in
        [("", 1, 12), ("", 2, 12), ("HEAD", 1, 1), (" ", 1, 1)]
    {
        let mut head = vec![Inline::anchor("empty-head-origin")];
        head.push(Inline::Strong {
            children: vec![Inline::Text {
                value: prefix.into(),
            }],
        });
        head.extend((0..prefix_rows).map(|_| Inline::line_break()));
        head.push(Inline::Text { value: "ob".into() });
        let block = Block::DefinitionList {
            declaration_groups: vec![],
            compact: true,
            items: vec![DefinitionItem {
                source: None,
                entry: None,
                terms: vec![head],
                description: vec![paragraph("BODY", 0)],
                layout: mant_ir::DefinitionLayout {
                    head_body_relation: mant_ir::HeadBodyRelation::RunIn,
                    body_indent_columns: 14,
                    min_term_gap_columns: 1,
                    spacing_before_lines: None,
                },
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        for decorated in [false, true] {
            let paint = |_: TextPresentation, value: &str| {
                if decorated {
                    format!("\x1b[1m{value}\x1b[0m")
                } else {
                    value.into()
                }
            };
            let renderer = BlockRenderer {
                names: None,
                locations: None,
                decorate: &paint,
            };
            let output = renderer
                .render_blocks(std::slice::from_ref(&block), 0)
                .replace("\x1b[1m", "")
                .replace("\x1b[0m", "");
            assert_eq!(
                output,
                format!(
                    "{prefix}{}ob{}BODY",
                    "\n".repeat(prefix_rows),
                    " ".repeat(expected_gap)
                )
            );
        }
    }
}

fn declared_column_table(widths: &[u16], cells: &[&str]) -> Block {
    Block::Table {
        column_widths: widths.to_vec(),
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: cells
                .iter()
                .map(|text| TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph(text, 0)],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                })
                .collect(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn declared_columns_pad_to_their_declared_offsets() {
    let renderer = super::super::plain_renderer();
    // mdoc_term.c::termp_it_pre (709-715): column 1 starts after the
    // first declared width plus the dcol gap of 4 (fewer than five
    // columns). Verified against the fixed -Tutf8 reference.
    let block = declared_column_table(&[3, 3], &["A", "B"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "A      B");
}

#[test]
fn declared_column_advances_truncate_at_256_columns() {
    let renderer = super::super::plain_renderer();
    // term_ascii.c::ascii_advance() truncates one advance at 256
    // columns; the fixed reference puts B 257 columns after A for a
    // 300-column declaration (300 + 4 - 1 = 303, clamped to 256).
    let block = declared_column_table(&[300, 10], &["A", "B"]);
    let expected = format!("A{}B", " ".repeat(256));
    assert_eq!(renderer.render_blocks(&[block], 0), expected);
    // A gap of exactly 255 remains unclamped.
    let block = declared_column_table(&[252, 10], &["A", "B"]);
    assert_eq!(
        renderer.render_blocks(&[block], 0),
        format!("A{}B", " ".repeat(255))
    );
}

#[test]
fn saturated_column_widths_keep_bounded_and_aligned_projection() {
    let renderer = super::super::plain_renderer();
    // External audit CW01: 65531/65532/65535/65536-column declarations
    // must neither overflow u16 arithmetic nor explode padding. Every
    // column boundary still advances one truncated 256-column step.
    for width in [65531_u16, 65532, 65535, u16::MAX] {
        let block = declared_column_table(&[width, width], &["A", "B"]);
        assert_eq!(
            renderer.render_blocks(&[block], 0),
            format!("A{}B", " ".repeat(256)),
            "width {width}"
        );
        let block = declared_column_table(&[width, width, width], &["A", "B", "C"]);
        assert_eq!(
            renderer.render_blocks(&[block], 0),
            format!("A{}B{}C", " ".repeat(256), " ".repeat(256)),
            "width {width}"
        );
    }
    // Zero and one column declarations keep the dcol-only separation.
    let block = declared_column_table(&[0, 0], &["A", "B"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "A   B");
    let block = declared_column_table(&[1, 1], &["A", "B"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "A    B");
}

#[test]
fn full_column_starts_wrap_to_the_next_physical_line() {
    let renderer = super::super::plain_renderer();
    // External audit CW03/CW04: content that already fills the next
    // column's start moves that column to a fresh physical line at its
    // declared start. The fixed reference renders `.Bl -column one two`
    // with `.It AAAA BB Ta C` as `AAAA BB` then `C` at the declared
    // column offset (term.c moves the field once viscol reaches the
    // field's rmargin).
    let block = declared_column_table(&[3, 3], &["AAAA BB", "C"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "AAAA BB\n       C");
    // Ending one column earlier keeps the same-line continuation.
    let block = declared_column_table(&[3, 3], &["AAAA B", "C"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "AAAA B C");
    // Overrunning content survives untruncated and later columns keep
    // their boundaries (reference: `.It AAAA BBX Ta C`).
    let block = declared_column_table(&[3, 3], &["AAAA BBX", "C"]);
    assert_eq!(renderer.render_blocks(&[block], 0), "AAAA BBX\n       C");
}

#[test]
fn undeclared_cells_keep_their_source_order_without_an_invented_stride() {
    let renderer = super::super::plain_renderer();
    // termp_it_pre caps its preceding-offset loop at ncols. Exact roff
    // oracle input was run first; the parser joins trailing Ta operands,
    // while this source-neutral IR deliberately keeps four actual cells.
    let block = declared_column_table(&[3, 3], &["A", "B", "C", "D"]);
    assert_eq!(
        renderer.render_blocks(&[block], 0),
        "A      B      C\n              D"
    );
    // CW02's many-cell boundary reaches the shared dense-placement
    // budget and uses existing source-order stack output. No derived
    // stride multiplication or large padding allocation is permitted.
    let cells: Vec<&str> = (0..4700).map(|_| "x").collect();
    let block = declared_column_table(&[3, 3], &cells);
    let text = renderer.render_blocks(&[block], 0);
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 4700);
    assert!(lines.iter().all(|line| *line == "x"));
    assert!(text.len() <= 4700 * 2);
}

#[test]
fn preserved_public_ir_padding_keeps_plain_and_ansi_cursors_monotonic() {
    let block = declared_column_table(&[3, 3], &["X          ", "CLICK"]);
    let expected = "X          CLICK";
    let plain = super::super::plain_renderer().render_blocks(std::slice::from_ref(&block), 0);
    assert_eq!(plain, expected);
    let decorated = BlockRenderer {
        locations: None,
        names: None,
        decorate: &|_, text| format!("\u{1b}[1m{text}\u{1b}[0m"),
    };
    let text = decorated.render_blocks(&[block], 0);
    assert_eq!(
        text.replace("\u{1b}[1m", "").replace("\u{1b}[0m", ""),
        expected
    );
}

#[test]
fn decorated_columns_measure_before_ansi_styling() {
    // External audit §18.3: positioning must measure the projection
    // before the decorator adds zero-width ANSI styles; the decorator
    // contract only preserves visible text, never byte length.
    let decorated = BlockRenderer {
        locations: None,
        names: None,
        decorate: &|_, text| {
            if text.trim().is_empty() {
                text.to_owned()
            } else {
                format!("\x1b[1m{text}\x1b[0m")
            }
        },
    };
    let block = declared_column_table(&[10, 10], &["styled", "cells"]);
    let projected = decorated.render_blocks(std::slice::from_ref(&block), 0);
    let plain = super::super::plain_renderer().render_blocks(&[block], 0);
    let stripped: String = projected
        .split("\x1b[1m")
        .map(|rest| rest.replace("\x1b[0m", ""))
        .collect();
    assert_eq!(stripped, plain);
}
