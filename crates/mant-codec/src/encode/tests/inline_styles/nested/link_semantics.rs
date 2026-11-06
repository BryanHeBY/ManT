//! Source IR owns label scalars and every occurrence, independent of its bytes.
use super::*;

#[derive(Clone, Copy, Default)]
pub(super) struct StyleChanges {
    pub(super) first_loss: bool,
    pub(super) second_further_loss: bool,
    pub(super) any_ir0_loss: bool,
}

pub(super) fn assert_rounds(original: &[Inline]) -> bool {
    roundtrip_styles(original).any_ir0_loss
}

pub(super) fn roundtrip_styles(original: &[Inline]) -> StyleChanges {
    let expected = meaning(original);
    let mut current = original.to_vec();
    let mut changes = StyleChanges::default();
    let mut first = None;
    for round in 1..=2 {
        let markdown = inline::render_inline(&current, MarkdownOptions::default());
        current = parsed_children(&markdown);
        let actual = meaning(&current);
        assert_allowed_styles(&expected, &actual, &format!("round {round}: {markdown}"));
        changes.any_ir0_loss |= actual != expected;
        if round == 1 {
            changes.first_loss = actual != expected;
            first = Some(actual);
        } else if let Some(first) = &first {
            // Re-encoding IR1 cannot invent a style that IR1 no longer owns,
            // even when that style happened to exist in the original IR0.
            assert_allowed_styles(first, &actual, &format!("second read: {markdown}"));
            changes.second_further_loss = actual != *first;
        }
    }
    changes
}

fn source_metadata(source: &str) -> String {
    source
        .replace('&', "&amp;")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[test]
fn link_metadata_and_bracket_context_matrix_use_original_ir_for_both_readbacks() {
    let titles = [
        None,
        Some(""),
        Some("plain"),
        Some("say \"quote\""),
        Some("say \\"),
        Some("say \\\\"),
        Some("say \\\""),
        Some("say \\*"),
        Some("say \\&"),
        Some("&amp;"),
        Some("&#x41;"),
        Some("&#32;"),
        Some("one\ntwo"),
        Some("one\r\ntwo"),
        Some("one\ttwo"),
        Some(" leading "),
        Some("中e\u{301}"),
    ];
    let targets = [
        "https://a.test",
        "https://a.test/&amp;",
        "https://a.test/&#38;",
        "https://a.test/%26amp%3B",
        "https://a.test/(a)",
        "https://a.test/%5Ca",
        "https://a.test/a&b",
        "https://a.test/%E4%B8%AD",
    ];
    let labels = ["a", "a  \nb", "_! ***a***_&#32;", r"\!", r"a\[b\]"];
    let mut count = 0;
    let mut degraded = 0;
    for title in titles {
        for target in targets {
            for label in labels {
                let suffix = title.map_or_else(String::new, |title| {
                    format!(" \"{}\"", source_metadata(title))
                });
                let source = format!("[{label}](<{}>{suffix})", source_metadata(target));
                for source in [
                    source.clone(),
                    format!("**{source}**"),
                    format!("x {source} y"),
                    format!("{source}[next](https://b.test \"control\")"),
                ] {
                    let original = parsed_children(&source);
                    assert!(
                        !meaning(&original).links.is_empty(),
                        "source must be a Link: {source:?}"
                    );
                    degraded += usize::from(assert_rounds(&original));
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 2720);
    eprintln!("link metadata matrix: {count} cases, {degraded} allowed style degradations");
}

#[test]
fn literal_metadata_entities_backslashes_and_control_rows_keep_their_ir_identity() {
    for target in [
        "https://a.test/&amp;",
        "https://a.test/&#38;",
        "https://a.test/&#x41;",
        "https://a.test/a&b",
        "https://a.test/%26amp%3B",
        "https://a.test/a(b)%5Cc",
    ] {
        for title in [
            None,
            Some("\\"),
            Some("\\\\"),
            Some("\\\""),
            Some("\\*"),
            Some("&amp; &#38; &#x41;"),
            Some("a\nb\r\nc\td"),
            Some(" 中 \u{a0} "),
        ] {
            for children in [
                vec![
                    text(" "),
                    styled(false, vec![styled(true, vec![text("a")])]),
                    text(" "),
                ],
                vec![
                    Inline::Code {
                        value: "x`y".into(),
                    },
                    Inline::line_break(),
                    text("!"),
                ],
                vec![
                    text("a"),
                    Inline::line_break(),
                    Inline::line_break(),
                    text("b"),
                ],
            ] {
                let nodes = [Inline::Link {
                    target: mant_ir::LinkTarget::External { uri: target.into() },
                    title: title.map(Into::into),
                    children,
                }];
                assert_rounds(&nodes);
            }
        }
    }
}

#[test]
fn literal_angle_urls_inside_single_link_labels_do_not_create_nested_links() {
    for source in [
        r"[\<https://inner.test\>](https://outer.test)",
        r"[**\<https://inner.test/?a&amp;b\>**](https://outer.test)",
        r"[*first \<https://inner.test\> last*](https://outer.test)",
    ] {
        let nodes = parsed_children(source);
        assert_eq!(meaning(&nodes).links.len(), 1, "{source}");
        assert_rounds(&nodes);
    }
    for node in [
        text("<https://inner.test>"),
        styled(true, vec![text("<https://inner.test> &amp;")]),
        styled(false, vec![text("<<https://inner.test>>")]),
    ] {
        let nodes = [Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://outer.test".into(),
            },
            title: Some("literal label".into()),
            children: vec![node],
        }];
        assert_rounds(&nodes);
    }
}
