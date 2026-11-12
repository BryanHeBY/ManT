//! Queries behavior contracts.

use super::*;
#[test]
fn parses_outline_and_repeatable_node_views_with_contextual_defaults() {
    assert_eq!(
        parse(&args(&["gcc", "--outline"])).expect("outline"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot14,
                input: QueryInput::Document {
                    selector: "gcc".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Outline {
                    entries: EntryProjection::Summary,
                    root: None,
                    references: mant_protocol::ReferenceProjection::default(),
                },
            }),
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
    assert!(matches!(
        parse(&args(&[
            "ssh",
            "--outline",
            "--outline-entries",
            "all",
            "--outline-root=path:1/e1",
        ]))
        .expect("rooted full outline"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                view: QueryView::Outline {
                    entries: EntryProjection::All,
                    root: Some(ref root),
                    ..
                },
                ..
            }),
            ..
        } if root == &mant_protocol::ContentSelector::path("1/e1")
    ));
    assert!(matches!(
        parse(&args(&[
            "bash",
            "--outline",
            "--outline-entries",
            "marker,operand,value,marker",
        ]))
        .expect("filtered outline"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                view: QueryView::Outline {
                    entries: EntryProjection::Kinds { ref kinds },
                    root: None,
                    ..
                },
                ..
            }),
            ..
        } if kinds == &[
            EntryKind::Parameter { parameter_kind: ParameterKind::Marker },
            EntryKind::Parameter { parameter_kind: ParameterKind::Operand },
            EntryKind::Value,
        ]
    ));
    for invalid in [
        &["git", "--outline-entries", "all"][..],
        &["git", "--outline-root", "options"][..],
        &["git", "--outline", "--outline-entries", "unknown"][..],
    ] {
        assert!(parse(&args(invalid)).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn parses_filtered_outlines_and_repeatable_nodes() {
    assert_eq!(
        parse(&args(&[
            "tar",
            "--outline",
            "--outline-entries",
            "option",
            "--format",
            "json",
        ]))
        .expect("option outline"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot14,
                input: QueryInput::Document {
                    selector: "tar".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Outline {
                    entries: EntryProjection::Kinds {
                        kinds: vec![EntryKind::Parameter {
                            parameter_kind: ParameterKind::Option,
                        }],
                    },
                    root: None,
                    references: mant_protocol::ReferenceProjection::default(),
                },
            }),
            presentation: OutputOptions {
                format: Some(QueryFormat::Json),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: true,
            policy: LoadPolicy::Combined,
            preserve_anchors: false,
        }
    );
    assert_eq!(
        parse(&args(&[
            "gcc",
            "--node",
            "4.2",
            "--node",
            "id:files-8",
            "--format",
            "text",
        ]))
        .expect("excerpt"),
        Command::Query {
            source: QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V0Dot14,
                input: QueryInput::Document {
                    selector: "gcc".to_owned(),
                    source: None,
                    manual_section: None,
                },
                view: QueryView::Excerpt {
                    selectors: vec![
                        mant_protocol::ContentSelector::path("4.2"),
                        mant_protocol::ContentSelector::id("files-8")
                    ],
                },
            }),
            presentation: OutputOptions {
                format: Some(QueryFormat::Text),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: true,
            policy: LoadPolicy::Combined,
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
                    schema: RequestSchema::V0Dot14,
                    input: QueryInput::Document {
                        selector: "tar".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Explain {
                        entry: selector.to_owned(),
                        options: mant_protocol::ExplanationOptions::default()
                    },
                }),
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
                presentation: OutputOptions {
                    format: None,
                    color: ColorMode::Auto,
                    display: DisplayMode::Auto,
                    markdown_math: None,
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
                schema: RequestSchema::V0Dot14,
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
                schema: RequestSchema::V0Dot14,
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
            presentation: OutputOptions {
                format: Some(QueryFormat::Json),
                color: ColorMode::Auto,
                display: DisplayMode::Auto,
                markdown_math: None,
            },
            pretty: true,
            policy: LoadPolicy::Combined,
            preserve_anchors: false,
        }
    );
}
