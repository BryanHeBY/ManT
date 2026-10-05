//! Ordinary phrasing retains author rows without promoting layout hints to text.
use super::super::{
    Block, DefinitionItem, Event, Inline, LayoutHint, MarkdownFragmentOptions, Parser, Tag, TagEnd,
    parse_content, render_blocks_fragment,
};
use super::fixtures::{layout, visible};
use mant_ir::InlineContentRef;

#[test]
fn ordinary_rows_ignore_hints_through_styles_links_and_literal_newlines() {
    for target in [
        mant_ir::LinkTarget::External {
            uri: "https://example.org".into(),
        },
        mant_ir::LinkTarget::Manual {
            name: "example".into(),
            manual_section: Some("1".into()),
        },
    ] {
        let nodes = vec![
            Inline::Text {
                value: "A\n".into(),
            },
            Inline::Strong {
                children: vec![Inline::Emphasis {
                    children: vec![Inline::Code {
                        value: "B\n".into(),
                    }],
                }],
            },
            Inline::Link {
                target,
                title: None,
                children: vec![Inline::Text { value: "C".into() }],
            },
            Inline::line_break(),
            Inline::Text { value: "D".into() },
        ];
        let rows = layout(&[(0, 1), (1, 2), (2, -4), (3, 3)]);
        let markdown = super::super::render_inline_content_fragment(
            InlineContentRef {
                content: &nodes,
                layout: &rows,
            },
            MarkdownFragmentOptions::default(),
        );
        assert_eq!(visible(&markdown), "A\nB\nC\nD", "{markdown}");
        assert_eq!(mant_ir::inline_plain_text(&nodes), "A\nB\nC\nD");
        assert_eq!(
            visible(&super::super::render_inline_fragment(
                &nodes,
                MarkdownFragmentOptions::default()
            )),
            "A\nB\nC\nD"
        );
    }
}

#[test]
fn ordinary_author_row_edges_read_back_identically_with_and_without_hints() {
    for value in [" A  \n B ", "\tA\t\n\tB\t", " \n  ", " A\u{a0} \n B "] {
        let nodes = [Inline::Text {
            value: value.into(),
        }];
        let none = layout(&[]);
        let hints = layout(&[(0, 4), (1, 7)]);
        let outputs = [&none, &hints].map(|rows| {
            super::super::render_inline_content_fragment(
                InlineContentRef {
                    content: &nodes,
                    layout: rows,
                },
                MarkdownFragmentOptions::default(),
            )
        });
        assert_eq!(outputs[0], outputs[1]);
        assert_eq!(visible(&outputs[0]), value);
        let parsed = parse_content(&outputs[0], None).unwrap();
        let Block::Paragraph { children, .. } = &parsed.document.as_ref().unwrap().blocks[0] else {
            panic!("ordinary readback")
        };
        assert_eq!(mant_ir::inline_plain_text(children), value);
        assert!(!outputs[0].contains("&#160;"));
    }
}

#[test]
fn ordinary_link_label_keeps_author_nbsp_and_one_wrapper_without_generated_cells() {
    let nodes = [Inline::Link {
        target: mant_ir::LinkTarget::Section {
            id: "destination".into(),
        },
        title: None,
        children: vec![Inline::Strong {
            children: vec![Inline::Text {
                value: "A\nB\u{a0}C".into(),
            }],
        }],
    }];
    let rows = layout(&[(0, 1), (1, 2)]);
    let markdown = super::super::render_inline_content_fragment(
        InlineContentRef {
            content: &nodes,
            layout: &rows,
        },
        MarkdownFragmentOptions {
            preserve_anchors: true,
        },
    );
    assert_eq!(markdown, "[**A  \nB\u{a0}C**](#destination)");
    let mut in_link = false;
    let mut in_strong = false;
    let mut labels = Vec::new();
    let mut layout_cells = 0;
    for event in Parser::new(&markdown) {
        match event {
            Event::Start(Tag::Link { .. }) => in_link = true,
            Event::End(TagEnd::Link) => in_link = false,
            Event::Start(Tag::Strong) => in_strong = true,
            Event::End(TagEnd::Strong) => in_strong = false,
            Event::Text(value) if in_link => {
                assert!(in_strong);
                labels.push(value.into_string());
            }
            Event::Text(value) => {
                assert!(!in_strong);
                layout_cells += value
                    .chars()
                    .filter(|character| *character == '\u{a0}')
                    .count();
            }
            _ => {}
        }
    }
    assert_eq!(labels, ["A", "B\u{a0}C"]);
    assert_eq!(layout_cells, 0);
    assert_eq!(mant_ir::inline_plain_text(&nodes), "A\nB\u{a0}C");
}

#[test]
fn hints_on_empty_or_open_positions_do_not_create_markdown_text() {
    for (nodes, row, expected) in [
        (vec![], 0, ""),
        (vec![Inline::anchor("only-anchor")], 0, ""),
        (vec![Inline::line_break()], 1, "<br />\n"),
        (
            vec![Inline::Code {
                value: "A\n".into(),
            }],
            1,
            "`A`<br>\n",
        ),
    ] {
        let rows = layout(&[(row, 8)]);
        assert_eq!(
            super::super::render_inline_content_fragment(
                InlineContentRef {
                    content: &nodes,
                    layout: &rows
                },
                MarkdownFragmentOptions::default(),
            ),
            expected
        );
    }
    let rows = layout(&[(0, 2), (1, 4), (2, 6)]);
    let literal = Block::Preformatted {
        children: vec![Inline::Text {
            value: "A\n\n".into(),
        }],
        inline_layout: rows,
        language: None,
        layout: LayoutHint::default(),
        source: None,
    };
    assert_eq!(
        render_blocks_fragment(&[literal], MarkdownFragmentOptions::default()),
        ["```\n  A\n\n\n```"]
    );
}

#[test]
fn joined_owner_layouts_preserve_one_code_context_and_no_body_word_gap() {
    let term = mant_ir::DefinitionTerm {
        content: vec![Inline::Code { value: "`".into() }, Inline::anchor("seam")],
        inline_layout: layout(&[(0, 1)]),
    };
    let body = Block::Paragraph {
        children: vec![Inline::Code {
            value: "BODY\nNEXT".into(),
        }],
        inline_layout: layout(&[(0, 99), (1, 2)]),
        layout: LayoutHint::default(),
        source: None,
    };
    let block = Block::DefinitionList {
        items: vec![DefinitionItem {
            head_body_relation: mant_ir::HeadBodyRelation::joined(),
            terms: vec![term],
            description: vec![body],
            entry: None,
            layout: mant_ir::DefinitionLayout {
                body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                ..Default::default()
            },
            source: None,
        }],
        compact: true,
        declaration_groups: vec![],
        layout: LayoutHint::default(),
        source: None,
    };
    let markdown =
        render_blocks_fragment(&[block], MarkdownFragmentOptions::default()).join("\n\n");
    assert_eq!(visible(&markdown), "`BODY\nNEXT", "{markdown}");
    assert!(!markdown.contains(&"&#160;".repeat(99)));
}
