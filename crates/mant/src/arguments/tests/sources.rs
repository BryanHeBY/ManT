//! Sources behavior contracts.

use super::*;
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
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: true,
            policy: LoadPolicy::Combined,
            preserve_anchors: false,
        }
    );

    #[cfg(feature = "tui")]
    assert!(matches!(
        parse(&args(&["git", "--follow-links", "--display", "tui"]))
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
            presentation: OutputOptions { display: DisplayMode::Tui, .. },
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
                    entries: EntryProjection::Summary,
                    root: None,
                    references: _,
                }
            },
            presentation: OutputOptions {
                format: None,
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            ..
        }
    ));
    #[cfg(feature = "roff")]
    assert!(
        parse(&args(&["--input", "README.md", "--man-section", "1"]))
            .expect_err("input has no man section selector")
            .to_string()
            .contains("cannot be used with")
    );
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
            presentation: OutputOptions { format: None, color: ColorMode::Auto, .. },
            ..
        } if selector == "git-checkout"
    ));
    assert!(matches!(
        parse(&args(&["git", "--tldr", "--format", "json"])).expect("structured tldr output"),
        Command::Query {
            presentation: OutputOptions {
                format: Some(QueryFormat::Json),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            ..
        }
    ));

    for values in [
        vec!["1", "tar", "--tldr"],
        vec!["tar(1)", "--tldr"],
        #[cfg(feature = "roff")]
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
                policy: LoadPolicy::TldrOnly,
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
            policy: LoadPolicy::TldrOnly,
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
            presentation: OutputOptions {
                format: Some(QueryFormat::Json),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: false,
            policy: LoadPolicy::Combined,
            preserve_anchors: false,
        }
    );
}

#[test]
#[cfg(feature = "roff")]
fn parses_explicit_manual_and_tldr_selections() {
    assert!(matches!(
        parse(&args(&["tar", "--manual", "--format", "json"])).expect("manual-only query"),
        Command::Query {
            policy: LoadPolicy::ManualOnly,
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
            presentation: OutputOptions { format: None, color: ColorMode::Auto, .. },
            policy: LoadPolicy::TldrOnly,
            ..
        } if selectors == &[mant_protocol::ContentSelector::id("tldr")]
    ));
}
