//! Link presentation policy cannot split the surrounding phrasing context.

use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
use mant_ir::{Block, Inline, LinkTarget, ResolvedContent};
use mant_protocol::{QueryBundle, SearchCase, SearchQuery, SearchScope, SearchSyntax};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cell {
    glyph: char,
    style: u8,
    target: Option<LinkTarget>,
}

fn phrasing(content: &ResolvedContent) -> &[Inline] {
    content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "Payload")
        .unwrap()
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. }
                if mant_ir::inline_plain_text(children).contains("HEADX") =>
            {
                Some(children.as_slice())
            }
            _ => None,
        })
        .unwrap()
}

fn cells(nodes: &[Inline], style: u8, target: Option<&LinkTarget>, output: &mut Vec<Cell>) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                let style = style | (u8::from(matches!(node, Inline::Code { .. })) * 4);
                output.extend(value.chars().map(|glyph| Cell {
                    glyph,
                    style,
                    target: target.cloned(),
                }));
            }
            Inline::Strong { children } => cells(children, style | 1, target, output),
            Inline::Emphasis { children } => cells(children, style | 2, target, output),
            Inline::Link {
                target, children, ..
            } => cells(children, style, Some(target), output),
            Inline::LineBreak { .. } => output.push(Cell {
                glyph: '\n',
                style: 0,
                target: None,
            }),
            Inline::Anchor { .. } => {}
            other @ Inline::Equation { .. } => panic!("unexpected phrase {other:?}"),
        }
    }
}

fn expected(head: u8, body: u8, target: &LinkTarget, placement: u8, active: bool) -> Vec<Cell> {
    [
        ("HEADX", head, placement != 1),
        ("BODY", body, placement != 0),
    ]
    .into_iter()
    .flat_map(|(word, style, linked)| {
        word.chars().map(move |glyph| Cell {
            glyph,
            style: [0, 1, 2, 4][usize::from(style)],
            target: (active && linked).then(|| target.clone()),
        })
    })
    .collect()
}

fn target(kind: u8) -> LinkTarget {
    match kind {
        0 => LinkTarget::Manual {
            name: "printf".into(),
            manual_section: Some("3".into()),
        },
        1 => LinkTarget::Section {
            id: "target".into(),
        },
        2 => LinkTarget::External {
            uri: "https://example.org/target".into(),
        },
        _ => panic!("target kind"),
    }
}

fn operand(word: &str, style: u8, target: &LinkTarget, linked: bool) -> String {
    let label = match style {
        0 => word.to_owned(),
        1 => format!("**{word}**"),
        2 => format!("*{word}*"),
        3 => format!("`{word}`"),
        _ => panic!("style"),
    };
    if linked {
        format!("[{label}]({} \"typed target\")", target.to_uri().unwrap())
    } else {
        label
    }
}

fn round_trip(content: &ResolvedContent) -> ResolvedContent {
    let wire = serde_json::to_string(&QueryBundle::from(content)).unwrap();
    let restored = serde_json::from_str::<QueryBundle>(&wire).unwrap().into();
    assert_eq!(&restored, content);
    restored
}

fn assert_queries(content: &ResolvedContent) {
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    for (word, present) in [
        ("HEADXBODY", true),
        ("HEADX", true),
        ("BODY", true),
        ("HEADX``BODY", false),
        ("HEADX****BODY", false),
        ("HEADX**BODY", false),
    ] {
        let result = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: word.into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope: SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 100,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(result.total, u32::from(present), "{word}");
        for hit in result.matches {
            assert_ne!(hit.occurrences, []);
            for occurrence in hit.occurrences {
                assert_eq!(occurrence.matched_text, word);
                let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                    ..usize::try_from(occurrence.markdown.end_byte).unwrap();
                let bytes = &artifact.text()[range];
                // A phrase spanning active links/styles includes their real
                // source delimiters. A single label has an exact scalar slice.
                if word == "HEADXBODY" {
                    assert!(bytes.contains("HEADX") && bytes.contains("BODY"));
                } else {
                    assert_eq!(bytes, word);
                }
            }
        }
    }
}

#[test]
fn actual_markdown_links_preserve_phrasing_styles_and_typed_target_policies() {
    let mut count = 0;
    for kind in 0..3 {
        for placement in 0..3 {
            for head in 0..4 {
                for body in 0..4 {
                    count += 1;
                    let target = target(kind);
                    let first = operand("HEADX", head, &target, placement != 1);
                    let second = operand("BODY", body, &target, placement != 0);
                    let source = format!(
                        "# Seam\n\n## Payload\n\n{first}{second}\n\n## Target\n\nDestination.\n"
                    );
                    let original = mant_loader::load_markdown_text(&source, None).unwrap();
                    let decoded = round_trip(&original);
                    let mut actual = Vec::new();
                    cells(phrasing(&decoded), 0, None, &mut actual);
                    assert_eq!(actual, expected(head, body, &target, placement, true));
                    assert_queries(&decoded);
                    for preserve_anchors in [false, true] {
                        let markdown = render_markdown_with_options(
                            &decoded,
                            MarkdownOptions {
                                preserve_anchors,
                                preserve_semantics: false,
                            },
                        );
                        // Preserved raw destinations outside this paragraph
                        // follow the reader's existing literal HTML policy.
                        let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
                        let mut actual = Vec::new();
                        cells(phrasing(&imported), 0, None, &mut actual);
                        let active = kind == 2 || kind == 1 && preserve_anchors;
                        assert_eq!(
                            actual,
                            expected(head, body, &target, placement, active),
                            "{kind}/{placement}/{head}/{body}/{preserve_anchors}: {markdown}"
                        );
                        assert_queries(&imported);
                    }
                    assert_eq!(original, decoded, "export never mutates source Link roots");
                }
            }
        }
    }
    assert_eq!(count, 144);
}
