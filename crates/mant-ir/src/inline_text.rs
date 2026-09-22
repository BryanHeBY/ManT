//! Shared operations over source-independent inline IR nodes.

use crate::{ContentContext, Inline};

/// Flatten inline structure into the text visible to readers and search.
/// Styles and links contribute their original children, anchors contribute no
/// text, and hard breaks contribute `\n`. This is not a terminal sanitizer,
/// whitespace normalizer, or bounded reference-label projection.
///
/// # Panics
///
/// Panics if a retained key or range does not resolve in `content`.
#[must_use]
pub fn inline_plain_text(content: ContentContext<'_>, nodes: &[Inline]) -> String {
    content
        .plain_text(nodes)
        .expect("inline text resolves in its authoritative content store")
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
/// Panics if a retained key or range does not resolve in `content`.
pub fn visit_inline_plain_text<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
    mut emit: impl FnMut(&'a str),
) {
    content
        .visit_plain_text(nodes, &mut emit)
        .expect("inline text resolves in its authoritative content store");
}

/// First character visible to a renderer without allocating flattened text.
/// Hard breaks and whitespace are characters; empty nodes and anchors are skipped.
///
/// # Panics
///
/// Panics if a retained key or range does not resolve in `content`.
#[must_use]
pub fn first_visible_character(content: ContentContext<'_>, nodes: &[Inline]) -> Option<char> {
    content
        .first_visible_character(nodes)
        .expect("inline text resolves in its authoritative content store")
}

/// Last character visible to a renderer without allocating flattened text.
/// Hard breaks and whitespace are characters; empty nodes and anchors are skipped.
///
/// # Panics
///
/// Panics if a retained key or range does not resolve in `content`.
#[must_use]
pub fn last_visible_character(content: ContentContext<'_>, nodes: &[Inline]) -> Option<char> {
    content
        .last_visible_character(nodes)
        .expect("inline text resolves in its authoritative content store")
}

/// Whether an inline fragment contains content other than layout-only breaks.
/// Spaces, tabs and carriage returns count as content; only `\n` is excluded.
/// This is intentionally different from checking trimmed text for nonemptiness.
///
/// # Panics
///
/// Panics if a retained key or range does not resolve in `content`.
#[must_use]
pub fn has_printable_character(content: ContentContext<'_>, nodes: &[Inline]) -> bool {
    content
        .has_printable_character(nodes)
        .expect("inline text resolves in its authoritative content store")
}

/// Decide whether definition terms fit beside their first description line.
#[must_use]
pub fn terms_fit_inline(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
    max_width: usize,
) -> bool {
    content
        .definition_run_in_width(terms)
        .ok()
        .flatten()
        .is_some_and(|width| (1..=max_width).contains(&width))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ContentFixture;

    #[test]
    fn original_text_is_shared_by_heading_and_nested_inline_consumers() {
        let mut fixture = ContentFixture::body();
        let start = fixture.anchor("start");
        let empty = fixture.text(String::new());
        let link = fixture.link_text(
            crate::LinkTarget::External {
                uri: "https://example.test".into(),
            },
            Some("not visible".into()),
            "é👩‍💻",
            true,
        );
        let line_break = fixture.hard_break();
        let tail = fixture.text("尾\t ");
        let end = fixture.anchor("end");
        let nodes = vec![
            start,
            Inline::Strong {
                children: vec![
                    empty,
                    Inline::Emphasis {
                        children: vec![link],
                    },
                ],
            },
            line_break,
            tail,
            end,
        ];
        let store = fixture.finish();
        let content = store.content();
        assert_eq!(inline_plain_text(content, &nodes), "é👩‍💻\n尾\t ");
        let mut pieces = Vec::new();
        visit_inline_plain_text(content, &nodes, |text| pieces.push(text));
        assert_eq!(pieces, ["", "é👩‍💻", "\n", "尾\t "]);
        assert_eq!(pieces.concat(), inline_plain_text(content, &nodes));
        assert_eq!(first_visible_character(content, &nodes), Some('é'));
        assert_eq!(last_visible_character(content, &nodes), Some(' '));
        assert!(has_printable_character(content, &nodes));
        assert_eq!(
            crate::Heading {
                content: nodes.clone(),
                source: None
            }
            .plain_text(content),
            inline_plain_text(content, &nodes)
        );
    }

    #[test]
    fn empty_anchors_and_breaks_are_distinct_from_space_content() {
        let mut fixture = ContentFixture::body();
        let anchors = vec![
            fixture.anchor("hidden"),
            Inline::Strong { children: vec![] },
        ];
        let line_break = fixture.hard_break();
        let newlines = fixture.styled_text(
            "\n\n",
            crate::ContentStyle {
                emphasis: true,
                ..crate::ContentStyle::default()
            },
        );
        let breaks = vec![
            line_break,
            Inline::Emphasis {
                children: vec![newlines],
            },
        ];
        let store = fixture.finish();
        let content = store.content();
        assert_eq!(inline_plain_text(content, &anchors), "");
        assert_eq!(first_visible_character(content, &anchors), None);
        assert_eq!(last_visible_character(content, &anchors), None);
        assert!(!has_printable_character(content, &anchors));
        assert_eq!(inline_plain_text(content, &breaks), "\n\n\n");
        assert_eq!(first_visible_character(content, &breaks), Some('\n'));
        assert_eq!(last_visible_character(content, &breaks), Some('\n'));
        assert!(!has_printable_character(content, &breaks));
        for value in [" ", "\t", "\r", "\n \n"] {
            let mut fixture = ContentFixture::body();
            let code = fixture.code(value);
            let store = fixture.finish();
            assert!(has_printable_character(store.content(), &[code]));
        }
    }

    #[test]
    fn tag_fit_uses_visible_cells_inside_styles_and_original_hard_lines() {
        let fits = |text: &str, width| {
            let mut fixture = ContentFixture::body();
            let text = fixture.styled_text(
                text,
                crate::ContentStyle {
                    strong: true,
                    ..crate::ContentStyle::default()
                },
            );
            let terms = vec![vec![Inline::Strong {
                children: vec![text],
            }]];
            let store = fixture.finish();
            terms_fit_inline(store.content(), &terms, width)
        };
        assert!(!fits("日本日本", 7));
        assert!(fits("日本日本", 8));
        assert!(fits("e\u{301}", 1));
        assert!(fits("😀", 2));
        assert!(!fits("😀", 1));
        assert!(fits("abc\ndef", 3));
    }
}
