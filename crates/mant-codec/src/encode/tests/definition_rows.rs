//! Executed row boundaries and destination roots survive output ownership seams.

use std::{borrow::Cow, cell::RefCell};

use super::*;
use mant_ir::{DefinitionBodyAlignment, HeadBodyRelation};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn spaced_link(kind: u8, children: Vec<Inline>) -> Inline {
    let target = match kind {
        0 => mant_ir::LinkTarget::Manual {
            name: "printf".into(),
            manual_section: Some("3".into()),
        },
        1 => mant_ir::LinkTarget::Section {
            id: "target".into(),
        },
        2 => mant_ir::LinkTarget::External {
            uri: "https://example.org".into(),
        },
        _ => unreachable!(),
    };
    Inline::Link {
        target,
        title: None,
        children,
    }
}

#[test]
fn independent_link_roots_preserve_author_spaces_under_each_target_policy() {
    // Source-neutral roots retain authored ASCII label cells even when a
    // target policy omits its wrapper. Independent roots keep their authored
    // block/term boundaries instead of becoming destination-only metadata.
    for kind in 0..3 {
        for preserve_anchors in [false, true] {
            let active = kind == 2 || kind == 1 && preserve_anchors;
            for label in [
                vec![text(" ")],
                vec![Inline::Strong {
                    children: vec![text(" ")],
                }],
                vec![Inline::Emphasis {
                    children: vec![text(" ")],
                }],
            ] {
                for in_head in [false, true] {
                    let link = spaced_link(kind, label.clone());
                    let (terms, body) = if in_head {
                        (
                            vec![vec![text("HEAD")], vec![link]],
                            vec![paragraph(vec![text("BODY")])],
                        )
                    } else {
                        (
                            vec![vec![text("HEAD")]],
                            vec![paragraph(vec![link]), paragraph(vec![text("BODY")])],
                        )
                    };
                    let original = definition(
                        terms,
                        body,
                        (
                            HeadBodyRelation::joined(),
                            DefinitionBodyAlignment::AfterTerm,
                        ),
                    );
                    let before = original.clone();
                    let markdown = render_blocks_fragment(
                        std::slice::from_ref(&original),
                        MarkdownFragmentOptions { preserve_anchors },
                    )
                    .join("\n\n");
                    let expected: Vec<String> = if in_head {
                        vec!["HEAD\n BODY".into()]
                    } else {
                        vec!["HEAD ".into(), "BODY".into()]
                    };
                    assert_eq!(
                        rows(&markdown),
                        expected,
                        "{kind}/{preserve_anchors}/{in_head}: {markdown}"
                    );
                    assert_eq!(original, before, "typed destinations remain in original IR");
                    let link_count = Parser::new(&markdown)
                        .filter(|event| matches!(event, Event::Start(Tag::Link { .. })))
                        .count();
                    assert_eq!(link_count, usize::from(active));
                }
            }
            // Inside one real root, ASCII label spacing remains authored
            // content even when this particular wrapper is omitted.
            let block = paragraph(vec![
                text("HEAD"),
                spaced_link(kind, vec![text(" ")]),
                text("BODY"),
            ]);
            let markdown =
                render_blocks_fragment(&[block], MarkdownFragmentOptions { preserve_anchors })
                    .join("\n\n");
            let parsed = crate::parse_markdown(&markdown, None).unwrap();
            let Block::Paragraph { children, .. } = &parsed.document.blocks[0] else {
                panic!("{markdown}");
            };
            assert_eq!(
                mant_ir::inline_plain_text(children),
                "HEAD BODY",
                "{markdown}"
            );
        }
    }
}

#[test]
fn transparent_root_selection_keeps_fixed_code_spacing_and_nested_destinations() {
    for label in [
        vec![text("\u{a0}")],
        vec![Inline::Code { value: " ".into() }],
    ] {
        let block = definition(
            vec![vec![text("HEAD")]],
            vec![
                paragraph(vec![spaced_link(0, label)]),
                paragraph(vec![text("BODY")]),
            ],
            (
                HeadBodyRelation::joined(),
                DefinitionBodyAlignment::AfterTerm,
            ),
        );
        let markdown =
            render_blocks_fragment(&[block], MarkdownFragmentOptions::default()).join("\n\n");
        let rows = rows(&markdown);
        assert_eq!(rows.len(), 2, "{markdown}");
        assert!(
            matches!(rows[0].as_str(), "HEAD " | "HEAD\u{a0}"),
            "{markdown}"
        );
        assert_eq!(rows[1], "BODY");
    }
    let block = definition(
        vec![vec![text("HEAD")]],
        vec![
            paragraph(vec![spaced_link(
                0,
                vec![text(" "), Inline::anchor("destination")],
            )]),
            paragraph(vec![text("BODY")]),
        ],
        (
            HeadBodyRelation::joined(),
            DefinitionBodyAlignment::AfterTerm,
        ),
    );
    for preserve_anchors in [false, true] {
        let markdown = render_blocks_fragment(
            std::slice::from_ref(&block),
            MarkdownFragmentOptions { preserve_anchors },
        )
        .join("\n\n");
        assert_eq!(
            rows(&markdown),
            [
                if preserve_anchors {
                    "HEAD <a id=\"destination\"></a>"
                } else {
                    "HEAD "
                },
                "BODY"
            ],
            "{markdown}"
        );
        assert_eq!(
            markdown.matches("<a id=\"destination\"").count(),
            usize::from(preserve_anchors)
        );
    }
}

fn definition(
    terms: Vec<Vec<Inline>>,
    description: Vec<Block>,
    (relation, body_alignment): (HeadBodyRelation, DefinitionBodyAlignment),
) -> Block {
    Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
        items: vec![DefinitionItem {
            head_body_relation: relation,
            terms: terms.into_iter().map(Into::into).collect(),
            description,
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                body_alignment,
                ..Default::default()
            },
        }],
    }
}

fn rows(markdown: &str) -> Vec<String> {
    let parsed = crate::parse_markdown(markdown, None).unwrap();
    let Block::List { items, .. } = &parsed.document.blocks[0] else {
        panic!("one list: {markdown}");
    };
    assert_eq!(items.len(), 1);
    items[0]
        .blocks
        .iter()
        .map(|block| {
            let Block::Paragraph { children, .. } = block else {
                panic!("prose: {markdown}");
            };
            mant_ir::inline_plain_text(children)
        })
        .collect::<Vec<_>>()
}

#[test]
fn leading_space_and_zero_output_roots_keep_effective_body_boundaries() {
    for (relation, body_alignment) in [
        (
            HeadBodyRelation::Separate,
            DefinitionBodyAlignment::Indented,
        ),
        (
            HeadBodyRelation::joined(),
            DefinitionBodyAlignment::AfterTerm,
        ),
        (
            HeadBodyRelation::separated(),
            DefinitionBodyAlignment::Indented,
        ),
    ] {
        for distance in [0, 1, 2] {
            for use_hint in [false, true] {
                for prefix in author_prefix_roots() {
                    let has_anchor = prefix
                        .iter()
                        .any(|node| matches!(node, Inline::Anchor { .. }));
                    let prefix_text = mant_ir::inline_plain_text(&prefix);
                    let mut body = vec![paragraph(prefix)];
                    let mut prose = paragraph(vec![text("BODY")]);
                    if use_hint {
                        let Block::Paragraph { layout, .. } = &mut prose else {
                            unreachable!();
                        };
                        layout.spacing_before_lines = distance;
                    } else {
                        body.push(Block::VerticalSpace {
                            lines: distance,
                            source: None,
                        });
                    }
                    body.push(prose);
                    let blocks = [definition(
                        vec![vec![text("HEAD")]],
                        body,
                        (relation, body_alignment),
                    )];
                    for preserve_anchors in [false, true] {
                        let markdown = render_blocks_fragment(
                            &blocks,
                            MarkdownFragmentOptions { preserve_anchors },
                        )
                        .join("\n\n");
                        let anchor = if preserve_anchors && has_anchor {
                            "<a id=\"destination\"></a>"
                        } else {
                            ""
                        };
                        let expected = if prefix_text.is_empty() {
                            let mut expected = expected_head_body_rows(distance, relation);
                            for row in &mut expected {
                                *row = row.replace("BODY", &format!("{anchor}BODY"));
                            }
                            expected
                        } else {
                            expected_author_prefix_rows(relation, &prefix_text, anchor)
                        };
                        assert_eq!(
                            rows(&markdown),
                            expected,
                            "{relation:?}/{distance}/{use_hint}/{preserve_anchors}: {markdown}"
                        );
                        if preserve_anchors && has_anchor {
                            assert!(markdown.contains("id=\"destination\""));
                        }
                    }
                }
            }
        }
    }
}

fn author_prefix_roots() -> Vec<Vec<Inline>> {
    vec![
        vec![],
        vec![text("")],
        vec![text(" \t ")],
        vec![Inline::Strong {
            children: vec![text(" \t ")],
        }],
        vec![Inline::Anchor {
            id: "destination".into(),
            fragment_aliases: vec![],
            owner_source: None,
        }],
        vec![
            Inline::Anchor {
                id: "destination".into(),
                fragment_aliases: vec![],
                owner_source: None,
            },
            text(" \t "),
        ],
        vec![
            Inline::Anchor {
                id: "destination".into(),
                fragment_aliases: vec![],
                owner_source: None,
            },
            Inline::Strong {
                children: vec![text(" \t ")],
            },
        ],
    ]
}

fn expected_author_prefix_rows(
    relation: HeadBodyRelation,
    prefix: &str,
    anchor: &str,
) -> Vec<String> {
    let separator = if relation == HeadBodyRelation::Separate {
        "\n"
    } else if relation.joins_without_separator() {
        ""
    } else {
        " "
    };
    vec![format!("HEAD{separator}{anchor}{prefix}"), "BODY".into()]
}

fn expected_head_body_rows(distance: u16, relation: HeadBodyRelation) -> Vec<String> {
    if distance > 0 {
        vec!["HEAD".into(), "BODY".into()]
    } else if relation == HeadBodyRelation::Separate {
        vec!["HEAD\nBODY".into()]
    } else if relation.joins_without_separator() {
        vec!["HEADBODY".into()]
    } else {
        vec!["HEAD BODY".into()]
    }
}

#[test]
fn destinations_and_zero_output_head_roots_do_not_fabricate_term_rows() {
    let anchor = || Inline::Anchor {
        id: "head-destination".into(),
        fragment_aliases: vec![],
        owner_source: None,
    };
    for head in [
        vec![vec![text("HEAD")], vec![anchor()]],
        vec![vec![anchor()], vec![text("HEAD")]],
        vec![vec![text("HEAD")], vec![anchor(), text(" \t ")]],
        vec![vec![text("HEAD")], vec![text(" \t ")]],
        vec![vec![anchor()]],
        vec![vec![text(" \t ")]],
    ] {
        let authored_terms = head
            .iter()
            .map(|nodes| mant_ir::inline_plain_text(nodes))
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        let expected = format!("{}BODY", authored_terms.join("\n"));
        for preserve_anchors in [false, true] {
            let blocks = [definition(
                head.clone(),
                vec![paragraph(vec![text("BODY")])],
                (
                    HeadBodyRelation::joined(),
                    DefinitionBodyAlignment::AfterTerm,
                ),
            )];
            let markdown =
                render_blocks_fragment(&blocks, MarkdownFragmentOptions { preserve_anchors })
                    .join("\n\n");
            let visible = Parser::new(&markdown)
                .filter_map(|event| match event {
                    Event::Text(value) => Some(value.into_string()),
                    Event::HardBreak | Event::SoftBreak => Some("\n".into()),
                    _ => None,
                })
                .collect::<String>();
            assert_eq!(visible, expected, "{markdown}");
            assert_eq!(
                Parser::new(&markdown)
                    .filter(|event| matches!(event, Event::HardBreak | Event::SoftBreak))
                    .count(),
                authored_terms.len().saturating_sub(1),
                "only authored term roots own rows: {markdown}"
            );
            if preserve_anchors
                && head
                    .iter()
                    .flatten()
                    .any(|node| matches!(node, Inline::Anchor { .. }))
            {
                assert!(markdown.contains("id=\"head-destination\""));
            }
        }
    }
}

#[test]
fn joined_last_term_uses_original_roots_once_and_keeps_prior_hard_rows() {
    struct Roots(RefCell<Vec<usize>>);
    impl super::super::MarkdownInlineProjection for Roots {
        fn project<'a>(&self, nodes: &'a [Inline]) -> Cow<'a, [Inline]> {
            self.0.borrow_mut().push(nodes.as_ptr() as usize);
            Cow::Borrowed(nodes)
        }
    }
    let strong = |value| Inline::Strong {
        children: vec![text(value)],
    };
    let blocks = [definition(
        vec![vec![strong("FIRST")], vec![strong("HEAD")]],
        vec![
            paragraph(vec![Inline::Anchor {
                id: "destination".into(),
                fragment_aliases: vec![],
                owner_source: None,
            }]),
            paragraph(vec![strong("BODY")]),
            paragraph(vec![text("TAIL")]),
        ],
        (
            HeadBodyRelation::joined(),
            DefinitionBodyAlignment::AfterTerm,
        ),
    )];
    for preserve_anchors in [false, true] {
        let roots = Roots(RefCell::new(Vec::new()));
        let markdown = render_located_blocks_fragment(
            &blocks,
            MarkdownFragmentOptions { preserve_anchors },
            Some(&roots),
        )
        .join("\n\n");
        let head = if preserve_anchors {
            "FIRST\nHEAD<a id=\"destination\"></a>BODY"
        } else {
            "FIRST\nHEADBODY"
        };
        assert_eq!(rows(&markdown), [head, "TAIL"], "{markdown}");
        assert_eq!(roots.0.borrow().len(), 5);
        assert_eq!(
            roots
                .0
                .borrow()
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            5,
            "no assembled decoration root or duplicate projection"
        );
        let mut depth = 0;
        let mut glyphs = Vec::new();
        for event in Parser::new(&markdown) {
            match event {
                Event::Start(Tag::Strong) => depth += 1,
                Event::End(TagEnd::Strong) => depth -= 1,
                Event::Text(value) => glyphs.extend(value.chars().map(|c| (c, depth > 0))),
                _ => {}
            }
        }
        let expected = "FIRSTHEADBODY"
            .chars()
            .map(|c| (c, true))
            .chain("TAIL".chars().map(|c| (c, false)))
            .collect::<Vec<_>>();
        assert_eq!(glyphs, expected, "{markdown}");
    }
}
