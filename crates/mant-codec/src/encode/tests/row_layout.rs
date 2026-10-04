//! Source-neutral owner layouts do not become another authoritative text root.
use super::*;
use mant_ir::{InlineContentRef, InlineLayout, RowLayoutHint};

fn layout(rows: &[(u32, i32)]) -> InlineLayout {
    InlineLayout {
        row_hints: rows
            .iter()
            .map(|&(row, indent_columns)| RowLayoutHint {
                row,
                indent_columns,
            })
            .collect(),
    }
}

fn visible(markdown: &str) -> String {
    Parser::new(markdown)
        .filter_map(|event| match event {
            Event::Text(value) | Event::Code(value) => Some(value.into_string()),
            Event::HardBreak | Event::SoftBreak => Some("\n".into()),
            _ => None,
        })
        .collect()
}

#[test]
fn source_rows_share_one_cursor_through_styles_links_and_literal_newlines() {
    for target in [
        mant_ir::LinkTarget::External {
            uri: "https://example.org".into(),
        },
        mant_ir::LinkTarget::Manual {
            name: "example".into(),
            manual_section: Some("1".into()),
        },
    ] {
        let nodes = vec![
            Inline::Text {
                value: "A\n".into(),
            },
            Inline::Strong {
                children: vec![Inline::Emphasis {
                    children: vec![Inline::Code {
                        value: "B\n".into(),
                    }],
                }],
            },
            Inline::Link {
                target,
                title: None,
                children: vec![Inline::Text { value: "C".into() }],
            },
            Inline::line_break(),
            Inline::Text { value: "D".into() },
        ];
        let rows = layout(&[(0, 1), (1, 2), (2, -4), (3, 3)]);
        let markdown = super::super::render_inline_content_fragment(
            InlineContentRef {
                content: &nodes,
                layout: &rows,
            },
            MarkdownFragmentOptions::default(),
        );
        assert_eq!(
            visible(&markdown),
            "\u{a0}A\n\u{a0}\u{a0}B\nC\n\u{a0}\u{a0}\u{a0}D",
            "{markdown}"
        );
        assert_eq!(mant_ir::inline_plain_text(&nodes), "A\nB\nC\nD");
        assert_eq!(
            visible(&super::super::render_inline_fragment(
                &nodes,
                MarkdownFragmentOptions::default()
            )),
            "A\nB\nC\nD"
        );
    }
}

#[test]
fn generated_row_cells_stay_outside_nested_style_and_link_ranges() {
    let nodes = [Inline::Link {
        target: mant_ir::LinkTarget::Section {
            id: "destination".into(),
        },
        title: None,
        children: vec![Inline::Strong {
            children: vec![Inline::Text {
                value: "A\nB\u{a0}C".into(),
            }],
        }],
    }];
    let rows = layout(&[(0, 1), (1, 2)]);
    let markdown = super::super::render_inline_content_fragment(
        InlineContentRef {
            content: &nodes,
            layout: &rows,
        },
        MarkdownFragmentOptions {
            preserve_anchors: true,
        },
    );
    assert_eq!(
        markdown,
        "&#160;[**A**](#destination)  \n&#160;&#160;[**B\u{a0}C**](#destination)"
    );
    let mut in_link = false;
    let mut in_strong = false;
    let mut labels = Vec::new();
    let mut layout_cells = 0;
    for event in Parser::new(&markdown) {
        match event {
            Event::Start(Tag::Link { .. }) => in_link = true,
            Event::End(TagEnd::Link) => in_link = false,
            Event::Start(Tag::Strong) => in_strong = true,
            Event::End(TagEnd::Strong) => in_strong = false,
            Event::Text(value) if in_link => {
                assert!(in_strong);
                labels.push(value.into_string());
            }
            Event::Text(value) => {
                assert!(!in_strong);
                layout_cells += value
                    .chars()
                    .filter(|character| *character == '\u{a0}')
                    .count();
            }
            _ => {}
        }
    }
    assert_eq!(labels, ["A", "B\u{a0}C"]);
    assert_eq!(layout_cells, 3);
    assert_eq!(mant_ir::inline_plain_text(&nodes), "A\nB\u{a0}C");
}

fn reference_counts(document: &Document) -> (usize, usize) {
    let mut occurrences = 0;
    let mut targets = std::collections::BTreeSet::new();
    let report = mant_ir::scan_references(
        document,
        mant_ir::ReferenceScanLimits::default(),
        |reference| {
            occurrences += 1;
            targets.insert(reference.target.to_uri().unwrap());
            std::ops::ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    (occurrences, targets.len())
}

#[test]
fn anchors_before_a_positioned_link_label_do_not_create_empty_links() {
    for children in [
        vec![Inline::anchor("before"), Inline::Text { value: "A".into() }],
        vec![Inline::Strong {
            children: vec![
                Inline::anchor("before"),
                Inline::Emphasis {
                    children: vec![Inline::Text { value: "A".into() }],
                },
            ],
        }],
    ] {
        let nodes = [Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.org".into(),
            },
            title: None,
            children,
        }];
        let rows = layout(&[(0, 2)]);
        let markdown = super::super::render_inline_content_fragment(
            InlineContentRef {
                content: &nodes,
                layout: &rows,
            },
            MarkdownFragmentOptions {
                preserve_anchors: true,
            },
        );
        let mut in_link = false;
        let mut starts = 0;
        let mut label = String::new();
        for event in Parser::new(&markdown) {
            match event {
                Event::Start(Tag::Link { .. }) => {
                    in_link = true;
                    starts += 1;
                }
                Event::End(TagEnd::Link) => in_link = false,
                Event::Text(value) if in_link => label.push_str(&value),
                Event::InlineHtml(value) if value.contains("id=\"before\"") => assert!(!in_link),
                _ => {}
            }
        }
        assert_eq!(starts, 1, "{markdown}");
        assert_eq!(label, "A", "{markdown}");
        assert!(markdown.contains("<a id=\"before\"></a>"));
        let parsed = parse_content(&markdown, None).unwrap();
        assert_eq!(reference_counts(parsed.document.as_ref().unwrap()), (1, 1));
    }
}

#[test]
fn multiline_link_export_fragments_retain_the_original_artifact_owner() {
    let mut document = manual(Vec::new());
    document.blocks = vec![Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            entry: Some(EntryFacts {
                id: "layout-owner".into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: Vec::new(),
                forms: Vec::new(),
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                value_domain: None,
            }),
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://example.org".into(),
                    },
                    title: Some("kept title".into()),
                    children: vec![Inline::Strong {
                        children: vec![Inline::Text {
                            value: "A\nB".into(),
                        }],
                    }],
                }],
                inline_layout: layout(&[(0, 1), (1, 2)]),
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: mant_ir::ListItemLayout::default(),
            source: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    assert_eq!(reference_counts(&document), (1, 1));
    let decoded: Document =
        serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap();
    assert_eq!(reference_counts(&decoded), (1, 1));
    let original = mant_ir::content_entries(&document.blocks);
    let original_owner = original[0].owner();
    let query = ResolvedContent {
        address: None,
        label: "layout".into(),
        document: Some(document.clone()),
        tldr: None,
    };
    let artifact = render_addressable_markdown(&query);
    let entries: Vec<_> = artifact
        .nodes()
        .iter()
        .filter_map(|mapped| match mapped.node() {
            MarkdownNode::DocumentEntry { owner, .. } => Some((mapped.range(), *owner)),
            _ => None,
        })
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].1.facts().unwrap().id,
        original_owner.facts().unwrap().id
    );
    let query_owners = mant_ir::content_entries(&query.document.as_ref().unwrap().blocks);
    assert!(std::ptr::eq(
        entries[0].1.facts().unwrap(),
        query_owners[0].owner().facts().unwrap()
    ));
    for label in ["A", "B"] {
        let syntax = format!("[**{label}**](https://example.org \"kept title\")");
        let start = artifact.text().find(&syntax).unwrap();
        assert!(entries[0].0.start <= start && start + syntax.len() <= entries[0].0.end);
    }
    let parsed = parse_content(artifact.text(), None).unwrap();
    let parsed_document = parsed.document.as_ref().unwrap();
    assert_eq!(reference_counts(parsed_document), (2, 1));
    mant_ir::scan_references(
        parsed_document,
        mant_ir::ReferenceScanLimits::default(),
        |reference| {
            assert!(
                matches!(reference.link, Inline::Link { title: Some(title), .. } if title == "kept title")
            );
            std::ops::ControlFlow::Continue(())
        },
    );
}

#[test]
fn hints_on_empty_or_open_positions_do_not_create_markdown_text() {
    for (nodes, row, expected) in [
        (vec![], 0, ""),
        (vec![Inline::anchor("only-anchor")], 0, ""),
        (vec![Inline::line_break()], 1, "<br />\n"),
        (
            vec![Inline::Code {
                value: "A\n".into(),
            }],
            1,
            "`A`<br>\n",
        ),
    ] {
        let rows = layout(&[(row, 8)]);
        assert_eq!(
            super::super::render_inline_content_fragment(
                InlineContentRef {
                    content: &nodes,
                    layout: &rows
                },
                MarkdownFragmentOptions::default(),
            ),
            expected
        );
    }
    let rows = layout(&[(0, 2), (1, 4), (2, 6)]);
    let literal = Block::Preformatted {
        children: vec![Inline::Text {
            value: "A\n\n".into(),
        }],
        inline_layout: rows,
        language: None,
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(
        render_blocks_fragment(&[literal], MarkdownFragmentOptions::default()),
        ["```\n  A\n\n\n```"]
    );
}

#[test]
fn joined_owner_layouts_preserve_one_code_context_and_no_body_word_gap() {
    let term = mant_ir::DefinitionTerm {
        content: vec![Inline::Code { value: "`".into() }, Inline::anchor("seam")],
        inline_layout: layout(&[(0, 1)]),
    };
    let body = Block::Paragraph {
        children: vec![Inline::Code {
            value: "BODY\nNEXT".into(),
        }],
        inline_layout: layout(&[(0, 99), (1, 2)]),
        layout: LayoutHint::default(),
        source: None,
    };
    let block = Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![term],
            description: vec![body],
            entry: None,
            layout: mant_ir::DefinitionLayout {
                head_body_relation: mant_ir::HeadBodyRelation::joined(
                    mant_ir::DefinitionBodyAlignment::Indented,
                ),
                ..Default::default()
            },
            source: None,
        }],
        compact: true,
        declaration_groups: vec![],
        layout: LayoutHint::default(),
        source: None,
    };
    let markdown =
        render_blocks_fragment(&[block], MarkdownFragmentOptions::default()).join("\n\n");
    assert_eq!(
        visible(&markdown),
        "\u{a0}`BODY\n\u{a0}\u{a0}NEXT",
        "{markdown}"
    );
    assert!(!markdown.contains(&"&#160;".repeat(99)));
}

#[test]
fn heading_and_table_owner_hints_reach_their_existing_markdown_projection() {
    let rows = layout(&[(0, 1), (1, 2)]);
    let heading = mant_ir::Heading {
        content: vec![Inline::Text {
            value: "A\nB".into(),
        }],
        inline_layout: rows.clone(),
        source: None,
    };
    assert_eq!(
        super::super::render_heading_fragment(1, &heading, MarkdownFragmentOptions::default()),
        "&#160;A  \n&#160;&#160;B\n==="
    );
    let table = Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![TableCell {
                kind: mant_ir::TableCellKind::Text,
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "A\nB".into(),
                    }],
                    inline_layout: rows,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        column_widths: vec![],
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(
        render_blocks_fragment(&[table], MarkdownFragmentOptions::default()),
        ["```\n A\n  B\n```"]
    );
}
