//! Exact hard rows across JSON, Markdown phrasing and source-safe HTML.
use super::*;
use mant_ir::LinkTarget;

#[path = "hard_rows/containers.rs"]
mod containers;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn query(children: Vec<Inline>) -> ResolvedContent {
    ResolvedContent {
        address: None,
        label: "hard rows".into(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![paragraph(children)],
            vec![],
        )])),
        tldr: None,
    }
}

fn phrasing(query: &ResolvedContent) -> &[Inline] {
    let Block::Paragraph { children, .. } = &query.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("hard rows must remain phrasing: {query:#?}")
    };
    children
}

fn styles(children: &[Inline], mask: u8, output: &mut Vec<(char, u8)>) {
    for child in children {
        match child {
            Inline::Text { value } | Inline::Code { value } => {
                let mask = mask | (u8::from(matches!(child, Inline::Code { .. })) * 4);
                output.extend(value.chars().map(|character| (character, mask)));
            }
            Inline::Strong { children } => styles(children, mask | 1, output),
            Inline::Emphasis { children } => styles(children, mask | 2, output),
            Inline::Link { children, .. } => styles(children, mask, output),
            Inline::LineBreak { .. } => output.push(('\n', 0)),
            Inline::Anchor { .. } => {}
            other @ Inline::Equation { .. } => panic!("uncovered style carrier: {other:?}"),
        }
    }
}

fn wrap(children: &[Inline], carrier: u8) -> Vec<Inline> {
    match carrier {
        0 => children.to_vec(),
        1 => vec![Inline::Strong {
            children: children.to_vec(),
        }],
        2 => vec![Inline::Emphasis {
            children: children.to_vec(),
        }],
        3 => vec![Inline::Strong {
            children: vec![Inline::Emphasis {
                children: children.to_vec(),
            }],
        }],
        4 => vec![Inline::Link {
            target: LinkTarget::External {
                uri: "https://example.org".into(),
            },
            title: Some("exact label".into()),
            children: children.to_vec(),
        }],
        _ => vec![Inline::Code {
            value: mant_ir::inline_plain_text(children),
        }],
    }
}

struct Links(Vec<(LinkTarget, Option<String>, String)>);
impl<'ir> Visit<'ir> for Links {
    fn visit_inline(&mut self, node: &'ir Inline) {
        if let Inline::Link {
            target,
            title,
            children,
        } = node
        {
            self.0.push((
                target.clone(),
                title.clone(),
                mant_ir::inline_plain_text(children),
            ));
        }
        walk_inline(self, node);
    }
}

fn assert_reader_projection(original: &ResolvedContent, carrier: u8) {
    let markdown = render_markdown_with_options(original, MarkdownOptions::default());
    let restored = parse_content(&markdown, Some("hard-rows.md".into())).unwrap();
    assert_eq!(
        mant_ir::inline_plain_text(phrasing(&restored)),
        mant_ir::inline_plain_text(phrasing(original)),
        "{markdown}"
    );
    let mut expected = Vec::new();
    let mut actual = Vec::new();
    styles(phrasing(original), 0, &mut expected);
    styles(phrasing(&restored), 0, &mut actual);
    // Markdown style delimiters deliberately leave boundary whitespace outside
    // a span. Exact content/rows protect those cells; compare font on glyphs.
    expected.retain(|(character, _)| !character.is_whitespace());
    actual.retain(|(character, _)| !character.is_whitespace());
    assert_eq!(actual, expected, "carrier={carrier}: {markdown}");
    if carrier == 4 {
        let mut links = Links(vec![]);
        links.visit_document(restored.document.as_ref().unwrap());
        assert_eq!(
            links.0,
            vec![(
                LinkTarget::External {
                    uri: "https://example.org".into()
                },
                Some("exact label".into()),
                mant_ir::inline_plain_text(phrasing(original))
            )],
            "{markdown}"
        );
    }
}

#[test]
fn every_edge_and_repeated_row_preserves_supported_styles_and_link_occurrences() {
    for value in [
        "Alpha",
        "中文",
        "e\u{301}",
        "🦀",
        r"\ &amp; <br> - #",
        "Alpha\u{a0}Beta",
        "<div>literal</div>",
        "1. item",
        "- item",
    ] {
        for (leading, middle, trailing) in [
            (0, 0, 0),
            (1, 0, 0),
            (2, 0, 0),
            (0, 1, 0),
            (0, 2, 0),
            (0, 0, 1),
            (0, 0, 2),
            (2, 2, 2),
        ] {
            let mut children = vec![Inline::line_break(); leading];
            children.push(text(value));
            children.extend(vec![Inline::line_break(); middle]);
            if middle > 0 {
                children.push(text(value));
            }
            children.extend(vec![Inline::line_break(); trailing]);
            for carrier in 0..6 {
                let original = query(wrap(&children, carrier));
                let document: Document = serde_json::from_str(
                    &serde_json::to_string(original.document.as_ref().unwrap()).unwrap(),
                )
                .unwrap();
                assert_eq!(original.document.as_ref().unwrap(), &document);
                assert_reader_projection(&original, carrier);
            }
        }
    }
}

#[test]
fn paragraphs_containing_only_hard_rows_do_not_need_visible_sentinels() {
    for count in [1, 2, 3] {
        for carrier in 0..6 {
            let content = query(wrap(&vec![Inline::line_break(); count], carrier));
            let markdown = render_markdown(&content);
            let restored = parse_content(&markdown, None).unwrap();
            assert_eq!(
                mant_ir::inline_plain_text(phrasing(&restored)),
                "\n".repeat(count),
                "{markdown}"
            );
            assert_eq!(mant_ir::inline_scalar_len(phrasing(&restored)), count);
        }
    }
}

#[test]
fn canonical_break_blocks_do_not_interpret_arbitrary_raw_html() {
    for body in [
        "<br>\n",
        "<div>literal</div>\n",
        "<script>alert(1)</script>\n",
        "<br class=x>\nAFTER\n",
        "<br />\n<div>mixed</div>\n",
        "<br />\n<script>alert(1)</script>\n",
        "<br />\n<br title=x>\n",
        "<br />\n<a href=\"javascript:x\">bad</a>\n",
    ] {
        let source = format!("# Tool\n\n## TEXT\n\n{body}");
        let restored = parse_content(&source, None).unwrap();
        let blocks = &restored.document.as_ref().unwrap().sections[0].blocks;
        assert!(
            matches!(&blocks[..], [Block::Unsupported { text, .. }] if text == body),
            "{source}: {blocks:#?}"
        );
    }
    let restored = parse_content("# Tool\n\n## TEXT\n\n\\<br /\\>\nAFTER\n", None).unwrap();
    assert_eq!(
        mant_ir::inline_plain_text(phrasing(&restored)),
        "<br /> AFTER"
    );
}

#[test]
fn canonical_rows_keep_original_byte_and_unicode_source_offsets_inside_lists() {
    for newline in ["\n", "\r\n", "\r"] {
        let source =
            "# Tool\n\n## TEXT\n\n- <br />\n  **中文e\u{301}**<br>\n  [END](https://example.org)\n"
                .replace('\n', newline);
        let restored = parse_content(&source, Some("rows.md".into())).unwrap();
        let Block::List { items, .. } = &restored.document.as_ref().unwrap().sections[0].blocks[0]
        else {
            panic!("list container")
        };
        let Block::Paragraph {
            children,
            source: Some(span),
            ..
        } = &items[0].blocks[0]
        else {
            panic!("list phrasing")
        };
        assert_eq!(mant_ir::inline_plain_text(children), "\n中文e\u{301}\nEND");
        assert_eq!((span.line, span.column), (5, 3));
        let range = span.byte_range.unwrap();
        let raw = &source[usize::try_from(range.start.get()).unwrap()
            ..usize::try_from(range.end.get()).unwrap()];
        assert!(
            raw.starts_with("<br />") && raw.contains("[END]"),
            "{raw:?}"
        );
    }
}
