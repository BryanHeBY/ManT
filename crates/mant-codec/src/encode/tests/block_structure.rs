use super::*;

#[test]
fn preserves_inline_lists_definitions_and_nested_headings() {
    let rich_paragraph = paragraph(vec![
        Inline::Strong {
            children: vec![Inline::Text {
                value: " demo ".to_owned(),
            }],
        },
        Inline::Text {
            value: "reads ".to_owned(),
        },
        Inline::Emphasis {
            children: vec![Inline::Text {
                value: "files".to_owned(),
            }],
        },
        Inline::Text {
            value: " with ".to_owned(),
        },
        Inline::Code {
            value: "a`b".to_owned(),
        },
        Inline::line_break(),
        Inline::Text {
            value: " a second line; see <<https://example.com/docs>>. ".to_owned(),
        },
    ]);
    let list = Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            layout: mant_ir::ListItemLayout::default(),
            source: None,
            entry: None,
            blocks: vec![paragraph(vec![Inline::Text {
                value: "first item".to_owned(),
            }])],
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let definitions = Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            head_body_relation: mant_ir::HeadBodyRelation::from(false),
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                spacing_before_lines: None,
                ..Default::default()
            },
            terms: (vec![
                vec![Inline::Strong {
                    children: vec![Inline::Text {
                        value: "-a".to_owned(),
                    }],
                }],
                vec![Inline::Strong {
                    children: vec![Inline::Text {
                        value: "--all".to_owned(),
                    }],
                }],
            ])
            .into_iter()
            .map(Into::into)
            .collect(),
            description: vec![paragraph(vec![Inline::Text {
                value: "Show all entries.".to_owned(),
            }])],
        }],
        compact: false,
        layout: LayoutHint::default(),
        source: None,
    };
    let query = ResolvedContent {
        address: None,
        label: "demo * command".to_owned(),
        document: Some(manual(vec![section(
            "OPTIONS",
            vec![rich_paragraph, list, definitions],
            vec![section("DETAILS", Vec::new(), Vec::new())],
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# demo \\* command"));
    assert!(markdown.contains("## OPTIONS"));
    assert!(markdown.contains("### DETAILS"));
    assert!(markdown.contains("**demo** reads *files* with ``a`b``"));
    assert!(markdown.contains("a second line; see <https://example.com/docs>."));
    assert!(markdown.contains("- first item"));
    assert!(markdown.contains("- **-a**  \n  **--all**"));
    assert!(markdown.contains("Show all entries."));
}

#[test]
fn chooses_safe_fences_and_preserves_native_table_and_equation_content() {
    let query = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(manual(vec![section(
            "DATA",
            vec![
                Block::Preformatted {
                    inline_layout: mant_ir::InlineLayout::default(),
                    children: vec![
                        Inline::Text {
                            value: "before ``` marker".to_owned(),
                        },
                        Inline::line_break(),
                        Inline::Strong {
                            children: vec![Inline::Text {
                                value: "after".to_owned(),
                            }],
                        },
                    ],
                    language: None,
                    layout: LayoutHint::default(),
                    source: None,
                },
                Block::Table {
                    column_preferences: mant_ir::ColumnPreferences::default(),
                    rows: vec![TableRow {
                        kind: mant_ir::TableRowKind::Data,
                        cells: vec![
                            TableCell {
                                break_after: false,
                                kind: mant_ir::TableCellKind::Text,
                                blocks: vec![paragraph(vec![Inline::Text {
                                    value: "left".to_owned(),
                                }])],
                                column_span: 1,
                                row_span: 1,
                                alignment: None,
                            },
                            TableCell {
                                break_after: false,
                                kind: mant_ir::TableCellKind::Text,
                                blocks: vec![paragraph(vec![Inline::Text {
                                    value: "right".to_owned(),
                                }])],
                                column_span: 1,
                                row_span: 1,
                                alignment: None,
                            },
                        ],
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                },
                Block::Equation {
                    value: "x = y + 1".to_owned(),
                    expression: None,
                    display: true,
                    layout: LayoutHint::default(),
                    source: None,
                },
            ],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("````\nbefore ``` marker\nafter\n````"));
    assert!(!markdown.contains("**after**"));
    assert!(markdown.contains("```\nleft | right\n```"));
    assert!(markdown.contains("```math\nx = y + 1\n```"));
}

#[test]
fn protects_hanging_definition_terms_from_becoming_nested_lists() {
    let query = ResolvedContent {
        address: None,
        label: "definition-markers".to_owned(),
        document: Some(manual(vec![section(
            "NOTES",
            vec![Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![DefinitionItem {
                    head_body_relation: mant_ir::HeadBodyRelation::from(true),
                    source: None,
                    entry: None,
                    terms: (vec![vec![Inline::Text {
                        value: "1.".to_owned(),
                    }]])
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                    description: vec![paragraph(vec![Inline::Text {
                        value: "first reference".to_owned(),
                    }])],
                    layout: mant_ir::DefinitionLayout {
                        body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                        spacing_before_lines: None,
                        ..Default::default()
                    },
                }],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("- 1\\. first reference"), "{markdown}");
    let lists = Parser::new(&markdown)
        .filter(|event| matches!(event, Event::Start(Tag::List(_))))
        .count();
    assert_eq!(lists, 1, "the definition owns one bullet list only");
}

#[test]
fn preserves_definition_prose_row_relation_in_both_markdown_projections() {
    // mdoc_term.c::termp_it_post executes term_newln for a completed TAG
    // HEAD. The exact tag_explicit_vspace source is independently exercised
    // by engine consumer tests; here the exporter must preserve the final
    // IR's row decision even with styled prose and both display projections.
    for inline in [false, true] {
        let query = ResolvedContent {
            address: None,
            label: "definition-rows".to_owned(),
            document: Some(manual(vec![section(
                "TEXT",
                vec![Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    items: vec![DefinitionItem {
                        head_body_relation: mant_ir::HeadBodyRelation::from(inline),
                        source: None,
                        entry: None,
                        layout: mant_ir::DefinitionLayout {
                            body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                            ..Default::default()
                        },
                        terms: (vec![vec![Inline::Text {
                            value: "HeadWord".to_owned(),
                        }]])
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                        description: vec![paragraph(vec![Inline::Emphasis {
                            children: vec![Inline::Text {
                                value: "BodyWord".to_owned(),
                            }],
                        }])],
                    }],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                Vec::new(),
            )])),
            tldr: None,
        };
        {
            let markdown = render_markdown_with_options(&query, MarkdownOptions::default());
            assert_eq!(
                Parser::new(&markdown)
                    .filter(|event| matches!(event, Event::HardBreak))
                    .count(),
                usize::from(!inline),
                "inline={inline}: {markdown}"
            );
            assert!(
                !Parser::new(&markdown).any(|event| matches!(event, Event::SoftBreak)),
                "inline={inline}: {markdown}"
            );
        }
    }
}

#[test]
fn keeps_block_definition_descriptions_on_their_own_commonmark_line() {
    let definitions = Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            head_body_relation: mant_ir::HeadBodyRelation::from(true),
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                spacing_before_lines: None,
                ..Default::default()
            },
            terms: (vec![vec![Inline::Text {
                value: "plain".to_owned(),
            }]])
            .into_iter()
            .map(Into::into)
            .collect(),
            description: vec![Block::Preformatted {
                inline_layout: mant_ir::InlineLayout::default(),
                children: vec![Inline::Text {
                    value: "code_line();".to_owned(),
                }],
                language: None,
                layout: LayoutHint::default(),
                source: None,
            }],
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    };
    let query = ResolvedContent {
        address: None,
        label: "definition".to_owned(),
        document: Some(manual(vec![section("TEXT", vec![definitions], Vec::new())])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains("- plain\n  ```\n  code_line();\n  ```"),
        "{markdown}"
    );
    assert!(!markdown.contains("plain ```"));
    assert_eq!(
        Parser::new(&markdown)
            .filter(|event| matches!(event, Event::Start(Tag::CodeBlock(_))))
            .count(),
        1
    );
}

#[cfg(all(unix, feature = "roff"))]
#[test]
fn serializes_a_large_source_lowered_document() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../libmandoc-rs/vendor/mandoc-cvs-20260927T130954Z/mandoc.1");
    if !source.exists() {
        // Published package tests must not require a sibling crate's vendor
        // tree; repository verification still exercises the real fixture.
        return;
    }
    let bytes = std::fs::read(&source).expect("read plain native fixture");
    let document =
        crate::mandoc::parse_plain_manual(&source, &bytes).expect("large native document");
    let query = ResolvedContent {
        address: None,
        label: "mandoc".to_owned(),
        document: Some(document),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# mandoc\n"));
    assert!(markdown.contains("## NAME"));
    assert!(markdown.contains("## DESCRIPTION"));
    assert!(!markdown.contains("<pre"));
}

#[test]
fn detached_fragment_export_accepts_ir_beyond_markdown_metadata_limits() {
    let names: Vec<_> = (0..34).map(|index| format!("--alias{index}")).collect();
    let head = names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let source =
        format!("<!-- mant:entries role=option case=sensitive -->\n- {head}: Kept body.\n");
    let mut content = parse_content(&source, None).unwrap();
    let document = content.document.as_mut().unwrap();
    let Block::List { items, .. } = &mut document.blocks[0] else {
        panic!("semantic list");
    };
    items[0].entry.as_mut().unwrap().alias_groups = vec![names.clone()];
    assert_eq!(mant_ir::validate_document(document).len(), 0);
    assert!(!super::semantic::supported(document));
    for preserve_anchors in [false, true] {
        let options = super::MarkdownFragmentOptions { preserve_anchors };
        let plain = super::render_blocks_fragment(&document.blocks, options).join("\n\n");
        let located =
            super::render_located_blocks_fragment(&document.blocks, options, None).join("\n\n");
        assert_eq!(plain, located);
        assert!(plain.contains("Kept body."));
        assert!(!plain.contains("mant:entry"));
        assert!(!plain.contains("mant:entries"));
        for name in &names {
            assert!(plain.contains(name));
        }
        let sections = [section("OPTIONS", document.blocks.clone(), Vec::new())];
        let mut rendered = Vec::new();
        super::render_sections_fragment(&mut rendered, &sections, 2, options);
        assert_eq!(rendered[1..].join("\n\n"), plain);
    }
}
