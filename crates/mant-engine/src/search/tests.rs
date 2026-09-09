use crate::ResolvedContent;
use mant_ir::{
    Block, DefinitionItem, Document, DocumentMeta, DocumentSource, EntryFacts, EntryKind, Inline,
    LayoutHint, NameCase, Section, SourceFormat,
};
use mant_protocol::{MAX_SEARCH_PATTERN_CHARS, SearchCase, SearchQuery, SearchScope, SearchSyntax};

use super::{
    LineIndex, MAX_OCCURRENCES_PER_MATCH, SearchError, display_markdown_line,
    occurrence_line_ranges, render_addressable_markdown, search_query, validate_search_query,
};

fn query() -> ResolvedContent {
    ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(Document {
            heading: None,
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Man,
                path: None,
            },
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "options-1".to_owned().into(),
                fragment_aliases: Vec::new(),
                heading: "OPTIONS".into(),
                spacing_before_lines: 0,
                blocks: vec![Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    items: vec![DefinitionItem {
                        source: None,
                        layout: mant_ir::DefinitionLayout {
                            inline_term: false,
                            spacing_before_lines: None,
                            ..Default::default()
                        },
                        entry: Some(EntryFacts {
                            name_bindings: Vec::new(),
                            alias_groups: Vec::new(),
                            alias_of: None,
                            forms: Vec::new(),
                            id: "option-acls".to_owned().into(),
                            kind: EntryKind::Parameter {
                                parameter_kind: mant_ir::ParameterKind::Option,
                            },
                            case: NameCase::Sensitive,
                            names: vec!["--acls".to_owned()],
                            value_domain: None,
                        }),
                        terms: vec![vec![
                            Inline::anchor("option-acls"),
                            Inline::Code {
                                value: "--acls".to_owned(),
                            },
                        ]],
                        description: vec![Block::Paragraph {
                            children: vec![
                                Inline::Text {
                                    value: "Preserve ".to_owned(),
                                },
                                Inline::Strong {
                                    children: vec![Inline::Text {
                                        value: "access control".to_owned(),
                                    }],
                                },
                                Inline::Text {
                                    value: " lists".to_owned(),
                                },
                            ],
                            layout: LayoutHint::default(),
                            source: None,
                        }],
                    }],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: Vec::new(),
                source: None,
            }],
        }),
        tldr: None,
    }
}

fn request(pattern: &str) -> SearchQuery {
    SearchQuery {
        pattern: pattern.to_owned(),
        syntax: SearchSyntax::Literal,
        case: SearchCase::Insensitive,
        scope: SearchScope::Visible,
        word: false,
        context_lines: 1,
        limit: 100,
        offset: 0,
    }
}

#[test]
fn visible_search_maps_inline_formatting_to_markdown_and_option_nodes() {
    let result = search_query(&query(), &request("access control")).expect("search");

    assert_eq!(result.total, 1);
    assert_eq!(result.matches[0].outline.node.path(), "1/e1");
    assert_eq!(
        result.matches[0].occurrences[0].matched_text,
        "access control"
    );
    assert_eq!(result.matches[0].occurrences[0].line_ranges.len(), 1);
    assert!(result.matches[0].occurrences[0].markdown.start_line > 1);
    assert!(result.matches[0].preview.contains("**access control**"));
    assert!(!result.matches[0].preview.contains("<a id="));
    assert!(!result.matches[0].context.is_empty());
}

#[test]
fn presented_line_ranges_exclude_trimmed_unicode_trailing_space() {
    let markdown = "zz 日本語   \nnext";
    let lines = LineIndex::new(markdown);
    let ranges = occurrence_line_ranges(3..15, markdown, &lines);

    assert_eq!(display_markdown_line(lines.line(markdown, 0)), "zz 日本語");
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].line, 1);
    assert_eq!(ranges[0].start_byte, 3);
    assert_eq!(ranges[0].end_byte, 12);
}

#[test]
fn styled_identifiers_are_visible_matches_not_contiguous_markdown_source() {
    let mut query = query();
    query.document.as_mut().unwrap().sections[0]
        .blocks
        .push(Block::Paragraph {
            children: vec![
                Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: "NAME".into(),
                    }],
                },
                Inline::Text {
                    value: "_PID".into(),
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        });
    assert_eq!(search_query(&query, &request("NAME_PID")).unwrap().total, 1);
    let raw = SearchQuery {
        scope: SearchScope::Markdown,
        ..request("NAME_PID")
    };
    assert_eq!(search_query(&query, &raw).unwrap().total, 0);
    let rendered = crate::render_markdown(&query);
    assert!(rendered.contains("*NAME*\\_PID"));
}

#[test]
fn searches_contiguous_text_across_an_unsafe_style_boundary() {
    let mut query = query();
    query.document.as_mut().expect("fixture document").sections[0]
        .blocks
        .push(Block::Paragraph {
            children: vec![
                Inline::Text {
                    value: "disabled with --".to_owned(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "no-".to_owned(),
                    }],
                },
                Inline::Text {
                    value: "option".to_owned(),
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        });

    let visible = search_query(&query, &request("no-option")).expect("visible search");
    assert_eq!(visible.total, 1);
    assert_eq!(visible.matches[0].occurrences[0].matched_text, "no-option");
    assert!(visible.matches[0].preview.contains("--no-option"));
    assert!(!visible.matches[0].preview.contains("**no-**"));

    let markdown = search_query(
        &query,
        &SearchQuery {
            scope: SearchScope::Markdown,
            ..request("no-option")
        },
    )
    .expect("Markdown search");
    assert_eq!(markdown.total, 1);
    assert_eq!(markdown.matches[0].occurrences[0].matched_text, "no-option");
}

#[test]
fn source_map_stripping_uses_codec_supplied_anchor_ranges() {
    let source = "before<a id=\"node\"></a>after";
    let lines = LineIndex::with_anchors(source, std::iter::once(6..23).collect());
    assert_eq!(lines.presented_line(source, 0).text, "beforeafter");
    assert_eq!(
        display_markdown_line("before<a id=\"node\">payload</a>after"),
        "before<a id=\"node\">payload</a>after"
    );
    assert_eq!(
        display_markdown_line("before<a id=\"node\"after"),
        "before<a id=\"node\"after"
    );
}

#[test]
fn visible_regex_anchors_apply_to_rendered_lines_not_the_whole_document() {
    for pattern in [r"^--acls", r"lists$"] {
        let mut request = request(pattern);
        request.syntax = SearchSyntax::Regex;
        request.case = SearchCase::Sensitive;

        let result = search_query(&query(), &request).expect("search");

        assert_eq!(result.total, 1, "pattern {pattern:?}");
        assert_eq!(result.matches[0].outline.node.path(), "1/e1");
    }
}

#[test]
fn synthetic_visible_whitespace_occurrences_are_skipped_without_failing_the_query() {
    for pattern in [r"\n", r"\s", r"\s+", "[[:space:]]"] {
        let mut request = request(pattern);
        request.syntax = SearchSyntax::Regex;
        request.case = SearchCase::Sensitive;

        let result = search_query(&query(), &request).expect("valid whitespace search");
        assert!(
            result
                .matches
                .iter()
                .flat_map(|hit| &hit.occurrences)
                .all(|occurrence| !occurrence.line_ranges.is_empty()),
            "pattern {pattern:?} emitted an unpresentable occurrence"
        );
    }
}

#[test]
fn markdown_anchor_only_matches_do_not_become_phantom_results() {
    let mut request = request("a id=");
    request.scope = SearchScope::Markdown;
    request.case = SearchCase::Sensitive;

    let result = search_query(&query(), &request).expect("search internal anchor text");

    assert_eq!(result.total, 0);
    assert!(result.matches.is_empty());
}

#[test]
fn markdown_matches_crossing_an_anchor_expose_only_presented_text() {
    let markdown = render_addressable_markdown(&query()).into_text();
    let marker = "\"></a>`--acls";
    assert!(
        markdown.contains(marker),
        "fixture anchor shape changed:\n{markdown}"
    );
    let mut request = request(marker);
    request.scope = SearchScope::Markdown;
    request.case = SearchCase::Sensitive;

    let result = search_query(&query(), &request).expect("search across source-map anchor");
    let occurrence = &result.matches[0].occurrences[0];

    assert_eq!(occurrence.matched_text, "`--acls");
    assert!(!occurrence.line_ranges.is_empty());
    assert!(!result.matches[0].preview.contains("<a id="));
}

#[test]
fn visible_search_maps_padded_code_span_content_not_its_delimiters() {
    for value in ["`x", "x`", " x", "x ", "`x`"] {
        let mut query = query();
        let Block::DefinitionList { items, .. } =
            &mut query.document.as_mut().expect("manual").sections[0].blocks[0]
        else {
            panic!("fixture contains a definition list");
        };
        items[0].description = vec![Block::Paragraph {
            children: vec![Inline::Code {
                value: value.to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }];
        let markdown = render_addressable_markdown(&query).into_text();

        let result = search_query(&query, &request(value)).expect("search");
        let occurrence = &result.matches[0].occurrences[0];
        let start = usize::try_from(occurrence.markdown.start_byte).expect("small fixture");
        let end = usize::try_from(occurrence.markdown.end_byte).expect("small fixture");

        assert_eq!(&markdown[start..end], value, "code value {value:?}");
        assert_eq!(occurrence.line_ranges.len(), 1, "code value {value:?}");
        let line = &occurrence.line_ranges[0];
        let line_start = usize::try_from(line.start_byte).expect("small fixture");
        let line_end = usize::try_from(line.end_byte).expect("small fixture");
        assert_eq!(
            &result.matches[0].preview[line_start..line_end],
            value,
            "code value {value:?}"
        );
    }
}

#[test]
fn visible_search_maps_an_explicit_line_break_to_its_markdown_byte() {
    let mut query = query();
    let Block::DefinitionList { items, .. } =
        &mut query.document.as_mut().expect("manual").sections[0].blocks[0]
    else {
        panic!("fixture contains a definition list");
    };
    items[0].description = vec![Block::Paragraph {
        children: vec![
            Inline::Text {
                value: "alpha".to_owned(),
            },
            Inline::LineBreak,
            Inline::Text {
                value: "beta".to_owned(),
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    let markdown = render_addressable_markdown(&query).into_text();

    let result = search_query(&query, &request("alpha\n")).expect("search");
    let occurrence = &result.matches[0].occurrences[0];
    let start = usize::try_from(occurrence.markdown.start_byte).expect("small fixture");
    let end = usize::try_from(occurrence.markdown.end_byte).expect("small fixture");

    assert_eq!(&markdown[start..end], "alpha  \n");
}

#[test]
fn same_line_occurrences_form_one_paginated_search_result() {
    let mut query = query();
    let Block::DefinitionList { items, .. } =
        &mut query.document.as_mut().expect("manual").sections[0].blocks[0]
    else {
        panic!("fixture contains a definition list");
    };
    items[0].description = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "needle, then another needle on one line".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let mut request = request("needle");
    request.limit = 1;
    let result = search_query(&query, &request).expect("search");

    assert_eq!(result.total, 1);
    assert_eq!(result.returned, 1);
    assert_eq!(result.matches[0].occurrences.len(), 2);
    assert_eq!(
        result.matches[0].occurrences[0].markdown.start_line,
        result.matches[0].occurrences[1].markdown.start_line
    );
    assert!(!result.truncated);
}

#[test]
fn one_repetitive_line_has_bounded_occurrence_details() {
    let mut query = query();
    let Block::DefinitionList { items, .. } =
        &mut query.document.as_mut().expect("manual").sections[0].blocks[0]
    else {
        panic!("fixture contains a definition list");
    };
    let occurrence_count = MAX_OCCURRENCES_PER_MATCH + 7;
    items[0].description = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: vec!["needle"; occurrence_count].join(" "),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let result = search_query(&query, &request("needle")).expect("search");

    assert_eq!(result.total, 1);
    assert_eq!(
        result.matches[0].occurrence_count,
        u32::try_from(occurrence_count).expect("small fixture")
    );
    assert_eq!(
        result.matches[0].occurrences.len(),
        MAX_OCCURRENCES_PER_MATCH
    );
    assert!(result.matches[0].occurrences_truncated);
}

#[test]
fn semantic_entry_ownership_ends_before_a_following_section_paragraph() {
    let mut query = query();
    query.document.as_mut().expect("manual").sections[0]
        .blocks
        .push(Block::Paragraph {
            children: vec![Inline::Text {
                value: "General section tail".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        });

    let result = search_query(&query, &request("section tail")).expect("search");
    assert!(matches!(
        &result.matches[0].outline.node,
        mant_protocol::OutlineNodeReference::DocumentSection { path, .. } if path == "1"
    ));
}

#[test]
fn root_content_search_resolves_to_an_addressable_document_root() {
    let mut query = query();
    let document = query.document.as_mut().expect("document");
    document.source.format = SourceFormat::Markdown;
    document.blocks.push(Block::Paragraph {
        children: vec![Inline::Text {
            value: "Read the preface needle first.".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    });

    let result = search_query(&query, &request("preface needle")).expect("root search");

    assert_eq!(result.total, 1);
    assert!(matches!(
        &result.matches[0].outline.node,
        mant_protocol::OutlineNodeReference::DocumentRoot { path, id, .. }
            if path == "root" && id == "document-overview"
    ));
    assert!(result.matches[0].outline.ancestors.is_empty());
    assert!(result.matches[0].preview.contains("preface needle"));
}

#[test]
fn embedded_tldr_and_markdown_body_keep_distinct_search_owners() {
    let query = crate::query_markdown_text(
        "\
<!-- mant:tldr:start -->
# demo

> Quick needle.

- Run:

`demo quick-command`
<!-- mant:tldr:end -->

# Demo

Read the overview needle.

## Synopsis

Manual needle.
",
        Some("demo.md".to_owned()),
    )
    .expect("Markdown query");

    let quick = search_query(&query, &request("quick needle")).expect("tldr search");
    assert!(matches!(
        &quick.matches[0].outline.node,
        mant_protocol::OutlineNodeReference::Tldr { path, id, .. }
            if path == "0" && id == "tldr"
    ));

    let overview = search_query(&query, &request("overview needle")).expect("root search");
    assert!(matches!(
        &overview.matches[0].outline.node,
        mant_protocol::OutlineNodeReference::DocumentRoot { path, .. } if path == "root"
    ));

    let manual = search_query(&query, &request("manual needle")).expect("section search");
    assert!(matches!(
        &manual.matches[0].outline.node,
        mant_protocol::OutlineNodeReference::DocumentSection { path, id, .. }
            if path == "1" && id == "synopsis"
    ));
}

#[test]
fn regex_case_and_pagination_are_reported_without_losing_global_ordinals() {
    let mut request = request("ACLS|control");
    request.syntax = SearchSyntax::Regex;
    request.case = SearchCase::Insensitive;
    request.limit = 1;
    request.offset = 1;
    let result = search_query(&query(), &request).expect("search");

    assert_eq!(result.total, 2);
    assert_eq!(result.returned, 1);
    assert_eq!(result.matches[0].ordinal, 2);
    assert!(!result.truncated);
}

#[test]
fn regexes_that_match_empty_text_are_rejected() {
    for pattern in ["$", r"\b", r"\B", "a*"] {
        let mut request = request(pattern);
        request.syntax = SearchSyntax::Regex;
        let error = search_query(&query(), &request).expect_err("empty regex match");
        assert!(
            error.to_string().contains("must not match empty text"),
            "pattern {pattern:?}: {error}"
        );
    }
}

#[test]
fn search_results_never_cross_addressable_owner_boundaries() {
    let mut query = query();
    query
        .document
        .as_mut()
        .expect("document")
        .sections
        .push(Section {
            id: "next".into(),
            fragment_aliases: Vec::new(),
            heading: "NEXT".into(),
            spacing_before_lines: 0,
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: "Following owner".to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            children: Vec::new(),
            source: None,
        });
    let mut request = request(r"lists(?s:.*?)NEXT");
    request.syntax = SearchSyntax::Regex;
    request.scope = SearchScope::Markdown;
    request.case = SearchCase::Sensitive;

    let result = search_query(&query, &request).expect("bounded owner search");

    assert_eq!(result.total, 0);
}

#[test]
fn exclusive_newline_end_does_not_mark_the_following_context_line() {
    let mut query = query();
    query.document.as_mut().expect("document").sections[0]
        .blocks
        .push(Block::Paragraph {
            children: vec![
                Inline::Text {
                    value: "alpha".to_owned(),
                },
                Inline::LineBreak,
                Inline::Text {
                    value: "beta".to_owned(),
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        });
    let mut request = request("alpha  \n");
    request.scope = SearchScope::Markdown;
    request.case = SearchCase::Sensitive;
    let result = search_query(&query, &request).expect("newline search");
    let hit = &result.matches[0];
    let occurrence = &hit.occurrences[0];

    assert_eq!(occurrence.line_ranges.len(), 1);
    let following = hit
        .context
        .iter()
        .find(|line| line.line == occurrence.markdown.end_line)
        .expect("following context line");
    assert!(!following.matched);
}

#[test]
fn multibyte_match_ends_remain_valid_coordinate_boundaries() {
    let mut query = query();
    query.document.as_mut().expect("document").sections[0]
        .blocks
        .push(Block::Paragraph {
            children: vec![Inline::Text {
                value: "café — 日本".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        });
    let mut request = request("—");
    request.case = SearchCase::Sensitive;

    let result = search_query(&query, &request).expect("Unicode search");
    let occurrence = &result.matches[0].occurrences[0];

    assert_eq!(occurrence.matched_text, "—");
    assert_eq!(
        occurrence.markdown.end_column,
        occurrence.markdown.start_column + 1
    );
}

#[test]
fn byte_mode_regexes_are_rejected_before_matching_unicode_text() {
    let mut request = request("(?-u:.)");
    request.syntax = SearchSyntax::Regex;
    let error = search_query(&query(), &request).expect_err("byte-oriented regex");

    assert!(error.to_string().contains("UTF-8 character boundaries"));
}

#[test]
fn regex_syntax_errors_retain_their_actual_cause() {
    for (pattern, expected) in [
        ("(", "unclosed group"),
        ("a{2,1}", "invalid repetition count range"),
        ("[z-a]", "invalid character class range"),
    ] {
        let mut request = request(pattern);
        request.syntax = SearchSyntax::Regex;
        let error = validate_search_query(&request).expect_err("invalid regex");
        let message = error.to_string();
        assert!(message.contains(expected), "{pattern}: {message}");
        assert!(!message.contains("Unicode mode cannot be disabled"));
    }
}

#[test]
fn compiled_regex_programs_use_the_project_resource_budget() {
    let mut oversized = request("((a{100}){100}){100}");
    oversized.syntax = SearchSyntax::Regex;

    let error = validate_search_query(&oversized).expect_err("reject oversized regex program");

    assert_eq!(
        error,
        SearchError::InvalidPattern(
            "regular expression exceeds ManT's compiled-size limit".to_owned()
        )
    );
    let mut ordinary = request("needle|ordinary");
    ordinary.syntax = SearchSyntax::Regex;
    assert_eq!(validate_search_query(&ordinary), Ok(()));
}

#[test]
fn search_pattern_limit_counts_unicode_scalars() {
    let valid = request(&"界".repeat(MAX_SEARCH_PATTERN_CHARS));
    assert_eq!(validate_search_query(&valid), Ok(()));

    let request = request(&"界".repeat(MAX_SEARCH_PATTERN_CHARS + 1));
    assert_eq!(
        validate_search_query(&request),
        Err(SearchError::PatternTooLong)
    );
}
