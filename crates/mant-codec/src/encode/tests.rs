//! Contract-oriented tests for `CommonMark` structure and escaping.

use mant_ir::{
    Block, DefinitionItem, Document, DocumentMeta, DocumentSource, EntryFacts, EntryKind, Inline,
    LayoutHint, ListItem, ListKind, NameCase, Section, SourceFormat, TableCell, TableRow,
    visit::{Visit, walk_inline},
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown, render_markdown,
    render_markdown_with_options,
};
use mant_ir::ResolvedContent;

/// Supply already-parsed content to the document encoder without discovery,
/// query validation, projection, or report DTOs. Labels here are caller-owned.
fn parse_content(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, crate::markdown::MarkdownParseError> {
    let label = source_path.clone().unwrap_or_else(|| "stdin".to_owned());
    let parsed = crate::markdown::parse_markdown(source, source_path)?;
    Ok(ResolvedContent {
        address: None,
        label,
        document: Some(parsed.document),
        tldr: parsed.tldr,
    })
}

#[test]
fn large_entry_source_maps_keep_monotonic_exact_ownership() {
    use std::fmt::Write;
    let mut source = "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n".to_owned();
    for index in 0..1000 {
        writeln!(source, "- `--flag-{index}`: Payload{index}.").unwrap();
    }
    let query = parse_content(&source, None).unwrap();
    let document = query.document.unwrap();
    let rendered = super::blocks::render_blocks_with_entries(
        &document.blocks,
        MarkdownOptions {
            preserve_anchors: true,
            ..MarkdownOptions::default()
        },
        true,
    );
    assert_eq!(rendered.entries.len(), 1000);
    for (index, entry) in rendered.entries.iter().enumerate() {
        assert!(rendered.text[entry.start..entry.end].contains(&format!("Payload{index}.")));
        if let Some(next) = rendered.entries.get(index + 1) {
            assert!(entry.end <= next.start);
        }
    }
}

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn document_export_skips_maps_but_keeps_identical_addressable_bytes() {
    let source = "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--root`: Root payload.\n\n## Parent\n\n<!-- mant:entries role=option case=sensitive -->\n- `--first`: First payload.\n\n### Child\n\n<!-- mant:entries role=option case=sensitive -->\n- `--second`: Second payload.\n";
    let query = parse_content(source, None).unwrap();
    let mapped = render_addressable_markdown(&query);
    let plain = super::render_markdown_artifact(&query, MarkdownOptions::ADDRESSABLE, false);
    assert_eq!(mapped.text(), plain.text());
    assert!(plain.nodes().is_empty());
    assert!(plain.sections.is_empty());
    assert!(plain.anchors.get().is_none());
    assert_eq!(mapped.sections.len(), 2);
    assert_eq!(mapped.sections[0].parent, None);
    assert_eq!(mapped.sections[1].parent, Some(0));
    let document = query.document.as_ref().unwrap();
    assert!(std::ptr::eq(
        mapped.sections[0].section,
        &raw const document.sections[0]
    ));
    assert!(std::ptr::eq(
        mapped.sections[1].section,
        &raw const document.sections[0].children[0]
    ));
    let mut entries = 0;
    for mapped in mapped.nodes() {
        if let MarkdownNode::DocumentEntry { owner, names, .. } = &mapped.node {
            entries += 1;
            assert!(std::ptr::eq(
                names.as_ptr(),
                owner.facts().unwrap().names.as_ptr()
            ));
        }
    }
    assert_eq!(entries, 3);
}

#[test]
fn maps_borrow_owners_from_their_exact_source_snapshot() {
    fn owner<'a>(node: &MarkdownNode<'a>) -> Option<mant_ir::EntryOwner<'a>> {
        match node {
            MarkdownNode::DocumentEntry { owner, .. } => Some(*owner),
            _ => None,
        }
    }
    let source =
        "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--flag`: Payload.\n";
    let first = parse_content(source, None).unwrap();
    let second = first.clone();
    let a = render_addressable_markdown(&first);
    let b = render_addressable_markdown(&second);
    let a_owner = a
        .nodes()
        .iter()
        .find_map(|mapped| owner(&mapped.node))
        .unwrap();
    let b_owner = b
        .nodes()
        .iter()
        .find_map(|mapped| owner(&mapped.node))
        .unwrap();
    assert_eq!(a_owner.facts().unwrap().id, b_owner.facts().unwrap().id);
    assert!(!std::ptr::eq(
        a_owner.facts().unwrap(),
        b_owner.facts().unwrap()
    ));
    let original = mant_ir::content_entries(&first.document.as_ref().unwrap().blocks);
    assert!(std::ptr::eq(
        a_owner.facts().unwrap(),
        original[0].owner().facts().unwrap()
    ));
}

#[test]
fn logical_link_serialization_preserves_literal_percent_and_unicode_components() {
    #[derive(Default)]
    struct Links(Vec<mant_ir::LinkTarget>);
    impl<'a> Visit<'a> for Links {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Link { target, .. } = inline {
                self.0.push(target.clone());
            }
            walk_inline(self, inline);
        }
    }
    let source = "## Mixed {#Mixed%2ETarget}\n[local](#Mixed%252ETarget) [doc](space%20name.md#Mixed%2ETarget) [literal](literal%2520.md#literal%252E) [unicode](%E6%97%A5%E6%9C%AC.md)\n";
    let query = parse_content(source, None).unwrap();
    let options = MarkdownOptions {
        preserve_anchors: true,
        ..MarkdownOptions::default()
    };
    let markdown = render_markdown_with_options(&query, options);
    assert!(
        markdown.contains("literal%2520.md#literal%252E"),
        "{markdown}"
    );
    let reparsed = parse_content(&markdown, None).unwrap();
    assert!(
        !reparsed
            .document
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("ir.dangling-section-link"))
    );
    let mut original_links = Links::default();
    original_links.visit_document(query.document.as_ref().unwrap());
    let mut reparsed_links = Links::default();
    reparsed_links.visit_document(reparsed.document.as_ref().unwrap());
    assert_eq!(reparsed_links.0, original_links.0);
}

#[test]
fn underscore_escaping_is_independent_of_text_segmentation_but_respects_styles() {
    let text = |value: &str| Inline::Text {
        value: value.to_owned(),
    };
    for parts in [
        vec![text("NAME_PID")],
        vec![text("NAME"), text("_"), text("PID")],
    ] {
        let rendered = super::inline::render_inline(&parts, MarkdownOptions::default());
        assert_eq!(rendered, "NAME_PID");
    }
    let styled = vec![
        Inline::Emphasis {
            children: vec![text("NAME")],
        },
        text("_PID suffix_"),
    ];
    let rendered = super::inline::render_inline(&styled, MarkdownOptions::default());
    let events = Parser::new(&rendered).collect::<Vec<_>>();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::Emphasis)))
            .count(),
        1
    );
    let visible = events
        .into_iter()
        .filter_map(|event| {
            if let Event::Text(value) = event {
                Some(value.into_string())
            } else {
                None
            }
        })
        .collect::<String>();
    assert_eq!(visible, "NAME_PID suffix_");
    // Looking through the emitted '*' to the visible 'E' is unsafe: the
    // first underscore can now open a new emphasis delimiter run.
    assert_eq!(
        Parser::new("*NAME*_PID suffix_")
            .filter(|event| matches!(event, Event::Start(Tag::Emphasis)))
            .count(),
        2
    );
}

fn manual(sections: Vec<Section>) -> Document {
    Document {
        heading: None,
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Man,
            path: None,
        },
        meta: DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: Vec::new(),
        sections,
    }
}

fn section(title: &str, blocks: Vec<Block>, children: Vec<Section>) -> Section {
    Section {
        id: title.to_lowercase().into(),
        fragment_aliases: Vec::new(),
        heading: title.into(),
        spacing_before_lines: 0,
        blocks,
        children,
        source: None,
    }
}

#[test]
fn addressable_markdown_emits_canonical_and_authored_fragments() {
    let mut section = section(
        "Mixed target",
        vec![paragraph(vec![Inline::anchor_with_aliases(
            "option",
            vec!["--option".into()],
        )])],
        Vec::new(),
    );
    section.id = "mixed-target".into();
    section.fragment_aliases = vec!["Mixed.Target".into()];
    let query = ResolvedContent {
        address: None,
        label: "fragments".to_owned(),
        document: Some(manual(vec![section])),
        tldr: None,
    };

    let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert!(markdown.contains("<a id=\"mixed-target\"></a>"));
    assert!(markdown.contains("<a id=\"Mixed.Target\"></a>"));
    assert!(markdown.contains("<a id=\"option\"></a>"));
    assert!(markdown.contains("<a id=\"--option\"></a>"));
}

#[test]
fn addressable_markdown_emits_document_root_fragments() {
    let mut document = manual(Vec::new());
    document.blocks = vec![paragraph(vec![Inline::Text {
        value: "Preface.".to_owned(),
    }])];
    document.fragment_aliases = vec!["Mixed.Root".into()];
    let query = ResolvedContent {
        address: None,
        label: "fragments".to_owned(),
        document: Some(document),
        tldr: None,
    };

    let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert!(markdown.contains("<a id=\"document-overview\"></a>"));
    assert!(markdown.contains("<a id=\"Mixed.Root\"></a>"));
}

fn email_addresses(document: &Document) -> Vec<String> {
    #[derive(Default)]
    struct EmailCollector(Vec<String>);

    impl<'ir> Visit<'ir> for EmailCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link {
                target: mant_ir::LinkTarget::Email { address },
                ..
            } = inline
            {
                self.0.push(address.clone());
            }
            walk_inline(self, inline);
        }
    }

    let mut collector = EmailCollector::default();
    collector.visit_document(document);
    collector.0
}

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
        Inline::LineBreak,
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
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
            terms: vec![
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
            ],
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
fn keeps_adjacent_bold_and_italic_runs_unambiguous_in_commonmark() {
    let definitions = Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
            terms: vec![vec![
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "-r ".to_owned(),
                    }],
                },
                Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: "prompt".to_owned(),
                    }],
                },
                Inline::Text {
                    value: ", ".to_owned(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "--prompt=".to_owned(),
                    }],
                },
                Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: "prompt".to_owned(),
                    }],
                },
            ]],
            description: vec![paragraph(vec![Inline::Text {
                value: "Set the pager prompt.".to_owned(),
            }])],
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    };
    let query = ResolvedContent {
        address: None,
        label: "man".to_owned(),
        document: Some(manual(vec![section(
            "OPTIONS",
            vec![definitions],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("**-r** *prompt*, **--prompt=**_prompt_"));
    assert!(!markdown.contains("***"));
    assert!(!markdown.contains("<em>"));

    let styled_events = Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Start(Tag::Strong) => Some("strong-start"),
            Event::End(TagEnd::Strong) => Some("strong-end"),
            Event::Start(Tag::Emphasis) => Some("emphasis-start"),
            Event::End(TagEnd::Emphasis) => Some("emphasis-end"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        styled_events,
        [
            "strong-start",
            "strong-end",
            "emphasis-start",
            "emphasis-end",
            "strong-start",
            "strong-end",
            "emphasis-start",
            "emphasis-end",
        ]
    );
}

#[test]
fn coalesces_adjacent_roff_styles_and_uses_minimal_intraword_escaping() {
    let query = ResolvedContent {
        address: None,
        label: "zsh-style".to_owned(),
        document: Some(manual(vec![section(
            "INVOCATION",
            vec![paragraph(vec![
                Inline::Text {
                    value: "The long option `".to_owned(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "-".to_owned(),
                    }],
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "-emulate".to_owned(),
                    }],
                },
                Inline::Text {
                    value: "' and ".to_owned(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "PATH_SCRIPT".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " are literal tokens.".to_owned(),
                },
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("`**--emulate**' and **PATH_SCRIPT**"));
    assert!(!markdown.contains("**-**__-emulate__"));
    assert!(!markdown.contains("PATH\\_SCRIPT"));

    let visible = Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Text(value) | Event::Code(value) => Some(value.to_string()),
            _ => None,
        })
        .collect::<String>();
    assert!(visible.contains("The long option `--emulate' and PATH_SCRIPT are literal tokens."));
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
                    children: vec![
                        Inline::Text {
                            value: "before ``` marker".to_owned(),
                        },
                        Inline::LineBreak,
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
                    rows: vec![TableRow {
                        cells: vec![
                            TableCell {
                                blocks: vec![paragraph(vec![Inline::Text {
                                    value: "left".to_owned(),
                                }])],
                                column_span: 1,
                                row_span: 1,
                                alignment: None,
                            },
                            TableCell {
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
fn serializes_typed_email_links_through_the_shared_mailto_boundary() {
    let query = ResolvedContent {
        address: None,
        label: "mail".to_owned(),
        document: Some(manual(vec![section(
            "CONTACT",
            vec![paragraph(vec![
                Inline::Link {
                    target: mant_ir::LinkTarget::Email {
                        address: "user%tag@example.test".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "percent".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::Link {
                    target: mant_ir::LinkTarget::Email {
                        address: "a/b@example.test".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "slash".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::Link {
                    target: mant_ir::LinkTarget::Email {
                        address: "user=tag@example.test".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "equals".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::Link {
                    target: mant_ir::LinkTarget::Email {
                        address: ".invalid@example.test".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "invalid remains visible".to_owned(),
                    }],
                },
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("[percent](mailto:user%25tag@example.test)"));
    assert!(markdown.contains("[slash](mailto:a%2Fb@example.test)"));
    assert!(markdown.contains("[equals](mailto:user%3Dtag@example.test)"));
    assert!(markdown.contains("invalid remains visible"));
    assert!(!markdown.contains("mailto:.invalid@example.test"));
}

#[test]
fn typed_email_links_round_trip_through_markdown_without_uri_diagnostics() {
    let source = "[percent](mailto:user%25tag@example.test) \
                  [slash](mailto:a%2Fb@example.test) \
                  [equals](mailto:user%3Dtag@example.test)\n";
    let parsed =
        parse_content(source, Some("mail.md".to_owned())).expect("parse typed email links");
    let expected = vec![
        "user%tag@example.test".to_owned(),
        "a/b@example.test".to_owned(),
        "user=tag@example.test".to_owned(),
    ];
    assert_eq!(
        email_addresses(parsed.document.as_ref().expect("parsed document")),
        expected
    );

    let markdown = render_markdown(&parsed);
    let reparsed = parse_content(&markdown, Some("round-trip.md".to_owned()))
        .expect("reparse rendered Markdown");
    let document = reparsed.document.as_ref().expect("reparsed document");
    assert_eq!(email_addresses(document), expected);
    assert!(
        document.diagnostics.iter().all(|diagnostic| !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-external-uri" | "ir.invalid-email-address")
        )),
        "round trip introduced a URI diagnostic: {:?}",
        document.diagnostics
    );
}

#[test]
fn protects_paragraph_lines_from_accidental_block_syntax() {
    let query = ResolvedContent {
        address: None,
        label: "syntax".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![paragraph(vec![
                Inline::Text {
                    value: "- not a list".to_owned(),
                },
                Inline::LineBreak,
                Inline::Text {
                    value: "1. not an ordered list".to_owned(),
                },
                Inline::LineBreak,
                Inline::Text {
                    value: "# not a heading".to_owned(),
                },
                Inline::LineBreak,
                Inline::Text {
                    value: "-".to_owned(),
                },
                Inline::LineBreak,
                Inline::Text {
                    value: "===".to_owned(),
                },
                Inline::LineBreak,
                Inline::Text {
                    value: "```".to_owned(),
                },
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains(
            "\\- not a list  \n1\\. not an ordered list  \n\\# not a heading  \n\\-  \n\\===  \n\\`\\`\\`"
        ),
        "{markdown}"
    );
    let headings = Parser::new(&markdown)
        .filter(|event| matches!(event, Event::Start(Tag::Heading { .. })))
        .count();
    assert_eq!(headings, 2, "only the document and TEXT headings may exist");
    assert!(!Parser::new(&markdown).any(|event| matches!(event, Event::Start(Tag::CodeBlock(_)))));
}

#[test]
fn preserves_leading_consecutive_and_trailing_hard_breaks() {
    let query = ResolvedContent {
        address: None,
        label: "breaks".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![paragraph(vec![
                Inline::LineBreak,
                Inline::Text {
                    value: "before".to_owned(),
                },
                Inline::LineBreak,
                Inline::LineBreak,
                Inline::Text {
                    value: "after".to_owned(),
                },
                Inline::LineBreak,
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains("<br>\nbefore<br>\n<br>\nafter<br>"),
        "{markdown}"
    );
}

#[test]
fn preserves_literal_html_entity_spellings_across_commonmark() {
    let query = ResolvedContent {
        address: None,
        label: "entities".to_owned(),
        document: Some(manual(vec![section(
            "ENTITY TEXT",
            vec![paragraph(vec![Inline::Text {
                value: "literal a & b; spellings &amp;, &pound;, &#163;, and &notreal;".to_owned(),
            }])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("a &amp; b"), "{markdown}");
    assert!(markdown.contains("&amp;amp;"), "{markdown}");
    assert!(markdown.contains("&amp;pound;"), "{markdown}");
    assert!(markdown.contains("&amp;#163;"), "{markdown}");
    assert!(markdown.contains("&amp;notreal;"), "{markdown}");
    let visible = Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Text(value) => Some(value.into_string()),
            _ => None,
        })
        .collect::<String>();
    assert!(
        visible.contains("literal a & b; spellings &amp;, &pound;, &#163;, and &notreal;"),
        "{visible}"
    );
}

#[test]
fn escapes_literal_dollars_that_would_be_reparsed_as_math() {
    let query = ResolvedContent {
        address: None,
        label: "variables".to_owned(),
        document: Some(manual(vec![section(
            "$info = summary ([$conf])",
            Vec::new(),
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains("## \\$info = summary (\\[\\$conf\\])"),
        "{markdown}"
    );
    let headings = Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Text(value) => Some(value.into_string()),
            _ => None,
        })
        .collect::<String>();
    assert!(headings.contains("$info = summary ([$conf])"), "{headings}");
}

#[test]
fn escapes_enabled_extension_syntax_and_unsupported_block_prefixes() {
    let query = ResolvedContent {
        address: None,
        label: "extensions".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![Block::Unsupported {
                name: Some("source".to_owned()),
                text: "# injected\n~~~\na | b\n: definition\n~~strike~~\n^super^".to_owned(),
                layout: LayoutHint::default(),
                source: None,
            }],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("\\# injected"), "{markdown}");
    assert!(markdown.contains("\\~\\~\\~"), "{markdown}");
    assert!(markdown.contains("a \\| b"), "{markdown}");
    assert!(markdown.contains("\\: definition"), "{markdown}");
    assert!(markdown.contains("\\~\\~strike\\~\\~"), "{markdown}");
    assert!(markdown.contains("\\^super\\^"), "{markdown}");
    assert_eq!(
        Parser::new(&markdown)
            .filter(|event| matches!(event, Event::Start(Tag::Heading { .. })))
            .count(),
        2,
        "only the document and section headings remain structural"
    );
}

#[test]
fn nested_styles_preserve_contiguous_intraword_spellings() {
    let query = ResolvedContent {
        address: None,
        label: "styles".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![paragraph(vec![Inline::Emphasis {
                children: vec![
                    Inline::Text {
                        value: "x".to_owned(),
                    },
                    Inline::Strong {
                        children: vec![Inline::Text {
                            value: "-".to_owned(),
                        }],
                    },
                    Inline::Text {
                        value: "y".to_owned(),
                    },
                ],
            }])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("*x-y*"), "{markdown}");
    assert!(!markdown.contains("**-**"), "{markdown}");
    let visible = Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Text(value) => Some(value.into_string()),
            _ => None,
        })
        .collect::<String>();
    assert!(visible.contains("x-y"), "{visible}");
}

#[test]
fn styles_only_flatten_when_commonmark_cannot_delimit_them() {
    let query = ResolvedContent {
        address: None,
        label: "styles".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![paragraph(vec![
                Inline::Text {
                    value: "disabled with --".to_owned(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "no-".to_owned(),
                    }],
                },
                Inline::Text {
                    value: "option; safe ".to_owned(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "!".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " and ".to_owned(),
                },
                Inline::Emphasis {
                    children: vec![
                        Inline::Text {
                            value: "an ".to_owned(),
                        },
                        Inline::Strong {
                            children: vec![Inline::Text {
                                value: "important".to_owned(),
                            }],
                        },
                        Inline::Text {
                            value: " word".to_owned(),
                        },
                    ],
                },
                Inline::Text {
                    value: ". chained --".to_owned(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "no-".to_owned(),
                    }],
                },
                Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: "option-".to_owned(),
                    }],
                },
                Inline::Text {
                    value: "word.".to_owned(),
                },
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("disabled with --no-option"), "{markdown}");
    assert!(!markdown.contains("--**no-**option"), "{markdown}");
    assert!(markdown.contains("safe **!**"), "{markdown}");
    assert!(markdown.contains("_an **important** word_"), "{markdown}");
    assert!(markdown.contains("chained --no-option-word"), "{markdown}");
    assert!(!markdown.contains("**no-**option-word"), "{markdown}");

    let events = Parser::new(&markdown).collect::<Vec<_>>();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::Strong)))
            .count(),
        2,
        "safe top-level and nested strong spans remain semantic"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::Emphasis)))
            .count(),
        1,
        "the representable outer emphasis remains semantic"
    );
    let visible = events
        .into_iter()
        .filter_map(|event| match event {
            Event::Text(value) => Some(value.into_string()),
            _ => None,
        })
        .collect::<String>();
    assert!(
        visible.contains(
            "disabled with --no-option; safe ! and an important word. chained --no-option-word."
        ),
        "{visible}"
    );
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
                    source: None,
                    entry: None,
                    terms: vec![vec![Inline::Text {
                        value: "1.".to_owned(),
                    }]],
                    description: vec![paragraph(vec![Inline::Text {
                        value: "first reference".to_owned(),
                    }])],
                    layout: mant_ir::DefinitionLayout {
                        inline_term: true,
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
fn keeps_block_definition_descriptions_on_their_own_commonmark_line() {
    let definitions = Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                inline_term: true,
                spacing_before_lines: None,
                ..Default::default()
            },
            terms: vec![vec![Inline::Text {
                value: "plain".to_owned(),
            }]],
            description: vec![Block::Preformatted {
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

#[test]
fn escapes_literal_roff_quote_backticks_without_hiding_styles() {
    let query = ResolvedContent {
        address: None,
        label: "quote".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![paragraph(vec![
                Inline::Text {
                    value: "For example, `".to_owned(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "!".to_owned(),
                    }],
                },
                Inline::Text {
                    value: "' remains bold.".to_owned(),
                },
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains("For example, \\`**!**' remains bold."),
        "{markdown}"
    );
    assert!(Parser::new(&markdown).any(|event| matches!(event, Event::Start(Tag::Strong))));
    assert!(!Parser::new(&markdown).any(|event| matches!(event, Event::Code(_))));
}

#[test]
fn addressable_rendering_returns_exact_semantic_node_ranges() {
    let entry = DefinitionItem {
        source: None,
        entry: Some(EntryFacts {
            name_bindings: Vec::new(),
            alias_groups: Vec::new(),
            alias_of: None,
            forms: Vec::new(),
            id: "help-entry".into(),
            kind: EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            case: NameCase::Sensitive,
            names: vec!["--help".to_owned()],
            value_domain: None,
        }),
        terms: vec![vec![
            Inline::anchor("help-entry"),
            Inline::Code {
                value: "--help".to_owned(),
            },
        ]],
        description: vec![paragraph(vec![Inline::Text {
            value: "Show help.".to_owned(),
        }])],
        layout: mant_ir::DefinitionLayout {
            inline_term: false,
            spacing_before_lines: None,
            ..Default::default()
        },
    };
    let query = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(manual(vec![section(
            "OPTIONS",
            vec![
                Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    items: vec![entry],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                },
                paragraph(vec![Inline::Text {
                    value: "Following section prose.".to_owned(),
                }]),
            ],
            Vec::new(),
        )])),
        tldr: None,
    };

    let artifact = render_addressable_markdown(&query);
    let mapped = artifact
        .nodes()
        .iter()
        .find(|mapped| matches!(mapped.node, MarkdownNode::DocumentEntry { .. }))
        .expect("semantic entry range");
    let MarkdownNode::DocumentEntry { path, owner, .. } = &mapped.node else {
        unreachable!();
    };
    assert_eq!(path.to_string(), "1/e1");
    assert_eq!(owner.facts().unwrap().id, "help-entry");
    let rendered = &artifact.text()[mapped.range.clone()];
    assert!(rendered.contains("--help"));
    assert!(rendered.contains("Show help."));
    assert!(!rendered.contains("Following section prose."));
}

#[cfg(all(unix, feature = "roff"))]
#[test]
fn serializes_a_large_source_lowered_document() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../libmandoc-rs/vendor/mandoc-cvs-20260911/mandoc.1");
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
fn final_artifact_owns_only_real_anchor_ranges() {
    let mut builder = super::ArtifactBuilder {
        track: true,
        ..super::ArtifactBuilder::default()
    };
    builder.begin_root(0);
    builder.push("<a id=\"real\"></a>\n`<a id=\"literal\"></a>`\n\n ");
    let artifact = builder.finish();
    assert!(artifact.anchors.get().is_none());
    let ranges = artifact.anchor_ranges();
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0], 0.."<a id=\"real\"></a>".len());
    assert_eq!(&artifact.text()[ranges[0].clone()], "<a id=\"real\"></a>");
    assert!(std::ptr::eq(ranges, artifact.anchor_ranges()));
    assert!(artifact.text().ends_with('`'));
    assert_eq!(artifact.nodes().len(), 1);
    assert_eq!(artifact.nodes()[0].range, 0..artifact.text().len());
    assert!(matches!(
        artifact.nodes()[0].node,
        MarkdownNode::DocumentRoot
    ));
    let final_text = artifact.text().to_owned();
    assert_eq!(artifact.into_text(), final_text);
}

#[test]
fn public_artifact_section_lookup_rejects_out_of_range_slots() {
    let content = parse_content("# Tool\n\n## Parent\n\n### Child\n\nBody.\n", None).unwrap();
    let artifact = render_addressable_markdown(&content);
    assert_eq!(artifact.section(0).unwrap().path().to_string(), "1");
    let child = artifact.section(1).unwrap();
    assert_eq!(child.parent(), Some(0));
    assert_eq!(child.section().heading.plain_text(), "Child");
    assert!(artifact.section(2).is_none());
    assert!(artifact.section(usize::MAX).is_none());
    for mapped in artifact.nodes() {
        assert!(artifact.text().get(mapped.range()).is_some());
        if let MarkdownNode::DocumentSection { section, .. } = mapped.node() {
            assert!(artifact.section(*section).is_some());
        }
    }
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
    assert!(mant_ir::validate_document(document).is_empty());
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
