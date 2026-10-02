use super::*;

#[test]
fn protects_paragraph_lines_from_accidental_block_syntax() {
    let query = ResolvedContent {
        address: None,
        label: "syntax".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![paragraph(vec![
                Inline::Text {
                    value: "- not a list".to_owned(),
                },
                Inline::line_break(),
                Inline::Text {
                    value: "1. not an ordered list".to_owned(),
                },
                Inline::line_break(),
                Inline::Text {
                    value: "# not a heading".to_owned(),
                },
                Inline::line_break(),
                Inline::Text {
                    value: "-".to_owned(),
                },
                Inline::line_break(),
                Inline::Text {
                    value: "===".to_owned(),
                },
                Inline::line_break(),
                Inline::Text {
                    value: "```".to_owned(),
                },
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains(
            "\\- not a list  \n1\\. not an ordered list  \n\\# not a heading  \n\\-  \n\\===  \n\\`\\`\\`"
        ),
        "{markdown}"
    );
    let headings = Parser::new(&markdown)
        .filter(|event| matches!(event, Event::Start(Tag::Heading { .. })))
        .count();
    assert_eq!(headings, 2, "only the document and TEXT headings may exist");
    assert!(!Parser::new(&markdown).any(|event| matches!(event, Event::Start(Tag::CodeBlock(_)))));
}

#[test]
fn preserves_leading_consecutive_and_trailing_hard_breaks() {
    let query = ResolvedContent {
        address: None,
        label: "breaks".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![paragraph(vec![
                Inline::line_break(),
                Inline::Text {
                    value: "before".to_owned(),
                },
                Inline::line_break(),
                Inline::line_break(),
                Inline::Text {
                    value: "after".to_owned(),
                },
                Inline::line_break(),
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains("<br />\nbefore<br>\n<br>\nafter<br>"),
        "{markdown}"
    );
    let decoded = parse_content(&markdown, None).unwrap();
    let Block::Paragraph { children, .. } =
        &decoded.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("exported hard rows must read back as a paragraph")
    };
    assert_eq!(mant_ir::inline_plain_text(children), "\nbefore\n\nafter\n");
}

#[test]
fn preserves_literal_html_entity_spellings_across_commonmark() {
    let query = ResolvedContent {
        address: None,
        label: "entities".to_owned(),
        document: Some(manual(vec![section(
            "ENTITY TEXT",
            vec![paragraph(vec![Inline::Text {
                value: "literal a & b; spellings &amp;, &pound;, &#163;, and &notreal;".to_owned(),
            }])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("a &amp; b"), "{markdown}");
    assert!(markdown.contains("&amp;amp;"), "{markdown}");
    assert!(markdown.contains("&amp;pound;"), "{markdown}");
    assert!(markdown.contains("&amp;#163;"), "{markdown}");
    assert!(markdown.contains("&amp;notreal;"), "{markdown}");
    let visible = Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Text(value) => Some(value.into_string()),
            _ => None,
        })
        .collect::<String>();
    assert!(
        visible.contains("literal a & b; spellings &amp;, &pound;, &#163;, and &notreal;"),
        "{visible}"
    );
}

#[test]
fn escapes_literal_dollars_that_would_be_reparsed_as_math() {
    let query = ResolvedContent {
        address: None,
        label: "variables".to_owned(),
        document: Some(manual(vec![section(
            "$info = summary ([$conf])",
            Vec::new(),
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(
        markdown.contains("## \\$info = summary (\\[\\$conf\\])"),
        "{markdown}"
    );
    let headings = Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Text(value) => Some(value.into_string()),
            _ => None,
        })
        .collect::<String>();
    assert!(headings.contains("$info = summary ([$conf])"), "{headings}");
}

#[test]
fn escapes_enabled_extension_syntax_and_unsupported_block_prefixes() {
    let query = ResolvedContent {
        address: None,
        label: "extensions".to_owned(),
        document: Some(manual(vec![section(
            "TEXT",
            vec![Block::Unsupported {
                name: Some("source".to_owned()),
                text: "# injected\n~~~\na | b\n: definition\n~~strike~~\n^super^".to_owned(),
                layout: LayoutHint::default(),
                source: None,
            }],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("\\# injected"), "{markdown}");
    assert!(markdown.contains("\\~\\~\\~"), "{markdown}");
    assert!(markdown.contains("a \\| b"), "{markdown}");
    assert!(markdown.contains("\\: definition"), "{markdown}");
    assert!(markdown.contains("\\~\\~strike\\~\\~"), "{markdown}");
    assert!(markdown.contains("\\^super\\^"), "{markdown}");
    assert_eq!(
        Parser::new(&markdown)
            .filter(|event| matches!(event, Event::Start(Tag::Heading { .. })))
            .count(),
        2,
        "only the document and section headings remain structural"
    );
}
