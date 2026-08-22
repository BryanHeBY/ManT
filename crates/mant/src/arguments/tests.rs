use mant_protocol::{
    CatalogDocumentKind, CatalogQuery, DocumentScope, DocumentSelector, DocumentTraversal,
    InputFormat, OutlineDetail, QueryInput, QueryRequest, QueryView, RequestSchema, ScopeQueryView,
    SearchCase, SearchScope, SearchSyntax,
};

use super::{
    CatalogPaging, ColorMode, Command, QueryFormat, QueryPolicy, QueryPresentation, QuerySource,
    SchemaContract, parse, parse_process, requested_color,
};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn defaults_direct_queries_to_markdown() {
    assert_eq!(
        parse(&args(&["git"])).expect("query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot9,
                input: QueryInput::Document {
                    selector: "git".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Full {},
            }),
            presentation: QueryPresentation::Auto,
            pretty: true,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
}

#[test]
fn parses_grouped_lists_and_grep_like_catalog_searches() {
    assert_eq!(
        parse(&args(&["--list", "--source", "pwsh7"])).expect("catalog list"),
        Command::Catalog {
            query: CatalogQuery {
                pattern: None,
                source: Some("pwsh7".to_owned()),
                limit: 10_000,
                ..CatalogQuery::default()
            },
            grouped: true,
            format: QueryFormat::Text,
            pretty: true,
            paging: CatalogPaging::Auto,
        }
    );
    assert_eq!(
        parse(&args(&[
            "--find",
            "^PRINT",
            "--regex",
            "--case",
            "sensitive",
            "--kind",
            "manual",
            "--man-section",
            "3",
            "--limit",
            "20",
            "--format",
            "json",
            "--compact",
        ]))
        .expect("catalog search"),
        Command::Catalog {
            query: CatalogQuery {
                pattern: Some("^PRINT".to_owned()),
                syntax: SearchSyntax::Regex,
                case: SearchCase::Sensitive,
                kind: Some(CatalogDocumentKind::Manual),
                source: None,
                manual_section: Some("3".to_owned()),
                limit: 20,
                offset: 0,
            },
            grouped: false,
            format: QueryFormat::Json,
            pretty: false,
            paging: CatalogPaging::Auto,
        }
    );

    assert!(matches!(
        parse(&args(&["--list", "--no-pager"])).expect("direct catalog list"),
        Command::Catalog {
            paging: CatalogPaging::Disabled,
            ..
        }
    ));

    for invalid in [
        vec!["--list", "--regex"],
        vec!["--list", "--format", "markdown"],
        vec!["git", "--limit", "2"],
        vec!["git", "--kind", "manual"],
        vec!["git", "--no-pager"],
        vec!["git", "--outline", "--format", "man"],
        vec!["git", "--node", "1", "--format", "man"],
        vec!["git", "--explain", "branch", "--format", "man"],
        vec!["git", "--search", "branch", "--format", "man"],
    ] {
        assert!(parse(&args(&invalid)).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn parses_an_explicit_interactive_query_without_an_output_projection() {
    assert!(matches!(
        parse(&args(&["git", "--ui"])).expect("interactive query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document { ref selector, .. },
                view: QueryView::Full {},
                ..
            }),
            presentation: QueryPresentation::Interactive,
            ..
        } if selector == "git"
    ));

    for conflicting in ["--outline", "--search=git", "--format=json"] {
        assert!(
            parse(&args(&["git", "--ui", conflicting])).is_err(),
            "accepted {conflicting}"
        );
    }
}

#[test]
fn parses_bounded_multi_document_queries_without_changing_single_document_syntax() {
    assert_eq!(
        parse(&args(&[
            "--document",
            "manual/1/git",
            "--document",
            "documents/mant",
            "--follow-links",
            "--max-depth",
            "3",
            "--max-documents",
            "12",
            "--search",
            "worktree",
        ]))
        .expect("bounded document scope"),
        Command::Query {
            source: QuerySource::ScopeArguments {
                scope: DocumentScope {
                    documents: vec![
                        DocumentSelector {
                            selector: "manual/1/git".to_owned(),
                            source: None,
                            manual_section: None,
                        },
                        DocumentSelector {
                            selector: "documents/mant".to_owned(),
                            source: None,
                            manual_section: None,
                        },
                    ],
                    traversal: DocumentTraversal {
                        follow_links: true,
                        max_depth: Some(3),
                        max_documents: Some(12),
                    },
                },
                view: Some(ScopeQueryView::Search {
                    pattern: "worktree".to_owned(),
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Insensitive,
                    scope: SearchScope::Visible,
                    word: false,
                    context_lines: 0,
                    limit: 100,
                    offset: 0,
                }),
            },
            presentation: QueryPresentation::Output {
                format: QueryFormat::Text,
                color: ColorMode::Auto,
            },
            pretty: true,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );

    assert!(matches!(
        parse(&args(&["git", "--follow-links", "--ui"]))
            .expect("interactive transitive scope"),
        Command::Query {
            source: QuerySource::ScopeArguments {
                view: None,
                scope: DocumentScope {
                    ref documents,
                    traversal: DocumentTraversal {
                        follow_links: true,
                        ..
                    },
                },
            },
            presentation: QueryPresentation::Interactive,
            ..
        } if documents[0].selector == "git"
    ));

    for invalid in [
        vec!["--document", "git", "--outline"],
        vec!["--document", "git", "--tldr"],
        vec!["--document", "git", "--max-depth", "2", "--search", "x"],
        vec!["git", "--follow-links", "--manual"],
        vec!["--list", "--follow-links"],
    ] {
        assert!(parse(&args(&invalid)).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn dispatches_explicit_files_and_direct_stdin_without_embedding_content() {
    for path in ["README.md", "docs/guide", "./notes"] {
        assert!(matches!(
            parse(&args(&["--input", path])).expect("input file query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::File {
                        path: parsed,
                        format: InputFormat::Auto,
                    },
                    ..
                }),
                ..
            } if parsed == path
        ));
    }

    assert!(matches!(
        parse(&args(&[
            "--input",
            "-",
            "--input-format",
            "markdown",
            "--outline"
        ]))
        .expect("piped Markdown outline"),
        Command::Query {
            source: QuerySource::InputStdin {
                format: InputFormat::Markdown,
                view: QueryView::Outline {
                    detail: OutlineDetail::Entries
                }
            },
            presentation: QueryPresentation::Output {
                format: QueryFormat::Text,
                color: ColorMode::Auto
            },
            ..
        }
    ));
    assert!(
        parse(&args(&["--input", "README.md", "--man-section", "1"]))
            .expect_err("input has no man section selector")
            .to_string()
            .contains("cannot be used with")
    );
}

#[test]
fn preserves_markdown_anchors_only_when_requested() {
    assert!(matches!(
        parse(&args(&["git", "--preserve-anchors"])).expect("addressable Markdown"),
        Command::Query {
            presentation: QueryPresentation::Output {
                format: QueryFormat::Markdown,
                color: ColorMode::Auto
            },
            preserve_anchors: true,
            ..
        }
    ));
}

#[test]
fn parses_format_man_section_and_compact_json_options() {
    assert_eq!(
        parse(&args(&[
            "printf",
            "--man-section",
            "3",
            "--format",
            "json",
            "--compact",
        ]))
        .expect("query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot9,
                input: QueryInput::Document {
                    selector: "printf".to_owned(),
                    source: None,
                    manual_section: Some("3".to_owned()),
                },
                view: QueryView::Full {},
            }),
            presentation: QueryPresentation::Output {
                format: QueryFormat::Json,
                color: ColorMode::Auto
            },
            pretty: false,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
    assert!(matches!(
        parse(&args(&["printf", "--source", "team"])).expect("source query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document {
                    ref source,
                    manual_section: None,
                    ..
                },
                ..
            }),
            ..
        } if source.as_deref() == Some("team")
    ));
}

#[test]
fn removed_section_option_is_hidden_and_explains_both_replacements() {
    let Command::Help(help) = parse(&args(&["--help"])).expect("help") else {
        panic!("expected help output")
    };
    assert!(!help.contains("--section"));

    for arguments in [
        vec!["cmake", "--section", "1"],
        vec!["cmake", "--section=DESCRIPTION"],
        vec!["--section", "1"],
    ] {
        let diagnostic = parse(&args(&arguments))
            .expect_err("removed option")
            .to_string();
        assert!(diagnostic.contains("--section was removed in ManT 0.7.0"));
        assert!(diagnostic.contains("--man-section <MAN_SECTION>"));
        assert!(diagnostic.contains("--node <SELECTOR>"));
        assert!(diagnostic.contains("--outline"));
    }

    assert!(!super::uses_removed_section_option(&args(&[
        "cmake",
        "--explain",
        "--section",
    ])));
}

#[test]
fn process_help_retains_styles_while_injected_help_stays_plain() {
    let arguments = args(&["--help", "--color", "always"]);
    let Command::Help(help) = parse(&arguments).expect("captured help") else {
        panic!("expected captured help")
    };
    assert!(!help.contains('\u{1b}'));

    let styled = parse_process(&arguments).expect_err("process help remains a clap display");
    assert_eq!(styled.kind(), clap::error::ErrorKind::DisplayHelp);
    assert!(styled.render().ansi().to_string().contains('\u{1b}'));
}

#[test]
fn color_policy_is_global_without_changing_deterministic_presentations() {
    assert!(matches!(
        parse(&args(&["git", "--format", "json", "--color", "always"]))
            .expect("JSON query with terminal color policy"),
        Command::Query {
            presentation: QueryPresentation::Output {
                format: QueryFormat::Json,
                color: ColorMode::Always
            },
            ..
        }
    ));
    assert!(matches!(
        parse(&args(&["git", "--tldr", "--color", "never"])).expect("plain tldr query"),
        Command::Query {
            presentation: QueryPresentation::Tldr(ColorMode::Never),
            ..
        }
    ));

    assert_eq!(
        requested_color(&args(&["git", "--color=always"])),
        ColorMode::Always
    );
    assert_eq!(
        requested_color(&args(&["git", "--color", "never"])),
        ColorMode::Never
    );
    assert_eq!(
        requested_color(&args(&["--", "--color=always"])),
        ColorMode::Auto
    );
}

#[test]
fn normalizes_man_style_and_hierarchical_selectors() {
    for values in [vec!["1", "git"], vec!["git(1)"]] {
        assert!(matches!(
            parse(&args(&values)).expect("man-style selector"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document {
                        ref selector,
                        manual_section: Some(ref manual_section),
                        ..
                    },
                    ..
                }),
                ..
            } if selector == "git" && manual_section == "1"
        ));
    }
    assert!(matches!(
        parse(&args(&["manual/1/git"])).expect("canonical selector"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document { ref selector, manual_section: None, .. },
                ..
            }),
            ..
        } if selector == "manual/1/git"
    ));
    assert!(matches!(
        parse(&args(&["git.1"])).expect("dotted logical name"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document {
                    ref selector,
                    manual_section: None,
                    ..
                },
                ..
            }),
            ..
        } if selector == "git.1"
    ));
}

#[test]
fn tldr_joins_multiword_topics_and_keeps_explicit_formats() {
    assert!(matches!(
        parse(&args(&["git", "checkout", "--tldr"])).expect("multiword tldr topic"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document { ref selector, .. },
                ..
            }),
            presentation: QueryPresentation::Tldr(ColorMode::Auto),
            ..
        } if selector == "git-checkout"
    ));
    assert!(matches!(
        parse(&args(&["git", "--tldr", "--format", "json"])).expect("structured tldr output"),
        Command::Query {
            presentation: QueryPresentation::Output {
                format: QueryFormat::Json,
                color: ColorMode::Auto
            },
            ..
        }
    ));

    for values in [
        vec!["1", "tar", "--tldr"],
        vec!["tar(1)", "--tldr"],
        vec!["tar", "--man-section", "1", "--tldr"],
    ] {
        assert!(matches!(
            parse(&args(&values)).expect("command section qualifies a tldr topic"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document {
                        ref selector,
                        manual_section: Some(ref manual_section),
                        ..
                    },
                    ..
                }),
                policy: QueryPolicy::TldrOnly,
                ..
            } if selector == "tar" && manual_section == "1"
        ));
    }

    assert!(matches!(
        parse(&args(&["command.1", "--tldr"]))
            .expect("dots remain part of explicit tldr topics"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document {
                    ref selector,
                    manual_section: None,
                    ..
                },
                ..
            }),
            policy: QueryPolicy::TldrOnly,
            ..
        } if selector == "command.1"
    ));
}

#[test]
fn parses_the_closed_stdin_request_mode_used_by_the_tui() {
    assert_eq!(
        parse(&args(&["--request-json", "--format", "json", "--compact",])).expect("stdin query"),
        Command::Query {
            source: QuerySource::StdinJson,
            presentation: QueryPresentation::Output {
                format: QueryFormat::Json,
                color: ColorMode::Auto
            },
            pretty: false,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
}

#[test]
fn parses_explicit_manual_and_tldr_selections() {
    assert!(matches!(
        parse(&args(&["tar", "--manual", "--format", "json"])).expect("manual-only query"),
        Command::Query {
            policy: QueryPolicy::ManualOnly,
            ..
        }
    ));
    assert!(matches!(
        parse(&args(&["tar", "--tldr"])).expect("tldr-only query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                view: QueryView::Excerpt { ref selectors },
                ..
            }),
            presentation: QueryPresentation::Tldr(ColorMode::Auto),
            policy: QueryPolicy::TldrOnly,
            ..
        } if selectors == &["tldr"]
    ));
}

#[test]
fn parses_outline_and_repeatable_node_views_with_contextual_defaults() {
    assert_eq!(
        parse(&args(&["gcc", "--outline"])).expect("outline"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot9,
                input: QueryInput::Document {
                    selector: "gcc".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Outline {
                    detail: OutlineDetail::Entries,
                },
            }),
            presentation: QueryPresentation::Output {
                format: QueryFormat::Text,
                color: ColorMode::Auto
            },
            pretty: true,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
    assert_eq!(
        parse(&args(&["tar", "--outline", "options", "--format", "json"])).expect("option outline"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot9,
                input: QueryInput::Document {
                    selector: "tar".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Outline {
                    detail: OutlineDetail::Entries,
                },
            }),
            presentation: QueryPresentation::Output {
                format: QueryFormat::Json,
                color: ColorMode::Auto
            },
            pretty: true,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
    assert_eq!(
        parse(&args(&[
            "gcc", "--node", "4.2", "--node", "files-8", "--format", "text",
        ]))
        .expect("excerpt"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot9,
                input: QueryInput::Document {
                    selector: "gcc".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Excerpt {
                    selectors: vec!["4.2".into(), "files-8".into()],
                },
            }),
            presentation: QueryPresentation::Output {
                format: QueryFormat::Text,
                color: ColorMode::Auto
            },
            pretty: true,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
}

#[test]
fn parses_explain_as_a_first_class_semantic_view() {
    for (values, selector) in [
        (vec!["tar", "--explain=--exclude"], "--exclude"),
        (vec!["tar", "--explain", "--exclude"], "--exclude"),
        (vec!["tar", "--explain", "exclude"], "exclude"),
    ] {
        assert_eq!(
            parse(&args(&values)).expect("explain query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V0Dot9,
                    input: QueryInput::Document {
                        selector: "tar".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Explain {
                        entry: selector.to_owned(),
                    },
                }),
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Text,
                    color: ColorMode::Auto
                },
                pretty: true,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
    }
}

#[test]
fn defaults_all_partial_document_views_to_text() {
    for values in [
        vec!["gcc", "--node", "4.2"],
        vec!["gcc", "--outline"],
        vec!["gcc", "--search", "link"],
    ] {
        assert!(matches!(
            parse(&args(&values)).expect("partial document query"),
            Command::Query {
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Text,
                    color: ColorMode::Auto
                },
                ..
            }
        ));
    }
}

#[test]
fn parses_literal_and_regex_searches_with_text_as_the_default() {
    assert_eq!(
        parse(&args(&["tar", "--search=--acls"])).expect("literal search"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot9,
                input: QueryInput::Document {
                    selector: "tar".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Search {
                    pattern: "--acls".to_owned(),
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Insensitive,
                    scope: SearchScope::Visible,
                    word: false,
                    context_lines: 0,
                    limit: 100,
                    offset: 0,
                },
            }),
            presentation: QueryPresentation::Output {
                format: QueryFormat::Text,
                color: ColorMode::Auto
            },
            pretty: true,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
    assert_eq!(
        parse(&args(&[
            "git",
            "--grep",
            "worktree|branch",
            "--regex",
            "--case",
            "smart",
            "--word",
            "--scope",
            "markdown",
            "--context",
            "2",
            "--limit",
            "20",
            "--offset",
            "5",
            "--format",
            "json",
        ]))
        .expect("regex search"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot9,
                input: QueryInput::Document {
                    selector: "git".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Search {
                    pattern: "worktree|branch".to_owned(),
                    syntax: SearchSyntax::Regex,
                    case: SearchCase::Smart,
                    scope: SearchScope::Markdown,
                    word: true,
                    context_lines: 2,
                    limit: 20,
                    offset: 5,
                },
            }),
            presentation: QueryPresentation::Output {
                format: QueryFormat::Json,
                color: ColorMode::Auto
            },
            pretty: true,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
}

#[test]
fn parses_long_option_actions_without_ad_hoc_subcommands() {
    assert_eq!(
        parse(&args(&["--doctor"])).expect("doctor"),
        Command::Doctor {
            format: QueryFormat::Text,
            pretty: true,
            color: ColorMode::Auto,
        }
    );
    assert_eq!(
        parse(&args(&[
            "--doctor",
            "--format",
            "json",
            "--compact",
            "--color",
            "always",
        ]))
        .expect("compact doctor JSON"),
        Command::Doctor {
            format: QueryFormat::Json,
            pretty: false,
            color: ColorMode::Always,
        }
    );
    assert_eq!(
        parse(&args(&["--update-docs", "--compact"])).expect("document update"),
        Command::UpdateDocs { pretty: false }
    );
    assert_eq!(
        parse(&args(&["--prune-docs", "--dry-run", "--compact"])).expect("document source prune"),
        Command::PruneDocs {
            pretty: false,
            dry_run: true,
        }
    );
    assert_eq!(
        parse(&args(&["--update-tldr"])).expect("update"),
        Command::UpdateTldr { pretty: true }
    );
    assert_eq!(
        parse(&args(&["--protocol-version", "--compact"])).expect("version"),
        Command::ProtocolVersion { pretty: false }
    );
    assert_eq!(
        parse(&args(&["--schema", "request", "--compact"])).expect("schema"),
        Command::Schema {
            contract: SchemaContract::Request,
            pretty: false,
        }
    );
    assert_eq!(
        parse(&args(&["--schema", "doctor"])).expect("doctor schema"),
        Command::Schema {
            contract: SchemaContract::Doctor,
            pretty: true,
        }
    );
    assert_eq!(parse(&args(&["--mcp"])).expect("MCP"), Command::Mcp);
}

#[test]
fn rejects_ambiguous_or_incompatible_inputs() {
    let cases = [
        vec!["git", "--format", "json", "--format", "text"],
        vec!["git", "--compact"],
        vec!["git", "--preserve-anchors", "--format", "json"],
        vec!["git", "--outline", "--preserve-anchors"],
        vec!["git", "--search", "branch", "--preserve-anchors"],
        vec!["--request-json", "git", "--format", "json"],
        vec!["--request-json", "--man-section", "1", "--format", "json"],
        vec!["--request-json", "--outline", "--format", "json"],
        vec!["git", "--outline", "--node", "1"],
        vec!["git", "--outline", "--search", "branch"],
        vec!["git", "--node", "1", "--search", "branch"],
        vec!["git", "--explain=--help", "--node", "help"],
        vec!["git", "--explain=--help", "--outline"],
        vec!["git", "--explain=--help", "--search", "help"],
        vec!["git", "--regex"],
        vec!["git", "--search", "branch", "--limit", "many"],
        vec!["git", "--node"],
        vec!["--man-section", "1"],
        vec!["--update-tldr", "--format", "json"],
        vec!["--update-docs", "--format", "json"],
        vec!["--prune-docs", "--format", "json"],
        vec!["--doctor", "--format", "markdown"],
        vec!["--doctor", "--compact"],
        vec!["--doctor", "--source", "team"],
        vec!["--dry-run"],
        vec!["git", "--source", "team", "--man-section", "1"],
        vec!["git", "--source", "team", "--manual"],
        vec!["git", "--manual", "--tldr"],
        vec!["git", "--tldr", "--node", "0"],
        vec!["git", "--tldr", "--ui"],
        vec!["--input", "README.md", "--source", "team"],
        vec!["--schema", "request", "--format", "json"],
        vec!["--mcp", "git"],
        vec!["--mcp", "--format", "json"],
        vec!["--mcp", "--manual"],
        vec!["--mcp", "--tldr"],
        vec!["--mcp", "--update-tldr"],
        vec!["--update-tldr", "--preserve-anchors"],
        vec!["--input", "README.md", "--manual"],
        vec!["--input", "-", "--input-format", "markdown", "--manual"],
        vec!["--request-json", "--manual", "--format", "json"],
        vec!["--schema", "unknown"],
        vec!["update", "tldr"],
        vec!["git", "--json"],
        vec!["git", "--md"],
        vec!["git", "--markdown"],
        vec!["git", "--text"],
        vec!["git", "-s", "1"],
        vec!["git", "-n", "1"],
        vec!["--unknown", "git"],
    ];
    for values in cases {
        assert!(parse(&args(&values)).is_err(), "accepted {values:?}");
    }
}

#[test]
fn help_is_side_effect_free_and_the_option_terminator_preserves_a_name() {
    for flag in ["--help", "-h"] {
        let help = parse(&args(&[flag])).expect("help");
        assert!(matches!(help, Command::Help(text) if text.contains("Usage: mant")));
    }
    assert_eq!(
        parse(&args(&["--", "--help"])).expect("query"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot9,
                input: QueryInput::Document {
                    selector: "--help".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Full {},
            }),
            presentation: QueryPresentation::Auto,
            pretty: true,
            policy: QueryPolicy::Combined,
            preserve_anchors: false,
        }
    );
}

#[test]
fn version_is_side_effect_free() {
    let version = parse(&args(&["--version"])).expect("version");
    assert!(
        matches!(version, Command::Help(text) if text == concat!("mant ", env!("CARGO_PKG_VERSION"), "\n"))
    );
}
