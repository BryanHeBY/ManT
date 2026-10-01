use super::*;

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
    let renderer = super::super::super::plain_renderer();
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
    let renderer = super::super::super::plain_renderer();
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
            let mut expected = value
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
            // The exact man/mdoc literal EOF matrix ran the pristine oracle
            // before this assertion. An empty last row needs its own closing
            // delimiter, rather than borrowing the previous row's close.
            if value.is_empty() || value.ends_with('\n') {
                expected.push('\n');
            }
            assert_eq!(
                renderer.render_blocks(&[block], i32::try_from(origin).unwrap()),
                expected
            );
        }
    }
}

#[test]
fn literal_typed_row_ends_match_embedded_newlines_across_block_joins() {
    // The exact pristine Bd -literal control X / empty TEXT / Ed / Y
    // preserves X\n\nY: term_vspace() asserts the empty row, while
    // termp_bd_post() only closes it (term.c:475-497, mdoc_term.c:1474).
    // In source-neutral IR the producer has already removed native closes;
    // a typed literal LineBreak is content, including inside style wrappers.
    let renderer = super::super::super::plain_renderer();
    for nested in [false, true] {
        for ending in [
            Inline::Text { value: "\n".into() },
            Inline::line_break(),
            Inline::Strong {
                children: vec![Inline::Emphasis {
                    children: vec![Inline::line_break()],
                }],
            },
        ] {
            let literal = Block::Preformatted {
                children: vec![Inline::Text { value: "X".into() }, ending],
                language: None,
                layout: LayoutHint::default(),
                source: None,
            };
            let first = if nested {
                plain_list(vec![literal], 0)
            } else {
                literal
            };
            assert_eq!(
                renderer.render_blocks(&[first, paragraph("Y", 0)], 0),
                "X\n\nY",
                "nested={nested}"
            );
        }
    }
}

#[test]
fn paragraph_preserves_a_formatter_generated_leading_line_break() {
    let renderer = super::super::super::plain_renderer();
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
    let renderer = super::super::super::plain_renderer();
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
    let renderer = super::super::super::plain_renderer();
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
    let renderer = super::super::super::plain_renderer();
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
