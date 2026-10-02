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
fn independently_empty_link_roots_cannot_reintroduce_a_word_separator() {
    // Source-neutral roots: an independently trimmed whitespace-only root
    // contributes destinations, while an active link's actual label cells
    // remain content. A transparent wrapper cannot change that decision.
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
                        HeadBodyRelation::joined(DefinitionBodyAlignment::AfterTerm),
                    );
                    let before = original.clone();
                    let markdown = render_blocks_fragment(
                        std::slice::from_ref(&original),
                        MarkdownFragmentOptions { preserve_anchors },
                    )
                    .join("\n\n");
                    let expected: Vec<String> = if active {
                        if in_head {
                            vec!["HEAD\n BODY".into()]
                        } else {
                            vec!["HEAD ".into(), "BODY".into()]
                        }
                    } else {
                        vec!["HEADBODY".into()]
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
            HeadBodyRelation::joined(DefinitionBodyAlignment::AfterTerm),
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
        HeadBodyRelation::joined(DefinitionBodyAlignment::AfterTerm),
    );
    for preserve_anchors in [false, true] {
        let markdown = render_blocks_fragment(
            std::slice::from_ref(&block),
            MarkdownFragmentOptions { preserve_anchors },
        )
        .join("\n\n");
        assert_eq!(
            rows(&markdown),
            [if preserve_anchors {
                "HEAD<a id=\"destination\"></a>BODY"
            } else {
                "HEADBODY"
            }],
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
    relation: HeadBodyRelation,
) -> Block {
    Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
        items: vec![DefinitionItem {
            terms,
            description,
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                head_body_relation: relation,
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
    for relation in [
        HeadBodyRelation::Separate,
        HeadBodyRelation::joined(DefinitionBodyAlignment::AfterTerm),
        HeadBodyRelation::separated(DefinitionBodyAlignment::Indented),
    ] {
        for distance in [0, 1, 2] {
            for use_hint in [false, true] {
                for prefix in [
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
                ] {
                    let has_anchor = prefix
                        .iter()
                        .any(|node| matches!(node, Inline::Anchor { .. }));
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
                    let blocks = [definition(vec![vec![text("HEAD")]], body, relation)];
                    for preserve_anchors in [false, true] {
                        let markdown = render_blocks_fragment(
                            &blocks,
                            MarkdownFragmentOptions { preserve_anchors },
                        )
                        .join("\n\n");
                        let mut expected = if distance > 0 {
                            vec!["HEAD".to_owned(), "BODY".to_owned()]
                        } else if relation == HeadBodyRelation::Separate {
                            vec!["HEAD\nBODY".into()]
                        } else if relation.joins_without_separator() {
                            vec!["HEADBODY".into()]
                        } else {
                            vec!["HEAD BODY".into()]
                        };
                        if preserve_anchors && has_anchor {
                            // The ManT reader deliberately retains attributed
                            // HTML as literal source; browser destinations are
                            // checked separately from the portable readback.
                            for row in &mut expected {
                                *row = row.replace("BODY", "<a id=\"destination\"></a>BODY");
                            }
                        }
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
        let has_head = head
            .iter()
            .flatten()
            .any(|node| matches!(node,Inline::Text {value} if value=="HEAD"));
        for preserve_anchors in [false, true] {
            let blocks = [definition(
                head.clone(),
                vec![paragraph(vec![text("BODY")])],
                HeadBodyRelation::joined(DefinitionBodyAlignment::AfterTerm),
            )];
            let markdown =
                render_blocks_fragment(&blocks, MarkdownFragmentOptions { preserve_anchors })
                    .join("\n\n");
            let visible = Parser::new(&markdown)
                .filter_map(|event| match event {
                    Event::Text(value) => Some(value.into_string()),
                    _ => None,
                })
                .collect::<String>();
            assert_eq!(
                visible,
                if has_head { "HEADBODY" } else { "BODY" },
                "{markdown}"
            );
            assert!(
                !Parser::new(&markdown)
                    .any(|event| matches!(event, Event::HardBreak | Event::SoftBreak)),
                "zero-width term is not another row: {markdown}"
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
        HeadBodyRelation::joined(DefinitionBodyAlignment::AfterTerm),
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
