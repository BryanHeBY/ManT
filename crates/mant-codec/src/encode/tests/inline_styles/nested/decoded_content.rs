//! Both real readers compare against admitted source IR, never candidate text.

use super::*;
use std::fmt::Write as _;

fn link(children: Vec<Inline>, title: &str) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::External {
            uri: "https://a.test".into(),
        },
        title: Some(title.into()),
        children,
    }
}

fn safe_output(source: &str) {
    assert!(
        source
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\n' | '\t')),
        "Markdown must not contain bare forbidden controls or CR: {source:?}"
    );
}

fn real_rounds(original: &[Inline], options: MarkdownOptions) {
    let expected = meaning(original);
    let mut current = original.to_vec();
    for round in 1..=2 {
        let markdown = inline::render_inline(&current, options);
        safe_output(&markdown);
        let parsed = crate::parse_markdown(&markdown, None).unwrap();
        assert!(
            parsed.document.diagnostics.is_empty(),
            "round {round}: {markdown:?} / {:?}",
            parsed.document.diagnostics
        );
        assert_eq!(mant_ir::validate_document(&parsed.document), []);
        let [Block::Paragraph { children, .. }] = parsed.document.blocks.as_slice() else {
            panic!("round {round} changed paragraph count: {markdown:?}");
        };
        assert_allowed_styles(
            &expected,
            &meaning(children),
            &format!("round {round}: {markdown:?}"),
        );
        // Exercise actual serialized JSON, not a value-based reconstruction.
        current =
            serde_json::from_str::<Vec<Inline>>(&serde_json::to_string(children).unwrap()).unwrap();
    }
}

fn hexadecimal_references(source: &str) -> String {
    let mut output = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("&#") {
        output.push_str(&rest[..start]);
        let end = rest[start..].find(';').unwrap() + start;
        let value = rest[start + 2..end].parse::<u32>().unwrap();
        write!(output, "&#x{value:X};").unwrap();
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

#[test]
fn decoded_scalar_link_text_title_matrix_is_stable_in_two_actual_reads() {
    let entities = [
        "&#13;",
        "&#10;",
        "&#13;&#10;",
        "&#9;",
        "&#12;",
        "&#11;",
        "&#27;",
        "&#127;",
        "&#1;",
        "&#7;",
        "&#8;",
        "&#31;",
        "&#129;",
        "&#133;",
        "&#159;",
        "&#0;",
        "&#160;",
        "&#8232;",
        "&#8233;",
        "&#8203;",
        "&amp;#13;",
        "&amp;#x1B;",
    ];
    let mut count = 0;
    let mut sources = std::collections::BTreeSet::new();
    for reference in entities {
        for reference in [reference.to_owned(), hexadecimal_references(reference)] {
            for source in [
                format!("x{reference}y"),
                format!("**x{reference}y**"),
                format!("[x{reference}y](https://a.test \"title\")"),
                format!("[**x{reference}y**](https://a.test \"title\")"),
                format!("[*x{reference}y*](https://a.test \"title\")"),
                format!("[label](https://a.test \"x{reference}y\")"),
                format!("`x{reference}y`"),
            ] {
                // IR0 is established by the actual reader admission. Explicit
                // independent first-read expected values are in the companion
                // markdown decoded-content matrix, including numeric C1 scalars.
                sources.insert(source.clone());
                let original = parsed_children(&source);
                for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
                    real_rounds(&original, options);
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 616);
    assert_eq!(sources.len(), 294);
}

#[test]
fn manual_ir_controls_are_a_checked_projection_without_mutating_original_content() {
    for scalar in ["\x0c", "\x0b", "\x1b", "\x7f", "\x01", "\u{81}", "\u{85}"] {
        // Hand-built IR has no producer diagnostic channel at String export.
        // Its explicit safe projection masks one SP per original UTF-8 byte,
        // including raw C1 and numeric references to those same scalars.
        let normalized = " ".repeat(scalar.len());
        let original = vec![link(
            vec![text(&format!("a{scalar}b"))],
            &format!("t{scalar}u"),
        )];
        let frozen = original.clone();
        let expected = vec![link(
            vec![text(&format!("a{normalized}b"))],
            &format!("t{normalized}u"),
        )];
        let markdown = inline::render_inline(&original, MarkdownOptions::default());
        safe_output(&markdown);
        let actual = parsed_children(&markdown);
        assert_allowed_styles(&meaning(&expected), &meaning(&actual), &markdown);
        real_rounds(&actual, MarkdownOptions::default());
        assert_eq!(original, frozen, "projection must not mutate hand IR");
    }
    for scalar in [
        "\r", "\n", "\r\n", "\t", "\u{a0}", "\u{2028}", "\u{2029}", "\u{200b}",
    ] {
        let original = vec![link(
            vec![text(&format!("a{scalar}b"))],
            &format!("t{scalar}u"),
        )];
        real_rounds(&original, MarkdownOptions::default());
    }
}

#[test]
fn safe_decoded_controls_keep_fallback_brackets_and_neighbor_occurrences() {
    for scalar in ["\r", "\r\n", "\t", "\x0c", "\x1b", "\u{81}"] {
        let safe = if matches!(scalar, "\x0c" | "\x1b") {
            " ".to_owned()
        } else if scalar == "\u{81}" {
            "  ".to_owned()
        } else {
            scalar.to_owned()
        };
        let original = vec![
            text("!"),
            link(
                vec![
                    styled(
                        false,
                        vec![text("! "), styled(true, vec![text(&format!("a{scalar}b"))])],
                    ),
                    text(" "),
                ],
                &format!("tail\\{scalar}"),
            ),
            link(
                vec![Inline::Code {
                    value: "[literal]`".into(),
                }],
                "neighbor",
            ),
        ];
        let normalized = vec![
            text("!"),
            link(
                vec![
                    styled(
                        false,
                        vec![text("! "), styled(true, vec![text(&format!("a{safe}b"))])],
                    ),
                    text(" "),
                ],
                &format!("tail\\{safe}"),
            ),
            link(
                vec![Inline::Code {
                    value: "[literal]`".into(),
                }],
                "neighbor",
            ),
        ];
        let markdown = inline::render_inline(&original, MarkdownOptions::default());
        safe_output(&markdown);
        let actual = parsed_children(&markdown);
        assert_allowed_styles(&meaning(&normalized), &meaning(&actual), &markdown);
        assert_eq!(
            meaning(&actual).links.len(),
            2,
            "! must not turn a Link into Image"
        );
        real_rounds(&actual, MarkdownOptions::default());
    }
}

#[test]
fn decoded_bom_content_stays_content_in_detached_fragments_and_link_rows() {
    use crate::encode::{MarkdownFragmentOptions, render_inline_fragment};

    // A source BOM is separately masked by the input sanitizer. An entity at
    // the same position is authored content admitted by the ordinary reader.
    let source = "&#65279;BODY";
    let admitted = crate::parse_markdown(source, None).unwrap().document;
    assert_eq!(admitted.diagnostics, []);
    let original = parsed_children(source);
    assert_eq!(mant_ir::inline_plain_text(&original), "\u{feff}BODY");
    let raw = crate::parse_markdown("\u{feff}BODY", None)
        .unwrap()
        .document;
    assert_eq!(
        raw.diagnostics[0].code.as_deref(),
        Some("markdown.byte-order-mark")
    );
    let [Block::Paragraph { children, .. }] = raw.blocks.as_slice() else {
        panic!("raw BOM must leave one paragraph");
    };
    assert_eq!(mant_ir::inline_plain_text(children), "BODY");

    for original in [
        original,
        vec![text("A\u{feff}BODY")],
        vec![text("\u{feff}"), link(vec![text("BODY")], "title")],
        vec![link(vec![text("\u{feff}BODY")], "t\u{feff}u")],
        vec![text("ROW"), Inline::LineBreak {}, text("\u{feff}BODY")],
    ] {
        let expected = meaning(&original);
        let mut current = original;
        for round in 1..=2 {
            let markdown = render_inline_fragment(&current, MarkdownFragmentOptions::default());
            let parsed = crate::parse_markdown(&markdown, None).unwrap().document;
            assert_eq!(parsed.diagnostics, [], "round {round}: {markdown:?}");
            let [Block::Paragraph { children, .. }] = parsed.blocks.as_slice() else {
                panic!("fragment changed owner: {markdown:?}");
            };
            assert_allowed_styles(&expected, &meaning(children), &markdown);
            current = children.clone();
        }
    }
}

#[test]
fn unsafe_destinations_remain_invalid_and_never_become_activatable_by_entity_remapping() {
    for reference in ["&#12;", "&#27;", "&#127;", "&#129;", "&#133;"] {
        let source = format!("[**label**](https://a.test/{reference})");
        let parsed = crate::parse_markdown(&source, None).unwrap();
        assert!(
            parsed
                .document
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref() == Some("ir.invalid-external-uri")),
            "{source:?}"
        );
        let Block::Paragraph { children, .. } = &parsed.document.blocks[0] else {
            panic!("paragraph")
        };
        assert_eq!(meaning(children).links.len(), 1);
        let frozen = children.clone();
        let markdown = inline::render_inline(children, MarkdownOptions::default());
        safe_output(&markdown);
        let actual = parsed_children(&markdown);
        assert!(
            meaning(&actual).links.is_empty(),
            "invalid source URI must not become a valid target: {markdown:?}"
        );
        assert_eq!(mant_ir::inline_plain_text(&actual), "label");
        assert_eq!(*children, frozen);
    }
    // Genuine printable Unicode remains valid label/title content while the
    // target stays an ASCII URI. It differs from numeric C1 U+0085.
    let admitted = parsed_children("[a&#8230;b](https://a.test \"t&#8230;u\")");
    real_rounds(&admitted, MarkdownOptions::default());
}

#[test]
fn inline_code_owner_retains_its_existing_hard_rows_after_cr_canonicalization() {
    for (value, normalized) in [
        ("a\rb", "a\nb"),
        ("a\r\nb", "a\nb"),
        ("a\nb", "a\nb"),
        ("a\x0cb\x1bc\u{81}d", "a b c  d"),
    ] {
        let original = vec![Inline::Code {
            value: value.into(),
        }];
        let frozen = original.clone();
        let expected = vec![Inline::Code {
            value: normalized.into(),
        }];
        let markdown = inline::render_inline(&original, MarkdownOptions::default());
        safe_output(&markdown);
        let actual = parsed_children(&markdown);
        assert_allowed_styles(&meaning(&expected), &meaning(&actual), &markdown);
        real_rounds(&actual, MarkdownOptions::default());
        assert_eq!(original, frozen);
    }
}

#[test]
fn admitted_controls_keep_original_meaning_across_final_owner_contexts() {
    let mut count = 0;
    for reference in [
        "&#13;",
        "&#13;&#10;",
        "&#12;",
        "&#27;",
        "&#129;",
        "&#133;",
        "&amp;#13;",
    ] {
        for source in [
            format!("[**a{reference}b**](https://a.test \"t{reference}u\")"),
            format!("![a{reference}b](https://a.test)"),
        ] {
            // The image source is an intentionally unsupported literal-source
            // policy control, not a promise that Markdown gains image IR.
            let original = parsed_children(&source);
            for context in [
                "paragraph",
                "heading",
                "list",
                "definition-head",
                "definition-body",
            ] {
                for options in [
                    MarkdownOptions::default(),
                    MarkdownOptions {
                        math: crate::encode::MarkdownMath::Latex,
                        preserve_semantics: true,
                        preserve_anchors: false,
                    },
                ] {
                    finalization::final_rounds(&original, context, options);
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 140);
}
