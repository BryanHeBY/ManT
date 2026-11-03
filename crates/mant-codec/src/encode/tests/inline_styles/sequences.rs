//! Whole-stream readback, rather than isolated delimiter spelling assertions.

use super::*;

type Cell = (char, u8, Option<mant_ir::LinkTarget>);

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn atom(kind: u8, value: &str) -> Inline {
    let children = vec![text(value)];
    match kind {
        0 => text(value),
        1 => Inline::Strong { children },
        2 => Inline::Emphasis { children },
        3 => Inline::Code {
            value: value.into(),
        },
        4 => Inline::Strong {
            children: vec![Inline::Emphasis { children }],
        },
        5 => Inline::Emphasis {
            children: vec![Inline::Strong { children }],
        },
        _ => unreachable!(),
    }
}

fn cells(nodes: &[Inline], style: u8, owner: Option<&mant_ir::LinkTarget>) -> Vec<Cell> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            Inline::Text { value } => output.extend(
                value
                    .chars()
                    .map(|character| (character, style, owner.cloned())),
            ),
            Inline::Code { value } | Inline::Equation { value, .. } => output.extend(
                value
                    .chars()
                    .map(|character| (character, style | 4, owner.cloned())),
            ),
            Inline::Strong { children } => output.extend(cells(children, style | 1, owner)),
            Inline::Emphasis { children } => output.extend(cells(children, style | 2, owner)),
            Inline::Link {
                target, children, ..
            } => output.extend(cells(children, style, Some(target))),
            Inline::LineBreak {} => output.push(('\n', 0, None)),
            Inline::Anchor { .. } => {}
        }
    }
    output
}

fn readback(nodes: &[Inline], options: MarkdownOptions) -> (String, Vec<Cell>) {
    let markdown = super::super::inline::render_inline(nodes, options);
    let parsed = parse_content(&markdown, None).unwrap();
    let document = parsed.document.unwrap();
    let [Block::Paragraph { children, .. }] = document.blocks.as_slice() else {
        panic!("one original phrasing owner: {markdown:?}");
    };
    (markdown, cells(children, 0, None))
}

#[test]
fn complete_bidirectional_and_same_style_matrix_keeps_text_and_styles() {
    // Source-neutral IR: each scalar and style is specified independently
    // of the encoder, then recovered through the real Markdown -> IR path.
    let words = [
        "alpha",
        "reuse_level",
        "-fstack-reuse=",
        "$((",
        "))",
        "--arg=",
        ",tail",
        "prefix_",
        "_suffix",
        "a*b",
        "a_b",
        "a__b",
        "é中=",
        "参数_值",
        "a\u{301}",
        "😀",
        "!",
        "x-y",
    ];
    for left in words {
        for right in words {
            for a in 1..=2 {
                for b in 1..=2 {
                    let source = [atom(a, left), atom(b, right)];
                    let (markdown, actual) = readback(&source, MarkdownOptions::default());
                    assert_eq!(
                        actual,
                        cells(&source, 0, None),
                        "{a}/{b} {left:?}/{right:?}: {markdown}"
                    );
                }
            }
        }
    }
}

#[test]
fn complete_three_piece_nested_and_code_sequences_keep_style_owners() {
    for a in 0..6 {
        for b in 0..6 {
            for c in 0..6 {
                let source = [atom(a, "HEADX"), atom(b, "BODY"), atom(c, "TAIL")];
                let (markdown, actual) = readback(&source, MarkdownOptions::default());
                assert_eq!(actual, cells(&source, 0, None), "{a}/{b}/{c}: {markdown}");
            }
        }
    }
}

#[test]
fn author_edge_whitespace_is_not_a_block_origin_in_the_style_witness() {
    for edge in [" ", "\t"] {
        let source = [
            text(edge),
            atom(1, "--arg="),
            atom(2, "reuse_level"),
            text(edge),
        ];
        let (markdown, actual) = readback(&source, MarkdownOptions::default());
        assert_eq!(actual, cells(&source, 0, None), "{markdown}");
    }
}

#[test]
fn local_and_complete_style_witnesses_use_the_same_hard_row_syntax() {
    for value in ["Alpha", "中e\u{301}🦀", "<br> &amp; literal"] {
        for (leading, middle, trailing) in [(0, 2, 0), (1, 2, 1), (2, 2, 2)] {
            for kind in [1, 2, 4, 5] {
                let source = [atom(
                    kind,
                    &format!(
                        "{}{value}{}{value}{}",
                        "\n".repeat(leading),
                        "\n".repeat(middle),
                        "\n".repeat(trailing)
                    ),
                )];
                let (markdown, actual) = readback(&source, MarkdownOptions::default());
                let mut expected = cells(&source, 0, None);
                for (character, style, _) in &mut expected {
                    if *character == '\n' {
                        *style = 0;
                    }
                }
                assert_eq!(actual, expected, "{kind}: {markdown}");
            }
        }
    }
}

#[test]
fn transparent_links_empty_anchors_and_external_labels_share_the_sequence() {
    let manual = mant_ir::LinkTarget::Manual {
        name: "printf".into(),
        manual_section: Some("3".into()),
    };
    let external = mant_ir::LinkTarget::External {
        uri: "https://example.test/label".into(),
    };
    for reverse in [false, true] {
        for preserve_anchors in [false, true] {
            let (a, b) = if reverse { (2, 1) } else { (1, 2) };
            let source = [
                atom(a, "--arg="),
                Inline::anchor_with_aliases("seam", vec!["Exact.Seam".into()]),
                Inline::Strong {
                    children: vec![Inline::anchor("empty-style")],
                },
                Inline::Link {
                    target: manual.clone(),
                    title: None,
                    children: vec![atom(b, "reuse_level")],
                },
                Inline::Link {
                    target: external.clone(),
                    title: Some("one owner".into()),
                    children: vec![atom(a, "é中="), atom(b, "参数_值")],
                },
                Inline::Link {
                    target: external.clone(),
                    title: Some("second owner".into()),
                    children: vec![text("END")],
                },
            ];
            let mut expected = cells(&source, 0, None);
            for (_, _, owner) in &mut expected {
                if owner.as_ref() == Some(&manual) {
                    *owner = None;
                }
            }
            if preserve_anchors {
                // Addressable export emits raw HTML destinations. The
                // conservative Markdown reader deliberately retains other
                // HTML as visible source (markdown/inline.rs); that existing
                // policy is independent of delimiter pairing. Pin it exactly
                // while checking the standard parser's navigation below.
                let html = "<a id=\"seam\"></a><a id=\"Exact.Seam\"></a>";
                let empty = "<a id=\"empty-style\"></a>";
                // Adjacent Strong roots coalesce, including the zero-width
                // destination before the next Strong word. Standard HTML
                // still contributes no glyphs; the conservative reader's
                // literal HTML source inherits that surrounding style.
                expected.splice(
                    6..6,
                    html.chars().map(|character| (character, 0, None)).chain(
                        empty
                            .chars()
                            .map(|character| (character, u8::from(b == 1), None)),
                    ),
                );
            }
            let (markdown, actual) = readback(
                &source,
                MarkdownOptions {
                    preserve_anchors,
                    ..Default::default()
                },
            );
            assert_eq!(actual, expected, "{markdown}");
            let links = Parser::new(&markdown)
                .filter_map(|event| {
                    if let Event::Start(Tag::Link {
                        dest_url, title, ..
                    }) = event
                    {
                        Some((dest_url.into_string(), title.into_string()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                links,
                [
                    ("https://example.test/label".into(), "one owner".into()),
                    ("https://example.test/label".into(), "second owner".into())
                ]
            );
            for target in ["seam", "Exact.Seam", "empty-style"] {
                assert_eq!(
                    markdown.matches(&format!("id=\"{target}\"")).count(),
                    usize::from(preserve_anchors)
                );
            }
        }
    }
}

#[test]
fn style_seams_do_not_reopen_code_or_turn_authored_markers_into_text() {
    for value in ["`", "a``中", " a ", "a*b_c", "**", "__"] {
        let source = [
            atom(1, "--arg="),
            atom(2, "reuse_level"),
            atom(3, value),
            Inline::anchor("hidden"),
            atom(3, "TAIL`"),
            text("!"),
        ];
        let (markdown, actual) = readback(&source, MarkdownOptions::default());
        assert_eq!(actual, cells(&source, 0, None), "{markdown}");
        assert_eq!(
            Parser::new(&markdown)
                .filter(|event| matches!(event, Event::Code(_)))
                .count(),
            1,
            "one accepted adjacent code owner: {markdown}"
        );
    }
}
