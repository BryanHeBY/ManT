//! Full-document reference rules and original ranges across leading hard rows.

use mant_ir::{
    Document, Inline, LinkTarget,
    visit::{Visit, walk_inline},
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::{
    markdown::SpannedEvent,
    markdown_mapping::{InlineMappingKind, map_inline_characters, markdown_source_events},
    parse_markdown,
};

type Link = (LinkTarget, Option<String>, String);

#[derive(Default)]
struct Links(Vec<Link>);

impl<'ir> Visit<'ir> for Links {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Link {
            target,
            title,
            children,
        } = inline
        {
            self.0.push((
                target.clone(),
                title.clone(),
                mant_ir::inline_plain_text(children),
            ));
        }
        walk_inline(self, inline);
    }
}

fn links(document: &Document) -> Vec<Link> {
    let mut links = Links::default();
    links.visit_document(document);
    links.0
}

fn link_events<'a>(events: impl Iterator<Item = SpannedEvent<'a>>) -> Vec<SpannedEvent<'a>> {
    let mut inside = false;
    events
        .filter(|(event, _)| match event {
            Event::Start(Tag::Link { .. }) => {
                inside = true;
                true
            }
            Event::End(TagEnd::Link) => {
                inside = false;
                true
            }
            _ => inside,
        })
        .collect()
}

fn source(label: &str, definitions: &str, before: bool, depth: usize, newline: &str) -> String {
    let body = match depth {
        0 => format!("<br />\n{label}\n"),
        1 => format!("- <br />\n  {}\n", label.replace('\n', "\n  ")),
        _ => format!(
            "- outer\n\n  - <br />\n    {}\n",
            label.replace('\n', "\n    ")
        ),
    };
    let (before, after) = if before {
        (definitions, "")
    } else {
        ("", definitions)
    };
    format!("# TEST\n\n## DESCRIPTION\n\n{before}\n{body}\n{after}\n").replace('\n', newline)
}

fn assert_reference(source: &str) {
    // pulldown-cmark firstpass.rs stores the first definition; linklabel.rs
    // normalizes whitespace and RefDefs::get performs Unicode case folding.
    // A six-byte ordinary word preserves offsets while the upstream parser
    // provides the independent reference events for the same document.
    let ordinary = source.replace("<br />", "BEFORE");
    let expected = link_events(Parser::new(&ordinary).into_offset_iter());
    let actual = link_events(markdown_source_events(source));
    assert_eq!(actual, expected, "{source}");
    let [(Event::Start(Tag::Link { dest_url, .. }), _), ..] = actual.as_slice() else {
        panic!("one resolved reference: {source}")
    };
    assert_eq!(dest_url.as_ref(), "https://example.org");
    assert_eq!(
        actual
            .iter()
            .filter(|(event, _)| matches!(event, Event::Start(Tag::Link { .. })))
            .count(),
        1
    );
    for (event, range) in &actual {
        assert!(source.is_char_boundary(range.start) && source.is_char_boundary(range.end));
        assert!(range.end <= source.len());
        if let Event::Text(value) = event {
            for mapped in
                map_inline_characters(source, value, range.clone(), InlineMappingKind::Text)
            {
                assert_eq!(&source[mapped.source], mapped.value.to_string());
            }
        }
    }
    let document = parse_markdown(source, None).unwrap().document;
    let control = parse_markdown(&ordinary, None).unwrap().document;
    assert_eq!(links(&document), links(&control), "{source}");
    assert_eq!(links(&document).len(), 1);
    let breaks = markdown_source_events(source)
        .filter_map(|(event, range)| matches!(event, Event::HardBreak).then_some(range))
        .collect::<Vec<_>>();
    assert_eq!(breaks.len(), 1, "{source}");
    assert_eq!(&source[breaks[0].clone()], "<br />");
    let json = serde_json::to_string(&document).unwrap();
    assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), document);
    assert_eq!(document.diagnostics.len(), 0, "{source}");
}

#[test]
fn reference_forms_share_document_definitions_and_original_ranges() {
    let definitions = [
        ("ref", "[ref]: https://example.org \"title\"\n"),
        ("ReF", "[rEf]: https://example.org \"title\"\n"),
        ("Straße", "[STRASSE]: https://example.org \"title\"\n"),
        (
            " Ref\t Label ",
            "[ref  label]: https://example.org \"title\"\n",
        ),
        (
            "ref\n label",
            "[REF LABEL]: https://example.org \"title\"\n",
        ),
        (
            "ref",
            "[REF]: https://example.org \"first\"\n[ref]: https://wrong.example \"second\"\n",
        ),
        ("ref", "[ref]: <https://example.org> \"a &amp; b\"\n"),
    ];
    let mut cases = 0;
    for (reference, definitions) in definitions {
        for label in [
            format!("[héllo🦀][{reference}]"),
            format!("[{reference}][]"),
            format!("[{reference}]"),
        ] {
            for before in [false, true] {
                for depth in 0..3 {
                    for newline in ["\n", "\r\n", "\r"] {
                        assert_reference(&source(&label, definitions, before, depth, newline));
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 378);
}

#[test]
fn undefined_references_and_other_html_keep_literal_source() {
    for label in ["[hello][missing]", "[missing][]", "[missing]"] {
        let source = source(label, "[ref]: https://example.org\n", false, 0, "\n");
        let document = parse_markdown(&source, None).unwrap().document;
        assert_eq!(links(&document).len(), 0);
        let text = markdown_source_events(&source)
            .filter_map(|(event, _)| match event {
                Event::Text(value) => Some(value.into_string()),
                _ => None,
            })
            .collect::<String>();
        assert!(text.ends_with(label), "{text}");
    }
    for body in [
        "<br>\n[hello][ref]\n",
        "<br class=x>\n[hello][ref]\n",
        "<div>\n[hello][ref]\n</div>\n",
        "<br />\n[hello][ref]\n<script>literal</script>\n",
    ] {
        let source = format!("{body}\n[ref]: https://example.org\n");
        assert_eq!(
            markdown_source_events(&source).collect::<Vec<_>>(),
            Parser::new(&source).into_offset_iter().collect::<Vec<_>>()
        );
        let document = parse_markdown(&source, None).unwrap().document;
        assert_eq!(links(&document).len(), 0);
        assert!(
            matches!(&document.blocks[..], [mant_ir::Block::Unsupported { text, .. }] if text == body)
        );
    }
}

#[test]
fn reference_images_keep_the_original_unsupported_spelling() {
    let source = "<br />\n![alt][ref]\n\n[ref]: https://example.org \"title\"\n";
    let expected = Parser::new(&source.replace("<br />", "BEFORE"))
        .filter(|event| matches!(event, Event::Start(Tag::Image { .. })))
        .map(Event::into_static)
        .collect::<Vec<_>>();
    let actual = markdown_source_events(source)
        .filter_map(|(event, _)| matches!(event, Event::Start(Tag::Image { .. })).then_some(event))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    let document = parse_markdown(source, None).unwrap().document;
    let [mant_ir::Block::Paragraph { children, .. }] = document.blocks.as_slice() else {
        panic!("image remains paragraph source")
    };
    assert_eq!(mant_ir::inline_plain_text(children), "\n![alt][ref]");
}

#[test]
fn hidden_definitions_do_not_shadow_document_references() {
    let source = concat!(
        "```\n[ref]: https://wrong.example/code\n```\n\n",
        "<div>\n[ref]: https://wrong.example/html\n</div>\n\n",
        "<br />\n[hello][ref]\n\n[ref]: https://example.org\n",
    );
    let ordinary = source.replace("<br />", "BEFORE");
    assert_eq!(
        link_events(markdown_source_events(source)),
        link_events(Parser::new(&ordinary).into_offset_iter())
    );
    let document = parse_markdown(source, None).unwrap().document;
    assert_eq!(
        links(&document),
        [(
            LinkTarget::External {
                uri: "https://example.org".into()
            },
            None,
            "hello".into()
        )]
    );
}

#[test]
fn reference_expansion_is_bounded_across_accepted_and_rejected_blocks() {
    // parse.rs::fetch_link_type_url_title charges URL plus title bytes and
    // allows the last expansion to saturate its quota. The adapter also shares
    // a quota across local parsers, including work performed in rejected blocks.
    let destination = format!("https://example.org/{}", "a".repeat(4096));
    let title = "b".repeat(4096);
    let cost = destination.len() + title.len();
    for rejected in [false, true] {
        let block = if rejected {
            "<br />\n[hello][ref]\n<script>literal</script>\n\n"
        } else {
            "<br />\n[hello][ref]\n\n"
        };
        let source = format!(
            "{}<br />\n[hello][ref]\n\n[ref]: {destination} \"{title}\"\n",
            block.repeat(128)
        );
        let quota = source.len().max(100_000);
        let events = markdown_source_events(&source).collect::<Vec<_>>();
        let resolved = events
            .iter()
            .filter(|(event, _)| matches!(event, Event::Start(Tag::Link { .. })))
            .count();
        if rejected {
            assert_eq!(
                resolved, 0,
                "rejected work cannot restore the expansion quota"
            );
            assert!(events.iter().any(|(event, _)| matches!(event, Event::Html(value) if value.contains("<script>literal</script>"))));
        } else {
            assert_eq!(resolved, quota.div_ceil(cost));
            assert!(resolved * cost < quota + cost);
        }
        let literal = events
            .iter()
            .filter_map(|(event, _)| match event {
                Event::Text(value) => Some(value.as_ref()),
                _ => None,
            })
            .collect::<String>();
        assert!(literal.ends_with("[hello][ref]"), "{literal}");
    }
}
