//! Final exporter bytes, compared twice with source IR rather than a candidate.
use super::*;

fn external(label: &str, uri: &str, title: Option<&str>) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::External { uri: uri.into() },
        title: title.map(Into::into),
        children: vec![text(label)],
    }
}

fn enclosed(nodes: Vec<Inline>, context: &str) -> ResolvedContent {
    let mut document = manual(vec![]);
    let definition = |head: bool| Block::DefinitionList {
        declaration_groups: vec![],
        items: vec![DefinitionItem {
            terms: if head {
                vec![nodes.clone().into()]
            } else {
                vec![]
            },
            description: if head {
                vec![]
            } else {
                vec![paragraph(nodes.clone())]
            },
            head_body_relation: mant_ir::HeadBodyRelation::Separate,
            layout: mant_ir::DefinitionLayout::default(),
            source: None,
            entry: None,
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    };
    if context == "heading" {
        let mut heading = section("unused", vec![], vec![]);
        heading.heading.content = nodes;
        document.sections.push(heading);
    } else {
        document.blocks.push(match context {
            "paragraph" => paragraph(nodes),
            "list" => Block::List {
                kind: ListKind::Bullet,
                compact: true,
                items: vec![ListItem {
                    blocks: vec![paragraph(nodes)],
                    layout: mant_ir::ListItemLayout::default(),
                    source: None,
                    entry: None,
                }],
                layout: LayoutHint::default(),
                source: None,
            },
            "definition-head" => definition(true),
            "definition-body" => definition(false),
            _ => panic!("unknown final embedding {context}"),
        });
    }
    ResolvedContent {
        address: None,
        label: "final Markdown bytes".into(),
        document: Some(document),
        tldr: None,
    }
}

fn first_nodes(block: &Block) -> &[Inline] {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => children,
        Block::List { items, .. } => first_nodes(&items[0].blocks[0]),
        other => panic!("unexpected parsed container {other:?}"),
    }
}

fn restored_nodes(query: &ResolvedContent, context: &str) -> Vec<Inline> {
    let document = query.document.as_ref().unwrap();
    if context == "heading" {
        document.sections[0].heading.content.clone()
    } else {
        first_nodes(&document.blocks[0]).to_vec()
    }
}

fn final_rounds(nodes: &[Inline], context: &str, options: MarkdownOptions) -> Meaning {
    let expected = meaning(nodes);
    let mut query = enclosed(nodes.to_vec(), context);
    let mut result = Meaning::default();
    for _ in 0..2 {
        let markdown = render_markdown_with_options(&query, options);
        query = parse_content(&markdown, None).unwrap();
        assert!(
            query
                .document
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .all(|d| { d.code.as_deref() != Some("markdown.unsupported") }),
            "{context}: {markdown:?}"
        );
        result = meaning(&restored_nodes(&query, context));
        // Both rounds use IR0, including occurrence number and scalar coverage
        // for equal links. Only existing optional Strong/Emphasis loss is allowed.
        assert_allowed_styles(&expected, &result, &format!("{context}: {markdown}"));
    }
    result
}

const CONTEXTS: [&str; 5] = [
    "paragraph",
    "heading",
    "list",
    "definition-head",
    "definition-body",
];
const OPTIONS: [MarkdownOptions; 2] = [
    MarkdownOptions {
        preserve_anchors: false,
        preserve_semantics: false,
    },
    MarkdownOptions {
        preserve_anchors: false,
        preserve_semantics: true,
    },
];

#[test]
fn author_edge_entities_validate_the_final_nested_delimiters_in_every_embedding() {
    for source in [
        "_! ***a***_&#32;",
        "_a ***b***_&#32;",
        "&#32;_***a*** !_",
        "&#9;_a ***b***_&#32;&#9;",
        "_! ***`a`***_&#32;",
        "_! ***[a](https://a.test \"owner\")***_&#32;",
    ] {
        let nodes = parsed_children(source);
        for context in CONTEXTS {
            for options in OPTIONS {
                final_rounds(&nodes, context, options);
            }
        }
    }
    // A safe isolated style remains available at these same final row edges.
    for context in CONTEXTS {
        for options in OPTIONS {
            let nodes = vec![text(" \t"), styled(true, vec![text("SAFE")]), text("\t ")];
            assert_eq!(final_rounds(&nodes, context, options), meaning(&nodes));
        }
    }
}

#[test]
fn bang_link_seams_survive_plain_styled_transparent_and_second_fallback_rounds() {
    let parsed = parsed_children(r"\![a](https://a.test)");
    let mut cases = vec![
        parsed,
        vec![
            text("!!"),
            external("a", "https://a.test/!", Some("owner!")),
        ],
        vec![
            text("!"),
            text("!"),
            Inline::anchor("empty"),
            text(""),
            external("a", "https://a.test/!", None),
        ],
        vec![
            styled(true, vec![text("!")]),
            external("a", "https://a.test", None),
        ],
        // The inner accepted child is unsafe under the outer wrapper, forcing
        // the content-preserving fallback before the next unstyled readback.
        vec![
            styled(
                true,
                vec![
                    styled(false, vec![Inline::Code { value: "a".into() }]),
                    text("b*"),
                    text("!"),
                ],
            ),
            external("a", "https://a.test", Some("same")),
        ],
        vec![
            text("!"),
            external("https://a.test/!", "https://a.test/!", None),
        ],
    ];
    // A transparent manual label exposes its bang to the following live link.
    let transparent = Inline::Link {
        target: mant_ir::LinkTarget::Manual {
            name: "tool".into(),
            manual_section: Some("1".into()),
        },
        title: None,
        children: vec![text("!")],
    };
    cases.push(vec![transparent, external("a", "https://a.test/!", None)]);
    for nodes in cases {
        // Plain body export deliberately unwraps manual links. Expected active
        // owners follow that existing policy, not the source wrapper shape.
        let expected_nodes = if matches!(
            nodes.first(),
            Some(Inline::Link {
                target: mant_ir::LinkTarget::Manual { .. },
                ..
            })
        ) {
            vec![text("!"), external("a", "https://a.test/!", None)]
        } else {
            nodes.clone()
        };
        for context in CONTEXTS {
            // Headings retain manual wrappers, so that policy is exercised in
            // the other cases and the transparent body case uses body owners.
            if context == "heading" && nodes != expected_nodes {
                continue;
            }
            for options in OPTIONS {
                assert_link_seam_rounds(
                    enclosed(nodes.clone(), context),
                    &meaning(&expected_nodes),
                    context,
                    options,
                );
            }
        }
    }
    // Joined source owners share the same emitted seam even though the HEAD
    // and BODY remain separate IR roots.
    let link = external("a", "https://a.test/!", Some("joined owner"));
    let expected = meaning(&[text("!"), link.clone()]);
    for options in OPTIONS {
        let mut query = enclosed(vec![text("!")], "definition-head");
        let Block::DefinitionList { items, .. } = &mut query.document.as_mut().unwrap().blocks[0]
        else {
            unreachable!()
        };
        items[0].head_body_relation = mant_ir::HeadBodyRelation::joined();
        items[0].description = vec![paragraph(vec![link.clone()])];
        assert_link_seam_rounds(query, &expected, "definition-head", options);
    }
}

fn assert_link_seam_rounds(
    mut query: ResolvedContent,
    expected: &Meaning,
    context: &str,
    options: MarkdownOptions,
) {
    for _ in 0..2 {
        let markdown = render_markdown_with_options(&query, options);
        assert!(
            !Parser::new(&markdown).any(|event| matches!(event, Event::Start(Tag::Image { .. }))),
            "{markdown}"
        );
        assert!(markdown.contains("https://a.test"), "{markdown}");
        assert!(!markdown.contains("https://a.test/\\!"), "{markdown}");
        query = parse_content(&markdown, None).unwrap();
        assert_allowed_styles(
            expected,
            &meaning(&restored_nodes(&query, context)),
            &markdown,
        );
    }
}

#[test]
fn row_whitespace_styles_code_and_link_occurrences_keep_independent_coverage() {
    // Bounded pairwise axes: SP/TAB/multiple/all-white, raw/entity-derived,
    // Unicode edge classes, repeated hard rows, same/alternating/multiple styles.
    for (leading, trailing) in [
        (" ", ""),
        ("", "\t"),
        (" \t", "\t  "),
        ("\u{a0}", "\u{2003}"),
        ("\u{202f}", "\u{a0}"),
    ] {
        for carrier in 0..4 {
            let mut core = vec![
                text("! "),
                styled(true, vec![text("a")]),
                Inline::Code {
                    value: " x*_y ".into(),
                },
                external("same", "https://a.test/!", Some("original")),
                external("same", "https://a.test/!", Some("original")),
            ];
            for depth in 0..carrier {
                core = vec![styled(depth % 2 == 0, core)];
            }
            let mut nodes = vec![text(leading), Inline::line_break()];
            nodes.extend(core);
            nodes.extend([
                Inline::line_break(),
                Inline::line_break(),
                text("# author"),
                Inline::line_break(),
                text(trailing),
            ]);
            for context in CONTEXTS {
                for options in OPTIONS {
                    final_rounds(&nodes, context, options);
                }
            }
        }
    }
    for nodes in [
        vec![text(" \t  ")],
        vec![Inline::line_break(), text(" \t"), Inline::line_break()],
    ] {
        for context in CONTEXTS {
            for options in OPTIONS {
                final_rounds(&nodes, context, options);
            }
        }
    }
}

#[test]
fn table_cells_keep_the_explicit_portable_fence_projection_for_two_rounds() {
    for source in ["_! ***a***_&#32;", r"\![a](https://a.test/!)"] {
        let nodes = parsed_children(source);
        let expected = mant_ir::inline_plain_text(&nodes);
        for nested_definition in [false, true] {
            for options in OPTIONS {
                let cell_blocks = if nested_definition {
                    enclosed(nodes.clone(), "definition-head")
                        .document
                        .unwrap()
                        .blocks
                } else {
                    vec![paragraph(nodes.clone())]
                };
                let table = Block::Table {
                    borders: mant_ir::TableBorders::default(),
                    alignment: None,
                    column_count: None,
                    rows: vec![TableRow {
                        column_borders: vec![],
                        kind: mant_ir::TableRowKind::Data,
                        cells: vec![TableCell {
                            preferred_width_columns: None,
                            break_after: false,
                            kind: mant_ir::TableCellKind::Text,
                            blocks: cell_blocks,
                            column_span: 1,
                            row_span: 1,
                            alignment: None,
                        }],
                    }],
                    column_preferences: mant_ir::ColumnPreferences::default(),
                    layout: LayoutHint::default(),
                    source: None,
                };
                let mut query = enclosed(vec![], "paragraph");
                query.document.as_mut().unwrap().blocks = vec![table];
                for _ in 0..2 {
                    let markdown = render_markdown_with_options(&query, options);
                    query = parse_content(&markdown, None).unwrap();
                    let nodes = restored_nodes(&query, "paragraph");
                    assert_eq!(mant_ir::inline_plain_text(&nodes), expected, "{markdown}");
                    // Tables intentionally become literal portable rows. Link
                    // activation and fonts are not part of this projection.
                    assert_eq!(meaning(&nodes).links, []);
                    assert!(
                        meaning(&nodes)
                            .scalars
                            .iter()
                            .all(|scalar| scalar.styles == 0)
                    );
                }
            }
        }
    }
}

#[test]
fn semantic_configuration_keeps_real_entry_metadata_distinct_and_content_exact() {
    for payload in [
        r#"\![a](https://a.test/! "owner")"#,
        "_a ***b***_&#32;",
        "***SAFE***",
    ] {
        let source = format!(
            "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--flag`: {payload}\n"
        );
        let original = parse_content(&source, None).unwrap();
        let expected = meaning(&restored_nodes(&original, "list"));
        let plain = render_markdown_with_options(&original, OPTIONS[0]);
        let semantic = render_markdown_with_options(&original, OPTIONS[1]);
        assert_ne!(plain, semantic);
        assert!(semantic.contains("<!-- mant:entries"));
        for options in OPTIONS {
            let mut query = original.clone();
            for _ in 0..2 {
                let markdown = render_markdown_with_options(&query, options);
                query = parse_content(&markdown, None).unwrap();
                assert_allowed_styles(
                    &expected,
                    &meaning(&restored_nodes(&query, "list")),
                    &markdown,
                );
                let Block::List { compact, items, .. } =
                    &query.document.as_ref().unwrap().blocks[0]
                else {
                    panic!("one declared list: {markdown}");
                };
                assert!(*compact, "metadata must not add a list gap: {markdown}");
                if options.preserve_semantics {
                    let entry = items[0].entry.as_ref().unwrap();
                    assert_eq!(entry.id.as_str(), "option-flag");
                    assert_eq!(entry.names, ["--flag"]);
                }
            }
        }
    }
}

#[test]
fn addressable_anchors_keep_commonmark_targets_and_the_explicit_native_html_policy() {
    let nodes = vec![
        text("!"),
        Inline::anchor_with_aliases("seam", vec!["Alias.Target".into()]),
        external("a", "https://a.test/!", Some("owner!")),
    ];
    let markdown = render_inline_fragment(
        &nodes,
        MarkdownFragmentOptions {
            preserve_anchors: true,
        },
    );
    assert_eq!(
        markdown,
        "!<a id=\"seam\"></a><a id=\"Alias.Target\"></a>[a](https://a.test/! \"owner!\")"
    );
    let mut actual = Meaning::default();
    let mut owner = None;
    let mut anchors = vec![];
    for event in Parser::new(&markdown) {
        match event {
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                owner = Some(actual.links.len());
                actual.links.push((
                    mant_ir::LinkTarget::from_uri(&dest_url),
                    Some(title.into_string()),
                ));
            }
            Event::End(TagEnd::Link) => owner = None,
            Event::Text(value) => actual.text(&value, 0, owner),
            Event::InlineHtml(value) => anchors.push(value.into_string()),
            Event::Start(Tag::Image { .. }) => panic!("addressable link became an image"),
            _ => {}
        }
    }
    assert_eq!(actual, meaning(&nodes));
    assert_eq!(
        anchors,
        ["<a id=\"seam\">", "</a>", "<a id=\"Alias.Target\">", "</a>"]
    );

    // The native reader deliberately preserves unsupported HTML as exact source
    // prose. This is a declared projection for ADDRESSABLE, not a lossless IR
    // roundtrip claim. Both native reads retain that source and active link.
    let native_expected = meaning(&[
        text("!<a id=\"seam\"></a><a id=\"Alias.Target\"></a>"),
        external("a", "https://a.test/!", Some("owner!")),
    ]);
    let mut query = parse_content(&markdown, None).unwrap();
    assert!(
        query
            .document
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("markdown.unsupported"))
    );
    for _ in 0..2 {
        assert_eq!(
            meaning(&restored_nodes(&query, "paragraph")),
            native_expected
        );
        let markdown = render_markdown_with_options(&query, OPTIONS[0]);
        query = parse_content(&markdown, None).unwrap();
    }
}

#[test]
fn actual_link_brackets_and_metadata_keep_meaning_in_all_export_roots() {
    for source in [
        "[_! ***a***_&#32;](https://a.test)",
        r#"[a](https://a.test "say \\")"#,
        r#"[a](https://a.test/&amp;amp; "&amp;amp;")"#,
        "[a](https://a.test \"one\ntwo\")",
        "[a](https://a.test \"one\r\ntwo\")",
        "[a  \nb](https://a.test \"line\nmetadata\")",
    ] {
        let nodes = parsed_children(source);
        for context in CONTEXTS {
            for options in OPTIONS {
                final_rounds(&nodes, context, options);
            }
        }
    }
}
