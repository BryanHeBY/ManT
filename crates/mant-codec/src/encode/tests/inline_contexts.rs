//! Borrowed phrasing shares delimiters across roots and transparent links.

use super::*;

fn seam_atom(kind: u8, value: &str, uri: &str) -> Inline {
    let text = || Inline::Text {
        value: value.into(),
    };
    let strong = || Inline::Strong {
        children: vec![text()],
    };
    let emphasis = || Inline::Emphasis {
        children: vec![text()],
    };
    let code = || Inline::Code {
        value: value.into(),
    };
    match kind {
        0 => text(),
        1 => strong(),
        2 => emphasis(),
        3 => code(),
        5 => Inline::Strong {
            children: vec![emphasis()],
        },
        6 => Inline::Emphasis {
            children: vec![strong()],
        },
        4 | 7 | 8 => Inline::Link {
            target: mant_ir::LinkTarget::External { uri: uri.into() },
            title: None,
            children: vec![match kind {
                7 => strong(),
                8 => code(),
                _ => text(),
            }],
        },
        _ => panic!("unsupported public inline kind"),
    }
}

fn seam_cells(markdown: &str) -> Vec<(char, u8, Option<String>)> {
    let mut strong = 0u8;
    let mut emphasis = 0u8;
    let mut links = Vec::new();
    let mut cells = Vec::new();
    for event in Parser::new(markdown) {
        let code = u8::from(matches!(&event, Event::Code(_))) * 4;
        match event {
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong -= 1,
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis -= 1,
            Event::Start(Tag::Link { dest_url, .. }) => links.push(dest_url.into_string()),
            Event::End(TagEnd::Link) => {
                links.pop();
            }
            Event::Text(value) | Event::Code(value) => {
                let mask = u8::from(strong > 0) | (u8::from(emphasis > 0) * 2) | code;
                cells.extend(
                    value
                        .chars()
                        .map(|glyph| (glyph, mask, links.last().cloned())),
                );
            }
            Event::HardBreak => cells.push(('\n', 0, None)),
            _ => {}
        }
    }
    cells
}

#[test]
fn borrowed_joined_fragments_keep_exact_words_styles_and_link_owners() {
    // Constructed source-neutral inline contract: no native reachability is
    // inferred from the shape. Expectations name every accepted scalar and
    // style directly; Markdown must not invent delimiter text or a separator.
    let masks = [0, 1, 2, 4, 0, 3, 3, 1, 4];
    for head_kind in 0..9 {
        for body_kind in 0..9 {
            let head = [seam_atom(head_kind, "HEADX", "https://head.example")];
            let body = [seam_atom(body_kind, "BODY", "https://body.example")];
            let expected = [
                (head_kind, "HEADX", "https://head.example"),
                (body_kind, "BODY", "https://body.example"),
            ]
            .into_iter()
            .flat_map(|(kind, word, uri)| {
                let link = matches!(kind, 4 | 7 | 8).then(|| uri.to_owned());
                word.chars()
                    .map(move |glyph| (glyph, masks[usize::from(kind)], link.clone()))
            })
            .collect::<Vec<_>>();
            let markdown = super::inline::render_inline_segments(
                &[&head, &[], &body],
                MarkdownOptions::default(),
            );
            assert_eq!(
                seam_cells(&markdown),
                expected,
                "{head_kind}/{body_kind}: {markdown}"
            );
        }
    }
}

#[test]
fn joined_code_and_nested_fragments_share_backtick_and_recursive_style_context() {
    for (head, body, text) in [
        ("HEADX", "BODY", "HEADXBODY"),
        ("HEAD`", "`BODY", "HEAD``BODY"),
        ("", "BODY", "BODY"),
        ("HEADX", "", "HEADX"),
        ("", "", ""),
    ] {
        for kind in [3, 5, 6, 8] {
            let head = [seam_atom(kind, head, "https://head.example")];
            let body = [seam_atom(kind, body, "https://body.example")];
            let markdown =
                super::inline::render_inline_segments(&[&head, &body], MarkdownOptions::default());
            let actual = seam_cells(&markdown)
                .into_iter()
                .map(|(glyph, _, _)| glyph)
                .collect::<String>();
            assert_eq!(actual, text, "kind {kind}: {markdown}");
        }
    }
    let head = [Inline::Strong {
        children: vec![Inline::Emphasis {
            children: vec![Inline::Code {
                value: "HEADX".into(),
            }],
        }],
    }];
    let body = [Inline::Strong {
        children: vec![Inline::Emphasis {
            children: vec![Inline::Code {
                value: "BODY".into(),
            }],
        }],
    }];
    let markdown =
        super::inline::render_inline_segments(&[&head, &body], MarkdownOptions::default());
    assert_eq!(
        seam_cells(&markdown),
        "HEADXBODY"
            .chars()
            .map(|glyph| (glyph, 7, None))
            .collect::<Vec<_>>(),
        "{markdown}"
    );
}

#[test]
fn joined_fragments_keep_authored_inner_spaces_and_hard_rows() {
    let head = [Inline::Code {
        value: "HEADX\n".into(),
    }];
    let body = [Inline::Code {
        value: "BODY".into(),
    }];
    let markdown =
        super::inline::render_inline_segments(&[&head, &body], MarkdownOptions::default());
    let mut expected = "HEADX"
        .chars()
        .map(|glyph| (glyph, 4, None))
        .collect::<Vec<_>>();
    expected.push(('\n', 0, None));
    expected.extend("BODY".chars().map(|glyph| (glyph, 4, None)));
    assert_eq!(seam_cells(&markdown), expected, "{markdown}");
    let head = [Inline::Text {
        value: "HEADX ".into(),
    }];
    let body = [Inline::Strong {
        children: vec![Inline::Text {
            value: "BODY".into(),
        }],
    }];
    let markdown =
        super::inline::render_inline_segments(&[&head, &body], MarkdownOptions::default());
    let mut expected = "HEADX "
        .chars()
        .map(|glyph| (glyph, 0, None))
        .collect::<Vec<_>>();
    expected.extend("BODY".chars().map(|glyph| (glyph, 1, None)));
    assert_eq!(seam_cells(&markdown), expected, "{markdown}");
}

#[test]
fn empty_fragments_and_optional_anchors_do_not_break_code_delimiter_context() {
    for separator in [
        Inline::Text {
            value: String::new(),
        },
        Inline::Strong { children: vec![] },
        Inline::Emphasis {
            children: vec![Inline::Text {
                value: String::new(),
            }],
        },
        Inline::Code {
            value: String::new(),
        },
        Inline::anchor("seam-target"),
    ] {
        for preserve_anchors in [false, true] {
            let head = [
                Inline::Code {
                    value: "HEADX".into(),
                },
                separator.clone(),
            ];
            let body = [Inline::Code {
                value: "BODY".into(),
            }];
            let markdown = super::inline::render_inline_segments(
                &[&head, &body],
                MarkdownOptions {
                    preserve_anchors,
                    preserve_semantics: false,
                },
            );
            let expected = "HEADXBODY"
                .chars()
                .map(|glyph| (glyph, 4, None))
                .collect::<Vec<_>>();
            assert_eq!(
                seam_cells(&markdown),
                expected,
                "{separator:?}/{preserve_anchors}: {markdown}"
            );
        }
    }
}

#[test]
fn completed_root_strings_are_not_a_valid_joined_inline_encoding() {
    for kind in [1, 3] {
        let head = [seam_atom(kind, "HEADX", "https://head.example")];
        let body = [seam_atom(kind, "BODY", "https://body.example")];
        let split_encoding = format!(
            "{}{}",
            super::inline::render_inline(&head, MarkdownOptions::default()),
            super::inline::render_inline(&body, MarkdownOptions::default()),
        );
        let visible = seam_cells(&split_encoding)
            .into_iter()
            .map(|(glyph, _, _)| glyph)
            .collect::<String>();
        assert_ne!(
            visible, "HEADXBODY",
            "independent serialization must exercise the delimiter defect: {split_encoding}"
        );
    }
}

fn contextual_link(target_kind: u8, children: Vec<Inline>) -> Inline {
    let target = match target_kind {
        0 => mant_ir::LinkTarget::Manual {
            name: "printf".into(),
            manual_section: Some("3".into()),
        },
        1 => mant_ir::LinkTarget::Section { id: "body".into() },
        2 => mant_ir::LinkTarget::External {
            uri: "https://example.org/target".into(),
        },
        _ => panic!("unsupported link target policy"),
    };
    Inline::Link {
        target,
        title: Some("typed target".into()),
        children,
    }
}

fn contextual_destination(target_kind: u8, preserve_anchors: bool) -> Option<String> {
    match target_kind {
        0 => None,
        1 => preserve_anchors.then(|| "#body".to_owned()),
        2 => Some("https://example.org/target".to_owned()),
        _ => panic!("unsupported link target policy"),
    }
}

fn render_contextual_roots(
    head: &[Inline],
    body: &[Inline],
    options: MarkdownOptions,
    api: u8,
) -> String {
    match api {
        0 => super::inline::render_inline(
            &head.iter().chain(body).cloned().collect::<Vec<_>>(),
            options,
        ),
        1 => super::inline::render_inline_segments(&[head, body], options),
        2 => super::inline::render_inline_node_refs(
            &head.iter().chain(body).collect::<Vec<_>>(),
            options,
        ),
        _ => panic!("unsupported borrowed encoding API"),
    }
}

#[test]
fn omitted_link_wrappers_share_all_inline_encoding_contexts() {
    // Hand-constructed public IR: the typed target remains in the input.
    // Only wrapper presentation changes. Every expected glyph/style/target
    // is stated independently of the encoder's grouping decisions.
    let masks = [0, 1, 2, 4];
    for target_kind in 0..3 {
        for owner in 0..3 {
            for preserve_anchors in [false, true] {
                for head_kind in 0..4 {
                    for body_kind in 0..4 {
                        let head_atom = seam_atom(head_kind, "HEADX", "unused");
                        let body_atom = seam_atom(body_kind, "BODY", "unused");
                        let head_linked = owner != 1;
                        let body_linked = owner != 0;
                        let head = [if head_linked {
                            contextual_link(target_kind, vec![head_atom])
                        } else {
                            head_atom
                        }];
                        let body = [if body_linked {
                            contextual_link(target_kind, vec![body_atom])
                        } else {
                            body_atom
                        }];
                        let destination = contextual_destination(target_kind, preserve_anchors);
                        let expected = [
                            ("HEADX", head_kind, head_linked),
                            ("BODY", body_kind, body_linked),
                        ]
                        .into_iter()
                        .flat_map(|(word, kind, linked)| {
                            let target = linked.then(|| destination.clone()).flatten();
                            word.chars()
                                .map(move |glyph| (glyph, masks[usize::from(kind)], target.clone()))
                        })
                        .collect::<Vec<_>>();
                        for api in 0..3 {
                            let markdown = render_contextual_roots(
                                &head,
                                &body,
                                MarkdownOptions {
                                    preserve_anchors,
                                    preserve_semantics: false,
                                },
                                api,
                            );
                            assert_eq!(
                                seam_cells(&markdown),
                                expected,
                                "target={target_kind} owner={owner} anchors={preserve_anchors} styles={head_kind}/{body_kind} api={api}: {markdown}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn transparent_nested_labels_keep_anchors_backticks_and_executed_hard_rows() {
    for target_kind in [0, 1] {
        for preserve_anchors in [false, true] {
            let head = [Inline::Code {
                value: "HEAD`".into(),
            }];
            let body = [contextual_link(
                target_kind,
                vec![
                    Inline::Text {
                        value: String::new(),
                    },
                    Inline::Strong { children: vec![] },
                    Inline::anchor("label-anchor"),
                    contextual_link(
                        0,
                        vec![Inline::Code {
                            value: "`BODY\nTAIL".into(),
                        }],
                    ),
                ],
            )];
            let mut expected = "HEAD`"
                .chars()
                .map(|glyph| (glyph, 4, None))
                .collect::<Vec<_>>();
            let destination = contextual_destination(target_kind, preserve_anchors);
            expected.extend("`BODY".chars().map(|glyph| (glyph, 4, destination.clone())));
            expected.push(('\n', 0, None));
            expected.extend("TAIL".chars().map(|glyph| (glyph, 4, destination.clone())));
            for api in 0..3 {
                let markdown = render_contextual_roots(
                    &head,
                    &body,
                    MarkdownOptions {
                        preserve_anchors,
                        preserve_semantics: false,
                    },
                    api,
                );
                assert_eq!(
                    seam_cells(&markdown),
                    expected,
                    "{target_kind}/{preserve_anchors}/{api}: {markdown}"
                );
                assert_eq!(
                    markdown.matches("<a id=\"label-anchor\"").count(),
                    usize::from(preserve_anchors)
                );
            }
        }
    }
}

#[test]
fn empty_transparent_label_nodes_cannot_change_adjacent_style_context() {
    let masks = [0, 1, 2, 4];
    let empty_nodes = [
        Inline::Text {
            value: String::new(),
        },
        Inline::Strong { children: vec![] },
        Inline::Emphasis {
            children: vec![Inline::Text {
                value: String::new(),
            }],
        },
        Inline::Code {
            value: String::new(),
        },
        Inline::anchor("empty-label-anchor"),
    ];
    for target_kind in [0, 1] {
        for preserve_anchors in [false, true] {
            for empty in &empty_nodes {
                for head_kind in 0..4 {
                    for body_kind in 0..4 {
                        let head = [seam_atom(head_kind, "HEADX", "unused")];
                        let body = [contextual_link(
                            target_kind,
                            vec![empty.clone(), seam_atom(body_kind, "BODY", "unused")],
                        )];
                        let mut expected = "HEADX"
                            .chars()
                            .map(|glyph| (glyph, masks[usize::from(head_kind)], None))
                            .collect::<Vec<_>>();
                        let destination = contextual_destination(target_kind, preserve_anchors);
                        expected.extend("BODY".chars().map(|glyph| {
                            (glyph, masks[usize::from(body_kind)], destination.clone())
                        }));
                        for api in 0..3 {
                            let markdown = render_contextual_roots(
                                &head,
                                &body,
                                MarkdownOptions {
                                    preserve_anchors,
                                    preserve_semantics: false,
                                },
                                api,
                            );
                            assert_eq!(
                                seam_cells(&markdown),
                                expected,
                                "target={target_kind} anchors={preserve_anchors} empty={empty:?} styles={head_kind}/{body_kind} api={api}: {markdown}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn parsed_manual_and_section_labels_do_not_split_adjacent_code_spans() {
    for source in ["`HEADX`[`BODY`](man:printf(3))", "`HEADX`[`BODY`](#body)"] {
        let parsed = parse_content(source, None).unwrap();
        let Block::Paragraph { children, .. } = &parsed.document.as_ref().unwrap().blocks[0] else {
            panic!("expected an inline paragraph");
        };
        let markdown = super::inline::render_inline(children, MarkdownOptions::default());
        assert_eq!(
            seam_cells(&markdown),
            "HEADXBODY"
                .chars()
                .map(|glyph| (glyph, 4, None))
                .collect::<Vec<_>>(),
            "{source}: {markdown}"
        );
    }
}

#[test]
fn heading_manual_targets_keep_their_visible_link_wrapper() {
    let head = Inline::Code {
        value: "HEADX".into(),
    };
    let body = contextual_link(
        0,
        vec![Inline::Code {
            value: "BODY".into(),
        }],
    );
    let markdown = super::inline::render_heading_inline(&[head, body], MarkdownOptions::default());
    let mut expected = "HEADX"
        .chars()
        .map(|glyph| (glyph, 4, None))
        .collect::<Vec<_>>();
    expected.extend(
        "BODY"
            .chars()
            .map(|glyph| (glyph, 4, Some("man:printf(3)".to_owned()))),
    );
    assert_eq!(seam_cells(&markdown), expected, "{markdown}");
}
