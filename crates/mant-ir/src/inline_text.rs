//! Shared operations over source-independent inline IR nodes.

use crate::Inline;

/// Flatten inline structure into the text visible to readers and search.
/// Styles and links contribute their original children, anchors contribute no
/// text, and hard breaks contribute `\n`. This is not a terminal sanitizer,
/// whitespace normalizer, or bounded reference-label projection.
///
/// # Panics
///
/// Panics only if the internal legacy backend rejects a directly owned inline
/// leaf, which violates this compatibility API's representation invariant.
#[must_use]
pub fn inline_plain_text(nodes: &[Inline]) -> String {
    crate::ContentContext::detached()
        .plain_text(nodes)
        .expect("legacy inline text is self-contained")
}

/// Visit borrowed visible text leaves in source order, without decoration.
///
/// Styles and links contribute their children, anchors are empty, and hard
/// breaks contribute a newline. This shares the exact content domain of
/// [`inline_plain_text`] without allocating an intermediate flattened string.
/// No sanitization, name matching or presentation roles are applied.
///
/// # Panics
///
/// Panics only if the internal legacy backend rejects a directly owned inline
/// leaf.
pub fn visit_inline_plain_text<'a>(nodes: &'a [Inline], mut emit: impl FnMut(&'a str)) {
    crate::ContentContext::detached()
        .visit_plain_text(nodes, &mut emit)
        .expect("legacy inline text is self-contained");
}

/// First character visible to a renderer without allocating flattened text.
/// Hard breaks and whitespace are characters; empty nodes and anchors are skipped.
///
/// # Panics
///
/// Panics only if the internal legacy backend rejects a directly owned inline
/// leaf.
#[must_use]
pub fn first_visible_character(nodes: &[Inline]) -> Option<char> {
    crate::ContentContext::detached()
        .first_visible_character(nodes)
        .expect("legacy inline text is self-contained")
}

/// Last character visible to a renderer without allocating flattened text.
/// Hard breaks and whitespace are characters; empty nodes and anchors are skipped.
///
/// # Panics
///
/// Panics only if the internal legacy backend rejects a directly owned inline
/// leaf.
#[must_use]
pub fn last_visible_character(nodes: &[Inline]) -> Option<char> {
    crate::ContentContext::detached()
        .last_visible_character(nodes)
        .expect("legacy inline text is self-contained")
}

/// Whether an inline fragment contains content other than layout-only breaks.
/// Spaces, tabs and carriage returns count as content; only `\n` is excluded.
/// This is intentionally different from checking trimmed text for nonemptiness.
///
/// # Panics
///
/// Panics only if the internal legacy backend rejects a directly owned inline
/// leaf.
#[must_use]
pub fn has_printable_character(nodes: &[Inline]) -> bool {
    crate::ContentContext::detached()
        .has_printable_character(nodes)
        .expect("legacy inline text is self-contained")
}

/// Decide whether definition terms fit beside their first description line.
#[must_use]
pub fn terms_fit_inline(terms: &[Vec<Inline>], max_width: usize) -> bool {
    crate::geometry::definition_run_in_width(terms)
        .is_some_and(|width| (1..=max_width).contains(&width))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_text_is_shared_by_heading_and_nested_inline_consumers() {
        let nodes = vec![
            Inline::anchor("start"),
            Inline::Strong {
                children: vec![
                    Inline::Text {
                        value: String::new(),
                    },
                    Inline::Emphasis {
                        children: vec![Inline::Link {
                            target: crate::LinkTarget::External {
                                uri: "https://example.test".into(),
                            },
                            title: Some("not visible".into()),
                            children: vec![Inline::Code {
                                value: "é👩‍💻".into(),
                            }],
                        }],
                    },
                ],
            },
            Inline::LineBreak,
            Inline::Text {
                value: "尾\t ".into(),
            },
            Inline::anchor("end"),
        ];
        assert_eq!(inline_plain_text(&nodes), "é👩‍💻\n尾\t ");
        let mut pieces = Vec::new();
        visit_inline_plain_text(&nodes, |text| pieces.push(text));
        assert_eq!(pieces, ["", "é👩‍💻", "\n", "尾\t "]);
        assert_eq!(pieces.concat(), inline_plain_text(&nodes));
        assert_eq!(first_visible_character(&nodes), Some('é'));
        assert_eq!(last_visible_character(&nodes), Some(' '));
        assert!(has_printable_character(&nodes));
        assert_eq!(
            crate::Heading {
                content: nodes.clone(),
                source: None
            }
            .plain_text(),
            inline_plain_text(&nodes)
        );
    }

    #[test]
    fn empty_anchors_and_breaks_are_distinct_from_space_content() {
        let anchors = vec![
            Inline::anchor("hidden"),
            Inline::Strong { children: vec![] },
        ];
        assert_eq!(inline_plain_text(&anchors), "");
        assert_eq!(first_visible_character(&anchors), None);
        assert_eq!(last_visible_character(&anchors), None);
        assert!(!has_printable_character(&anchors));
        let breaks = vec![
            Inline::LineBreak,
            Inline::Emphasis {
                children: vec![Inline::Text {
                    value: "\n\n".into(),
                }],
            },
        ];
        assert_eq!(inline_plain_text(&breaks), "\n\n\n");
        assert_eq!(first_visible_character(&breaks), Some('\n'));
        assert_eq!(last_visible_character(&breaks), Some('\n'));
        assert!(!has_printable_character(&breaks));
        for value in [" ", "\t", "\r", "\n \n"] {
            assert!(has_printable_character(&[Inline::Code {
                value: value.into()
            }]));
        }
    }

    #[test]
    fn tag_fit_uses_visible_cells_inside_styles_and_original_hard_lines() {
        let terms = |text: &str| {
            vec![vec![Inline::Strong {
                children: vec![Inline::Text { value: text.into() }],
            }]]
        };
        assert!(!terms_fit_inline(&terms("日本日本"), 7));
        assert!(terms_fit_inline(&terms("日本日本"), 8));
        assert!(terms_fit_inline(&terms("e\u{301}"), 1));
        assert!(terms_fit_inline(&terms("😀"), 2));
        assert!(!terms_fit_inline(&terms("😀"), 1));
        assert!(terms_fit_inline(&terms("abc\ndef"), 3));
    }
}
