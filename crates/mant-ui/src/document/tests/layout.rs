//! Existing regressions grouped by layout behavior; expected values remain independent.
use super::*;

#[test]
fn resolved_gaps_precede_whole_items_and_share_transparent_container_budgets() {
    for rows in [0, 1, 2, 3000] {
        let mut builder = DocumentBuilder::new("gaps".into(), None, None);
        builder.blocks(
            &[Block::List {
                kind: ListKind::Bullet,
                compact: true,
                items: vec![ListItem {
                    layout: mant_ir::ListItemLayout::default(),
                    source: None,
                    entry: None,
                    blocks: vec![Block::Paragraph {
                        children: vec![crate::test_content::text("BODY\nNEXT")],
                        layout: LayoutHint {
                            spacing_before_lines: rows,
                            ..Default::default()
                        },
                        source: None,
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            0,
        );
        assert_eq!(builder.lines.len(), usize::from(rows) + 2);
        let text = builder.lines[usize::from(rows)]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert_eq!(text, "• BODY");
    }
    let mut builder = DocumentBuilder::new("bounded".into(), None, None);
    builder.blocks(
        &[Block::List {
            kind: ListKind::Plain,
            compact: true,
            layout: LayoutHint {
                spacing_before_lines: 3000,
                ..Default::default()
            },
            source: None,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![
                    Block::VerticalSpace {
                        lines: 3000,
                        source: None,
                    },
                    Block::Paragraph {
                        children: vec![crate::test_content::text("BODY")],
                        layout: LayoutHint::default(),
                        source: None,
                    },
                ],
            }],
        }],
        0,
    );
    assert_eq!(builder.lines.len(), 4097);
}

#[test]
fn anchors_follow_hard_lines_in_terms_and_run_in_bodies() {
    for inline_term in [false, true] {
        let mut builder = DocumentBuilder::new("target-rows".into(), None, None);
        builder.blocks(
            &[Block::DefinitionList {
                declaration_groups: Vec::new(),
                compact: true,
                items: vec![DefinitionItem {
                    source: None,
                    entry: None,
                    layout: mant_ir::DefinitionLayout {
                        inline_term,
                        ..Default::default()
                    },
                    terms: vec![vec![
                        crate::test_content::text("FIRST"),
                        crate::test_content::line_break(),
                        Inline::Strong {
                            children: vec![
                                crate::test_content::anchor("second-head"),
                                crate::test_content::text("SECOND"),
                            ],
                        },
                    ]],
                    description: vec![Block::Paragraph {
                        children: vec![
                            crate::test_content::anchor("body"),
                            crate::test_content::text("BODY\n"),
                            crate::test_content::anchor("next-body"),
                            crate::test_content::text("NEXT"),
                        ],
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            0,
        );
        assert_eq!(builder.anchors["second-head"], 1);
        let first_body = if inline_term { 1 } else { 2 };
        assert_eq!(builder.anchors["body"], first_body);
        assert_eq!(builder.anchors["next-body"], first_body + 1);
    }
    let mut builder = DocumentBuilder::new("list-target".into(), None, None);
    builder.blocks(
        &[Block::List {
            kind: ListKind::Bullet,
            compact: true,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![Block::Paragraph {
                    children: vec![
                        crate::test_content::anchor("first"),
                        crate::test_content::text("FIRST"),
                        crate::test_content::line_break(),
                        crate::test_content::anchor("second"),
                        crate::test_content::text("SECOND"),
                    ],
                    layout: LayoutHint {
                        continuation_indent_columns: 3,
                        ..Default::default()
                    },
                    source: None,
                }],
            }],
            layout: LayoutHint::default(),
            source: None,
        }],
        0,
    );
    assert_eq!(builder.anchors["first"], 0);
    assert_eq!(builder.anchors["second"], 1);
    assert_eq!(builder.lines[1].indent, 5);
}

#[test]
fn hanging_paragraph_preserves_hard_and_soft_continuation_origins() {
    let mut builder = DocumentBuilder::new("hanging".into(), None, None);
    builder.blocks(
        &[Block::Paragraph {
            children: vec![
                crate::test_content::text("FIRST words words words"),
                crate::test_content::line_break(),
                crate::test_content::text("SECOND"),
            ],
            layout: LayoutHint {
                indent_columns: 2,
                continuation_indent_columns: 7,
                ..Default::default()
            },
            source: None,
        }],
        3,
    );
    assert_eq!(
        (
            builder.lines[0].indent,
            builder.lines[0].continuation_indent
        ),
        (5, 12)
    );
    assert_eq!(
        (
            builder.lines[1].indent,
            builder.lines[1].continuation_indent
        ),
        (12, 12)
    );
    for (width, first, continuation) in [(20, 3, 10), (40, 5, 12), (80, 5, 12)] {
        let rows = wrap_line(&builder.lines[0], width);
        assert!(
            rows[0]
                .to_string()
                .starts_with(&format!("{}FIRST", " ".repeat(first)))
        );
        for row in rows.iter().skip(1) {
            assert!(row.to_string().starts_with(&" ".repeat(continuation)));
        }
    }
}

#[test]
fn container_translation_and_nonparagraph_marker_width_are_preserved() {
    for start in [9, 99, u64::MAX] {
        let blocks = [Block::List {
            kind: ListKind::Ordered { start: Some(start) },
            compact: true,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![Block::Preformatted {
                    children: vec![crate::test_content::text("CODE")],
                    language: None,
                    layout: LayoutHint::default(),
                    source: None,
                }],
            }],
            layout: LayoutHint {
                indent_columns: 3,
                ..Default::default()
            },
            source: None,
        }];
        let before = blocks.clone();
        for shift in [0, 2, 5] {
            let mut builder = DocumentBuilder::new("translation".into(), None, None);
            builder.blocks(&blocks, shift);
            assert_eq!(builder.lines[0].indent, usize::try_from(shift + 3).unwrap());
            let marker_width = format!("{start}. ").len();
            assert_eq!(
                builder.lines[1].indent,
                usize::try_from(shift + 3).unwrap() + marker_width
            );
            assert_eq!(
                builder.lines[1].continuation_indent,
                builder.lines[1].indent
            );
        }
        assert_eq!(blocks, before);
    }
    let mut child = paragraph("OUTDENT");
    let Block::Paragraph { layout, .. } = &mut child else {
        unreachable!()
    };
    layout.indent_columns = 3;
    let mut builder = DocumentBuilder::new("signed".into(), None, None);
    builder.blocks(
        &[Block::List {
            kind: ListKind::Plain,
            compact: true,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![child],
            }],
            layout: LayoutHint {
                indent_columns: -2,
                ..Default::default()
            },
            source: None,
        }],
        0,
    );
    assert_eq!(
        builder.lines[0].indent, 1,
        "do not clamp a container before composing its child"
    );
}

#[test]
fn outdented_list_paragraph_keeps_links_on_the_visible_body() {
    let mut builder = DocumentBuilder::new("outdent".into(), None, None);
    builder.blocks(
        &[Block::List {
            kind: ListKind::Bullet,
            compact: true,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![Block::Paragraph {
                    children: vec![
                        crate::test_content::link(
                            mant_ir::LinkTarget::Section {
                                id: "target".into(),
                            },
                            None,
                            vec![crate::test_content::text("LINK")],
                        ),
                        crate::test_content::line_break(),
                        crate::test_content::text("CONTINUED"),
                    ],
                    layout: LayoutHint {
                        indent_columns: -1,
                        ..Default::default()
                    },
                    source: None,
                }],
            }],
            layout: LayoutHint::default(),
            source: None,
        }],
        5,
    );
    assert_eq!(builder.lines[0].indent, 5);
    assert_eq!(builder.lines[1].indent, 6);
    assert_eq!(builder.lines[2].indent, 6);
    assert_eq!(builder.lines[1].links[0].start_scalar, 0);
    assert_eq!(builder.lines[1].links[0].end_scalar, 4);
    assert_eq!(
        builder.lines[1]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        "LINK"
    );
}

#[test]
fn target_only_terms_are_zero_width_and_extreme_origins_are_bounded() {
    for inline_term in [false, true] {
        for origin in [0, i32::MAX, i32::MIN] {
            let mut builder = DocumentBuilder::new("targets".into(), None, None);
            builder.blocks(
                &[Block::DefinitionList {
                    declaration_groups: vec![],
                    compact: true,
                    items: vec![DefinitionItem {
                        source: None,
                        entry: None,
                        terms: vec![
                            vec![crate::test_content::anchor("target")],
                            vec![crate::test_content::text("TERM")],
                        ],
                        description: vec![paragraph("BODY")],
                        layout: mant_ir::DefinitionLayout {
                            inline_term,
                            ..Default::default()
                        },
                    }],
                    layout: LayoutHint {
                        indent_columns: origin,
                        ..Default::default()
                    },
                    source: None,
                }],
                0,
            );
            assert_eq!(builder.anchors.get("target"), Some(&0));
            assert!(!builder.lines[0].spans.is_empty());
            assert!(
                builder
                    .lines
                    .iter()
                    .all(|line| line.indent <= 4096 && line.continuation_indent <= 4096)
            );
        }
    }
}

#[test]
fn trailing_zero_width_heads_share_the_final_run_in_row() {
    for trailing_count in [1, 2, 3] {
        let mut terms = vec![vec![crate::test_content::text("TERM")]];
        terms.extend(
            (0..trailing_count).map(|i| vec![crate::test_content::anchor(format!("target-{i}"))]),
        );
        let mut builder = DocumentBuilder::new("trailing-targets".into(), None, None);
        builder.blocks(
            &[Block::DefinitionList {
                declaration_groups: vec![],
                compact: true,
                items: vec![DefinitionItem {
                    source: None,
                    entry: None,
                    terms,
                    description: vec![paragraph("BODY")],
                    layout: mant_ir::DefinitionLayout {
                        inline_term: true,
                        ..Default::default()
                    },
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            0,
        );
        assert_eq!(builder.lines.len(), 1);
        for i in 0..trailing_count {
            assert_eq!(builder.anchors.get(&format!("target-{i}")), Some(&0));
        }
    }
}

#[test]
fn definition_continuations_keep_rows_when_reparented_across_spacing() {
    for inline_term in [false, true] {
        for label in ["-a", "--long-option", "界", "e\u{301}"] {
            for width in [18, 100] {
                let mut document = bundle();
                let mut continuation = paragraph("Continuation.");
                let Block::Paragraph { layout, .. } = &mut continuation else {
                    unreachable!()
                };
                layout.indent_columns = i32::from(DefinitionItem::DESCRIPTION_INDENT_COLUMNS);
                let space = Block::VerticalSpace {
                    lines: 2,
                    source: None,
                };
                let definition = Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    items: vec![DefinitionItem {
                        source: None,
                        entry: None,
                        terms: vec![vec![crate::test_content::text(label)]],
                        description: vec![paragraph("Initial description.")],
                        layout: mant_ir::DefinitionLayout {
                            inline_term,
                            spacing_before_lines: None,
                            ..Default::default()
                        },
                    }],
                    compact: false,
                    layout: LayoutHint::default(),
                    source: None,
                };
                document.document.as_mut().unwrap().sections[0].blocks = vec![
                    definition,
                    space.clone(),
                    continuation.clone(),
                    paragraph("Outside."),
                ];
                let before = DocumentView::new(&document).render(width).text.to_string();
                let blocks = &mut document.document.as_mut().unwrap().sections[0].blocks;
                blocks.drain(1..3);
                let Block::DefinitionList { items, .. } = &mut blocks[0] else {
                    unreachable!()
                };
                let Block::Paragraph { layout, .. } = &mut continuation else {
                    unreachable!()
                };
                layout.indent_columns = 0;
                items[0].description.extend([space, continuation]);
                let after = DocumentView::new(&document).render(width).text.to_string();
                assert_eq!(before, after);
            }
        }
    }
}

#[test]
fn preformatted_rows_share_one_full_width_surface() {
    let mut builder = DocumentBuilder::new("demo".to_owned(), None, None);
    builder.inline_lines_with_surface(
        &[
            crate::test_content::text("short".to_owned()),
            crate::test_content::line_break(),
            crate::test_content::text("longer code".to_owned()),
        ],
        3,
        Style::default().fg(theme::TEXT),
        LineSurface::Code,
    );

    let rows = builder
        .lines
        .iter()
        .flat_map(|line| wrap_line(line, 40))
        .collect::<Vec<_>>();

    assert_eq!(rows.len(), 2);
    assert!(rows[0].to_string().starts_with("   short"));
    assert!(rows[1].to_string().starts_with("   longer code"));
    assert_eq!(UnicodeWidthStr::width(rows[0].to_string().as_str()), 40);
    assert_eq!(UnicodeWidthStr::width(rows[1].to_string().as_str()), 40);
    assert_eq!(rows[0].spans[0].style.bg, None);
    assert_eq!(
        rows[0].spans.last().and_then(|span| span.style.bg),
        Some(theme::SURFACE)
    );
}

#[test]
fn preformatted_character_wrapping_preserves_significant_spaces() {
    let line = LogicalLine::plain(2, "ab  cd", Style::default())
        .surface(LineSurface::Code)
        .wrap_mode(WrapMode::Character);
    let rows = wrap_line(&line, 7);

    assert_eq!(&rows[0].to_string()[..7], "  ab  c");
    assert!(rows[1].to_string().starts_with("  d"));
}

#[test]
fn indented_continuation_without_spacing_follows_its_lead_row() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![
        paragraph("alternate object database"),
        Block::Paragraph {
            children: vec![crate::test_content::text(
                "Via the alternates mechanism, a repository can inherit objects.".to_owned(),
            )],
            layout: LayoutHint {
                indent_columns: 4,
                spacing_before_lines: 0,
                ..Default::default()
            },
            source: None,
        },
    ];

    let rendered = DocumentView::new(&bundle).render(100);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let term = rows
        .iter()
        .position(|row| row.trim() == "alternate object database")
        .expect("visible glossary term");

    assert!(
        rows.get(term + 1)
            .is_some_and(|row| row.trim_start().starts_with("Via the alternates mechanism")),
        "an indented continuation with zero spacing must occupy the next row: {rows:?}",
    );
}

#[test]
fn wrapped_rows_preserve_their_indent() {
    let line = LogicalLine::plain(3, "abcdefgh", Style::default());
    let rows = wrap_line(&line, 7);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].to_string(), "   abcd");
    assert_eq!(rows[1].to_string(), "   efgh");
}

#[test]
fn wrapping_prefers_word_boundaries() {
    let line = LogicalLine::plain(2, "alpha beta", Style::default());
    let rows = wrap_line(&line, 8);

    assert_eq!(rows[0].to_string(), "  alpha");
    assert_eq!(rows[1].to_string(), "  beta");
}

#[test]
fn code_surfaces_fill_the_document_width_after_the_body_indent() {
    let line = LogicalLine::plain(3, "code", Style::default()).surface(LineSurface::Code);
    let rows = wrap_line(&line, 12);

    assert_eq!(UnicodeWidthStr::width(rows[0].to_string().as_str()), 12);
    assert_eq!(rows[0].spans[0].content, "   ");
    assert_eq!(rows[0].spans[0].style.bg, None);
    assert_eq!(rows[0].spans[1].style.bg, Some(theme::SURFACE));
    assert_eq!(rows[0].spans.last().expect("surface fill").content, "     ");
}

#[test]
fn bullet_lists_share_the_first_row_and_use_a_hanging_indent() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            layout: mant_ir::ListItemLayout::default(),
            source: None,
            entry: None,
            blocks: vec![Block::Paragraph {
                children: vec![crate::test_content::text("alpha beta gamma".to_owned())],
                layout: LayoutHint::default(),
                source: None,
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&bundle).render(16);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rows[1], "   • alpha beta");
    assert_eq!(rows[2], "     gamma");
}

#[test]
fn adjacent_blocks_add_only_explicit_vertical_space() {
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![crate::test_content::text(value.to_owned())],
        layout: LayoutHint::default(),
        source: None,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![
        paragraph("before"),
        Block::Preformatted {
            children: vec![crate::test_content::text("display".to_owned())],
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
        paragraph("after"),
        Block::VerticalSpace {
            lines: 1,
            source: None,
        },
        paragraph("spaced"),
    ];

    let rows = DocumentView::new(&bundle)
        .render(80)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let before = rows.iter().position(|row| row.contains("before")).unwrap();
    let display = rows.iter().position(|row| row.contains("display")).unwrap();
    let after = rows.iter().position(|row| row.contains("after")).unwrap();
    let spaced = rows.iter().position(|row| row.contains("spaced")).unwrap();

    assert_eq!(display, before + 1);
    assert_eq!(after, display + 1);
    assert_eq!(spaced, after + 2);
}

#[test]
fn ordered_list_markers_saturate_instead_of_overflowing() {
    let mut bundle = bundle();
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![crate::test_content::text(value.to_owned())],
        layout: LayoutHint::default(),
        source: None,
    };
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::List {
        kind: ListKind::Ordered {
            start: Some(u64::MAX),
        },
        compact: true,
        items: vec![
            ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![paragraph("first")],
            },
            ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![paragraph("second")],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];

    let output = DocumentView::new(&bundle).render(80).text.to_string();
    assert_eq!(output.matches("18446744073709551615. ").count(), 2);
}
