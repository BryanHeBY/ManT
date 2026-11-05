//! Decoded admission is independent of entity spelling and original offsets.

use super::*;
use mant_ir::{Document, LinkTarget};

struct ScalarCase {
    value: &'static str,
    decoded: &'static str,
    masked: bool,
}

fn scalars() -> [ScalarCase; 21] {
    [
        ("\r", "\r", false),
        ("\n", "\n", false),
        ("\r\n", "\r\n", false),
        ("\t", "\t", false),
        ("\x0c", " ", true),
        ("\x0b", " ", true),
        ("\x1b", " ", true),
        ("\x7f", " ", true),
        ("\x01", " ", true),
        ("\x07", " ", true),
        ("\x08", " ", true),
        ("\x1f", " ", true),
        ("\u{81}", "  ", true),
        // Locked pulldown-cmark scanners::scan_entity/char_from_codepoint
        // returns the numeric scalar directly; there is no C1 remapping.
        ("\u{85}", "  ", true),
        ("\u{9f}", "  ", true),
        ("\0", "\u{fffd}", false),
        ("\u{a0}", "\u{a0}", false),
        ("\u{2028}", "\u{2028}", false),
        ("\u{2029}", "\u{2029}", false),
        ("\u{200b}", "\u{200b}", false),
        ("中", "中", false),
    ]
    .map(|(value, decoded, masked)| ScalarCase {
        value,
        decoded,
        masked,
    })
}

fn first_root(document: &Document) -> &[Inline] {
    if let Some(heading) = &document.heading {
        &heading.content
    } else {
        first_root_block(&document.blocks[0])
    }
}

fn first_root_block(block: &Block) -> &[Inline] {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => children,
        Block::List { items, .. } => first_root_block(&items[0].blocks[0]),
        Block::Table { rows, .. } => first_root_block(&rows[0].cells[0].blocks[0]),
        other => panic!("unexpected decoded owner {other:?}"),
    }
}

fn first_link(nodes: &[Inline]) -> (&LinkTarget, &Option<String>, &[Inline]) {
    for node in nodes {
        match node {
            Inline::Link {
                target,
                title,
                children,
            } => return (target, title, children),
            Inline::Strong { children } | Inline::Emphasis { children } => {
                return first_link(children);
            }
            _ => {}
        }
    }
    panic!("missing source-owned link: {nodes:?}");
}

#[test]
fn decoded_scalar_admission_matrix_keeps_source_ranges_and_first_read_diagnostics() {
    let mut count = 0;
    for case in scalars() {
        for hexadecimal in [false, true] {
            let reference = case
                .value
                .chars()
                .map(|character| {
                    if hexadecimal {
                        format!("&#x{:X};", u32::from(character))
                    } else {
                        format!("&#{};", u32::from(character))
                    }
                })
                .collect::<String>();
            for context in [
                "text",
                "code",
                "link",
                "styled-link",
                "title",
                "heading",
                "list",
                "table",
            ] {
                let value = format!("x{reference}y");
                let source = match context {
                    "text" => value.clone(),
                    "code" => format!("`{value}`"),
                    "link" => format!("[{value}](https://a.test)"),
                    "styled-link" => format!("[**{value}**](https://a.test)"),
                    "title" => format!("[label](https://a.test \"{value}\")"),
                    "heading" => format!("# {value}"),
                    "list" => format!("- {value}"),
                    "table" => format!("| {value} |\n| --- |"),
                    _ => unreachable!(),
                };
                let document = parse_markdown(&source, None).unwrap().document;
                let nodes = first_root(&document);
                let expected = if context == "code" {
                    value
                } else {
                    format!("x{}y", case.decoded)
                };
                if context == "title" {
                    assert_eq!(
                        first_link(nodes).1.as_deref(),
                        Some(expected.as_str()),
                        "{source:?}"
                    );
                } else {
                    assert_eq!(mant_ir::inline_plain_text(nodes), expected, "{source:?}");
                }
                let diagnostics = document
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| {
                        diagnostic.code.as_deref() == Some("markdown.control-characters")
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    diagnostics.len(),
                    usize::from(case.masked && context != "code"),
                    "{source:?}"
                );
                assert!(
                    mant_ir::validate_document(&document).is_empty(),
                    "{source:?}"
                );
                for diagnostic in diagnostics {
                    let range = diagnostic.source.unwrap().byte_range.unwrap();
                    let start = usize::try_from(range.start.get()).unwrap();
                    let end = usize::try_from(range.end.get()).unwrap();
                    let original = &source[start..end];
                    assert!(
                        original.contains(&reference),
                        "event span must cover authored entity: {source:?} / {original:?}"
                    );
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 336);
}

#[test]
fn raw_controls_and_entity_controls_share_policy_without_double_decoding() {
    for (scalar, expected) in [
        ("\x0c", " "),
        ("\x1b", " "),
        ("\x7f", " "),
        ("\u{81}", "  "),
    ] {
        let source = format!("# 中\n\nx{scalar}y");
        let document = parse_markdown(&source, None).unwrap().document;
        let diagnostic = document
            .diagnostics
            .iter()
            .find(|finding| finding.code.as_deref() == Some("markdown.control-characters"))
            .unwrap();
        let range = diagnostic.source.unwrap().byte_range.unwrap();
        assert_eq!(
            &source[usize::try_from(range.start.get()).unwrap()
                ..usize::try_from(range.end.get()).unwrap()],
            scalar
        );
        assert_eq!(
            mant_ir::inline_plain_text(first_root_block(&document.blocks[0])),
            format!("x{expected}y")
        );
    }
    for reference in ["&amp;#13;", "&amp;#x1B;", "&amp;#129;"] {
        let source = format!("[x{reference}y](https://a.test \"x{reference}y\")");
        let document = parse_markdown(&source, None).unwrap().document;
        assert_eq!(document.diagnostics, []);
        let (_, title, children) = first_link(first_root(&document));
        let expected = format!("x{}y", reference.replacen("&amp;", "&", 1));
        assert_eq!(mant_ir::inline_plain_text(children), expected);
        assert_eq!(title.as_deref(), Some(expected.as_str()));
    }
}

#[test]
fn decoded_title_diagnostics_cover_original_entity_not_decoded_scalar_coordinates() {
    let source = "# 中\r\n\r\n[a&#12;b](https://a.test \"before&#27;after\")";
    let document = parse_markdown(source, None).unwrap().document;
    let diagnostics = document
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("markdown.control-characters"))
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 2);
    for (diagnostic, spelling) in diagnostics.into_iter().zip(["&#12;", "&#27;"]) {
        let span = diagnostic.source.unwrap();
        let range = span.byte_range.unwrap();
        let raw = &source[usize::try_from(range.start.get()).unwrap()
            ..usize::try_from(range.end.get()).unwrap()];
        assert!(raw.contains(spelling), "{spelling} must belong to {raw:?}");
        assert_eq!(span.line, 3);
    }
}

#[test]
fn every_forbidden_raw_c0_c1_scalar_is_admitted_once_in_text_code_label_and_title() {
    let mut cases = 0;
    for scalar in (0..=0x1f)
        .chain(0x7f..=0x9f)
        .filter(|value| !matches!(*value, 9 | 10 | 13))
    {
        let raw = char::from_u32(scalar).unwrap();
        let expected = if scalar < 0x80 { "a b" } else { "a  b" };
        for source in [
            format!("a{raw}b"),
            format!("`a{raw}b`"),
            format!("[a{raw}b](https://a.test)"),
            format!("[label](https://a.test \"a{raw}b\")"),
        ] {
            let document = parse_markdown(&source, None).unwrap().document;
            let diagnostics = document
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.code.as_deref() == Some("markdown.control-characters")
                })
                .collect::<Vec<_>>();
            assert_eq!(diagnostics.len(), 1, "one first-read mask: {source:?}");
            let range = diagnostics[0].source.unwrap().byte_range.unwrap();
            assert_eq!(
                &source[usize::try_from(range.start.get()).unwrap()
                    ..usize::try_from(range.end.get()).unwrap()],
                raw.to_string()
            );
            let nodes = first_root(&document);
            if source.starts_with("[label]") {
                assert_eq!(first_link(nodes).1.as_deref(), Some(expected));
            } else {
                assert_eq!(mant_ir::inline_plain_text(nodes), expected);
            }
            assert_eq!(mant_ir::validate_document(&document), []);
            cases += 1;
        }
    }
    assert_eq!(cases, 248);
}

#[test]
fn locked_parser_numeric_references_are_unicode_scalars_without_c1_reinterpretation() {
    // Independent upstream event observation: scanners::scan_entity delegates
    // to char_from_codepoint, replacing zero/invalid Unicode but retaining C1.
    // The producer's subsequent safe admission, not a guessed HTML remap,
    // is responsible for the first-read diagnostic and two masking spaces.
    use pulldown_cmark::{Event, Parser, Tag};
    let source = "[a&#133;b](https://a.test/&#133; \"t&#133;u\")";
    let events = Parser::new(source).collect::<Vec<_>>();
    let label = events
        .iter()
        .filter_map(|event| match event {
            Event::Text(value) => Some(value.as_ref()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(label, "a\u{85}b");
    assert!(events.iter().any(|event| matches!(event,
        Event::Start(Tag::Link { dest_url, title, .. })
        if dest_url.as_ref() == "https://a.test/\u{85}" && title.as_ref() == "t\u{85}u")));
    let document = parse_markdown("x&hellip;&#8230;y", None).unwrap().document;
    assert_eq!(document.diagnostics, []);
    assert_eq!(mant_ir::inline_plain_text(first_root(&document)), "x……y");
}

#[test]
fn physical_source_line_endings_and_decoded_content_use_their_own_first_read_rules() {
    for ending in ["\r", "\n", "\r\n"] {
        // Locked parse.rs::make_code_span visits CR and LF separately;
        // unlike a soft break, an authored CRLF currently produces two code
        // spaces. The frozen baseline source confirms this first-read fact.
        for (source, expected) in [
            (format!("a{ending}b"), "a b"),
            (
                format!("`a{ending}b`"),
                if ending == "\r\n" { "a  b" } else { "a b" },
            ),
        ] {
            let document = parse_markdown(&source, None).unwrap().document;
            assert_eq!(document.diagnostics, []);
            assert_eq!(mant_ir::inline_plain_text(first_root(&document)), expected);
        }
        let source = format!("```{ending}a{ending}b{ending}```");
        let document = parse_markdown(&source, None).unwrap().document;
        assert_eq!(document.diagnostics, []);
        assert_eq!(mant_ir::inline_plain_text(first_root(&document)), "a\nb");
    }
}
