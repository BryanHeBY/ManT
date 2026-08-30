//! Contract-oriented tests for `CommonMark` structure and escaping.

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Document,
    DocumentMeta, DocumentSource, Inline, LayoutHint, ListItem, ListKind, Section, SourceFormat,
    TableCell, TableRow, TldrCommandPart, TldrDocument, TldrExample, TldrOrigin,
};
use mant_protocol::QueryBundle;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown, render_excerpt_markdown,
    render_markdown, render_markdown_with_options, render_outline_markdown,
};
use crate::{ResolvedContent, build_outline, select_excerpt};

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn manual(sections: Vec<Section>) -> Document {
    Document {
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Man,
            path: None,
        },
        meta: DocumentMeta::default(),
        diagnostics: Vec::new(),
        blocks: Vec::new(),
        sections,
    }
}

fn section(title: &str, blocks: Vec<Block>, children: Vec<Section>) -> Section {
    Section {
        id: title.to_lowercase().into(),
        title: title.to_owned(),
        spacing_before_lines: 0,
        blocks,
        children,
        source: None,
    }
}

#[test]
fn renders_tldr_before_manual_and_resolves_placeholders() {
    let query = ResolvedContent {
        address: None,
        label: "ls".to_owned(),
        document: Some(manual(vec![section("NAME", Vec::new(), Vec::new())])),
        tldr: Some(TldrDocument {
            title: "ls".to_owned(),
            description: vec!["List directory contents.".to_owned()],
            more_information: Some("https://example.com/manual_page.html.".to_owned()),
            examples: vec![TldrExample {
                description: "List all files".to_owned(),
                command: "ls {{[-a|--all]}}".to_owned(),
                command_parts: vec![TldrCommandPart::Text {
                    value: "ls --all".to_owned(),
                }],
            }],
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/cache/pages/common/ls.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        }),
    };

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# ls\n\n## TLDR"));
    assert!(markdown.find("## TLDR") < markdown.find("## NAME"));
    assert!(markdown.contains("```sh\nls --all\n```"));
    assert!(!markdown.contains("{{[-a|--all]}}"));
    assert!(markdown.contains("**More information:** <https://example.com/manual_page.html>."));
    assert!(markdown.contains("*tldr-pages · CC BY 4.0 · common · en*"));
    assert!(markdown.contains("\n\n---\n\n## NAME"));
    assert!(!markdown.contains("<a "));
    assert!(!markdown.ends_with('\n'));

    let outline = render_outline_markdown(&build_outline(&query).expect("combined outline"));
    assert!(outline.contains("- `0` (`tldr`) TLDR QUICK REFERENCE"));
    assert!(outline.contains("- `1` (`name`) NAME"));

    let excerpt = select_excerpt(&query, &["0".to_owned()]).expect("tldr excerpt");
    let excerpt = render_excerpt_markdown(&excerpt);
    assert!(excerpt.contains("*Outline `0`: TLDR QUICK REFERENCE*"));
    assert!(excerpt.contains("## TLDR"));
    assert!(excerpt.contains("```sh\nls --all\n```"));
    assert!(!excerpt.contains("## NAME"));
}

#[test]
fn renders_and_selects_content_before_the_first_heading() {
    let mut document = manual(vec![section("GUIDE", Vec::new(), Vec::new())]);
    document.source.format = SourceFormat::Markdown;
    document.blocks = vec![paragraph(vec![Inline::Text {
        value: "Document preface.".to_owned(),
    }])];
    let query = ResolvedContent {
        address: None,
        label: "guide.md".to_owned(),
        document: Some(document),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("# guide.md\n\nDocument preface.\n\n## GUIDE"));
    assert!(!markdown.contains("<a "));

    let addressable = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert!(addressable.contains("<a id=\"document-overview\"></a>\n\nDocument preface."));

    let outline = build_outline(&query).expect("Markdown outline");
    assert_eq!(outline.nodes[0].path(), "root");
    assert_eq!(outline.nodes[1].path(), "1");

    let excerpt = select_excerpt(&query, &["root".to_owned()]).expect("root excerpt");
    let excerpt = render_excerpt_markdown(&excerpt);
    assert!(excerpt.contains("*Outline `root`: OVERVIEW*"));
    assert!(excerpt.contains("Document preface."));
    assert!(!excerpt.contains("## GUIDE"));
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
        start: None,
        compact: true,
        items: vec![ListItem {
            blocks: vec![paragraph(vec![Inline::Text {
                value: "first item".to_owned(),
            }])],
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let definitions = Block::DefinitionList {
        items: vec![DefinitionItem {
            identity: None,
            inline_term: false,
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
            spacing_before_lines: None,
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
    assert!(markdown.contains("- **-a**, **--all**"));
    assert!(markdown.contains("Show all entries."));
}

#[test]
fn keeps_adjacent_bold_and_italic_runs_unambiguous_in_commonmark() {
    let definitions = Block::DefinitionList {
        items: vec![DefinitionItem {
            identity: None,
            inline_term: false,
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
            spacing_before_lines: None,
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
fn renders_the_shared_query_contract_without_leaking_json() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/contracts/minimal-query-v0.10.json");
    if !fixture.exists() {
        // The tagged repository owns shared process-contract fixtures; they
        // intentionally remain outside the published engine package.
        return;
    }
    let query = serde_json::from_str::<QueryBundle>(
        &std::fs::read_to_string(fixture).expect("shared query fixture"),
    )
    .expect("query contract")
    .into();

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# ls\n"));
    assert!(markdown.contains("## TLDR"));
    assert!(markdown.contains("## NAME"));
    assert!(markdown.contains("**ls**"));
    assert!(
        markdown.contains("[the project site](https://example.test/ls \"Project documentation\")")
    );
    assert!(markdown.contains("[the documentation team](mailto:docs@example.test)"));
    assert!(markdown.contains(", or read OPTIONS"));
    assert!(!markdown.contains("[OPTIONS](#options-1)"));
    assert!(!markdown.contains("<a "));
    assert!(!markdown.contains("mant.query/v0.10"));

    let addressable = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert!(addressable.contains("[OPTIONS](#options-1)"));
    assert!(addressable.contains("<a id=\"options-1\"></a>\n\n## OPTIONS"));
    assert!(addressable.contains("<a id=\"all-option\"></a>"));
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
                items: vec![DefinitionItem {
                    identity: None,
                    terms: vec![vec![Inline::Text {
                        value: "1.".to_owned(),
                    }]],
                    description: vec![paragraph(vec![Inline::Text {
                        value: "first reference".to_owned(),
                    }])],
                    inline_term: true,
                    spacing_before_lines: None,
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
        items: vec![DefinitionItem {
            identity: None,
            inline_term: true,
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
            spacing_before_lines: None,
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
fn uses_markdown_document_title_without_changing_its_logical_label() {
    let mut document = manual(Vec::new());
    document.source.format = SourceFormat::Markdown;
    document.meta.title = Some("Actual Doc Title".to_owned());
    document.blocks = vec![paragraph(vec![Inline::Text {
        value: "body".to_owned(),
    }])];
    let query = ResolvedContent {
        address: None,
        label: "filename.md".to_owned(),
        document: Some(document),
        tldr: None,
    };

    assert!(render_markdown(&query).starts_with("# Actual Doc Title\n\nbody"));
    let outline = render_outline_markdown(&build_outline(&query).expect("outline"));
    assert!(
        outline.starts_with("# Actual Doc Title outline"),
        "{outline}"
    );
    let excerpt = render_excerpt_markdown(
        &select_excerpt(&query, &["root".to_owned()]).expect("root excerpt"),
    );
    assert!(excerpt.starts_with("# Actual Doc Title"), "{excerpt}");
}

#[test]
fn renders_selectable_outline_paths_and_excerpt_breadcrumbs() {
    let query = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some({
            let mut document = manual(vec![section(
                "OPTIONS",
                vec![paragraph(vec![Inline::Text {
                    value: "parent details".to_owned(),
                }])],
                vec![section(
                    "Common options",
                    vec![paragraph(vec![Inline::Strong {
                        children: vec![Inline::Text {
                            value: "child details".to_owned(),
                        }],
                    }])],
                    Vec::new(),
                )],
            )]);
            document.meta.manual_section = Some("1".to_owned());
            document
        }),
        tldr: None,
    };

    let outline = build_outline(&query).expect("outline");
    let outline_markdown = render_outline_markdown(&outline);
    assert!(outline_markdown.starts_with("# demo(1) outline"));
    assert!(outline_markdown.contains("- `1` (`options`) OPTIONS"));
    assert!(outline_markdown.contains("  - `1.1` (`common options`) Common options"));

    let excerpt = select_excerpt(&query, &["1.1".to_owned()]).expect("excerpt");
    let excerpt_markdown = render_excerpt_markdown(&excerpt);
    assert!(excerpt_markdown.starts_with("# demo(1)"));
    assert!(excerpt_markdown.contains("*Outline `1.1`: OPTIONS → Common options*"));
    assert!(excerpt_markdown.contains("## Common options"));
    assert!(excerpt_markdown.contains("**child details**"));
    assert!(!excerpt_markdown.contains("parent details"));
}

#[test]
fn addressable_rendering_returns_exact_semantic_node_ranges() {
    let entry = DefinitionItem {
        identity: Some(DefinitionIdentity {
            id: "help-entry".into(),
            role: DefinitionRole::Option,
            case: DefinitionCase::Sensitive,
            names: vec!["--help".to_owned()],
        }),
        terms: vec![vec![
            Inline::Anchor {
                id: "help-entry".into(),
            },
            Inline::Code {
                value: "--help".to_owned(),
            },
        ]],
        description: vec![paragraph(vec![Inline::Text {
            value: "Show help.".to_owned(),
        }])],
        inline_term: false,
        spacing_before_lines: None,
    };
    let query = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(manual(vec![section(
            "OPTIONS",
            vec![
                Block::DefinitionList {
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
        .nodes
        .iter()
        .find(|mapped| matches!(mapped.node, MarkdownNode::DocumentEntry { .. }))
        .expect("semantic entry range");
    let MarkdownNode::DocumentEntry { path, id, .. } = &mapped.node else {
        unreachable!();
    };
    assert_eq!(path.to_string(), "1/e1");
    assert_eq!(id, "help-entry");
    let rendered = &artifact.text[mapped.range.clone()];
    assert!(rendered.contains("--help"));
    assert!(rendered.contains("Show help."));
    assert!(!rendered.contains("Following section prose."));
}

#[cfg(unix)]
#[test]
fn serializes_a_large_source_lowered_document() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../libmandoc-rs/vendor/mandoc-1.14.6/mandoc.1");
    if !source.exists() {
        // Published package tests must not require a sibling crate's vendor
        // tree; repository verification still exercises the real fixture.
        return;
    }
    let document = crate::parse_manual_source(&source).expect("large native document");
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
