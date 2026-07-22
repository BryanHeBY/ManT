//! Contract-oriented tests for `CommonMark` structure and escaping.

use mant_ast::{
    Block, DefinitionItem, DocumentMeta, DocumentSchema, DocumentSource, Inline, LayoutHint,
    ListItem, ListKind, MantDocument, Producer, QueryBundle, QuerySchema, Section, SourceFormat,
    TableCell, TableRow, TldrCommandPart, TldrDocument, TldrExample,
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::render_markdown;
use super::{render_excerpt_markdown, render_outline_markdown};
use crate::{build_outline, select_excerpt};

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn manual(sections: Vec<Section>) -> MantDocument {
    MantDocument {
        schema: DocumentSchema::V2,
        producer: Producer {
            name: "test".to_owned(),
            version: "1".to_owned(),
            engine: None,
        },
        source: DocumentSource {
            format: SourceFormat::Man,
            path: None,
            renderer: None,
        },
        meta: DocumentMeta::default(),
        diagnostics: Vec::new(),
        sections,
    }
}

fn section(title: &str, blocks: Vec<Block>, children: Vec<Section>) -> Section {
    Section {
        id: title.to_lowercase(),
        title: title.to_owned(),
        spacing_before_lines: 0,
        blocks,
        children,
        source: None,
    }
}

#[test]
fn renders_tldr_before_manual_and_resolves_placeholders() {
    let query = QueryBundle {
        schema: QuerySchema::V2,
        topic: "ls".to_owned(),
        section: None,
        manual: Some(manual(vec![section("NAME", Vec::new(), Vec::new())])),
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
        }),
    };

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# ls\n\n## TLDR"));
    assert!(markdown.find("## TLDR") < markdown.find("## NAME"));
    assert!(markdown.contains("```sh\nls --all\n```"));
    assert!(!markdown.contains("{{[-a|--all]}}"));
    assert!(markdown.contains("**More information:** <https://example.com/manual_page.html>."));
    assert!(markdown.contains("*tldr-pages · CC BY 4.0 · common · en*"));
    assert!(markdown.contains("\n\n---\n\n<a id=\"name\"></a>\n\n## NAME"));
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
    let query = QueryBundle {
        schema: QuerySchema::V2,
        topic: "demo * command".to_owned(),
        section: None,
        manual: Some(manual(vec![section(
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
        items: vec![DefinitionItem {
            identity: None,
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
    let query = QueryBundle {
        schema: QuerySchema::V2,
        topic: "man".to_owned(),
        section: Some("1".to_owned()),
        manual: Some(manual(vec![section(
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
fn chooses_safe_fences_and_preserves_native_table_and_equation_content() {
    let query = QueryBundle {
        schema: QuerySchema::V2,
        topic: "demo".to_owned(),
        section: None,
        manual: Some(manual(vec![section(
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
        .join("../../..")
        .join("tests/contracts/minimal-query-v2.json");
    let query: QueryBundle =
        serde_json::from_str(&std::fs::read_to_string(fixture).expect("shared query fixture"))
            .expect("query contract");

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# ls\n"));
    assert!(markdown.contains("## TLDR"));
    assert!(markdown.contains("## NAME"));
    assert!(markdown.contains("**ls**"));
    assert!(
        markdown.contains("[the project site](https://example.test/ls \"Project documentation\")")
    );
    assert!(markdown.contains("[the documentation team](mailto:docs@example.test)"));
    assert!(markdown.contains("[OPTIONS](#options-1)"));
    assert!(markdown.contains("<a id=\"options-1\"></a>\n\n## OPTIONS"));
    assert!(markdown.contains("<a id=\"all-option\"></a>"));
    assert!(!markdown.contains("mant.query/v2"));
}

#[test]
fn protects_paragraph_lines_from_accidental_block_syntax() {
    let query = QueryBundle {
        schema: QuerySchema::V2,
        topic: "syntax".to_owned(),
        section: None,
        manual: Some(manual(vec![section(
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
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains("\\- not a list  \n1\\. not an ordered list  \n\\# not a heading"),
        "{markdown}"
    );
}

#[test]
fn renders_selectable_outline_paths_and_excerpt_breadcrumbs() {
    let query = QueryBundle {
        schema: QuerySchema::V2,
        topic: "demo".to_owned(),
        section: Some("1".to_owned()),
        manual: Some(manual(vec![section(
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
        )])),
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
fn serializes_a_large_source_lowered_document() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../libmandoc-rs/vendor/mandoc-1.14.6/mandoc.1");
    let document = crate::parse_manual_source(&source).expect("large native document");
    let query = QueryBundle {
        schema: QuerySchema::V2,
        topic: "mandoc".to_owned(),
        section: Some("1".to_owned()),
        manual: Some(document),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.starts_with("# mandoc\n"));
    assert!(markdown.contains("## NAME"));
    assert!(markdown.contains("## DESCRIPTION"));
    assert!(!markdown.contains("<pre"));
}
