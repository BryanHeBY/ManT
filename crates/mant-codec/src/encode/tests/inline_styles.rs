use super::*;

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

#[test]
fn keeps_adjacent_bold_and_italic_runs_unambiguous_in_commonmark() {
    let definitions = Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                head_body_relation: mant_ir::HeadBodyRelation::from(false),
                spacing_before_lines: None,
                ..Default::default()
            },
            terms: (vec![vec![
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
            ]])
            .into_iter()
            .map(Into::into)
            .collect(),
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
