//! One borrowed phrasing stream for source-owned fragments and transparent labels.

mod projection;
mod serialization;
mod styles;
#[cfg(test)]
mod work;

use std::borrow::Cow;

use mant_ir::Inline;

use super::links::{PhrasingNode, phrasing_nodes, render_link};
use super::{MarkdownOptions, code_span, escape_text, html_anchor};
pub(super) use projection::{LinkMeaning, link_meaning, matches_link};
use serialization::{Framing, append_safe_piece, render_inline_pieces};

#[derive(Clone, Copy)]
struct StyleMarkers {
    primary: &'static str,
    alternate: &'static str,
}

enum AcceptedMeaning {
    Phrasing,
    Link(LinkMeaning),
    UnsupportedLink,
}

struct InlinePiece<'source> {
    rendered: String,
    markers: Option<StyleMarkers>,
    styled: bool,
    code: Option<Cow<'source, str>>,
    accepted: AcceptedMeaning,
}

impl<'source> InlinePiece<'source> {
    fn plain(rendered: String) -> Self {
        Self {
            rendered,
            markers: None,
            styled: false,
            code: None,
            accepted: AcceptedMeaning::Phrasing,
        }
    }

    fn styled(rendered: String, primary: &'static str, alternate: &'static str) -> Self {
        let styled = !rendered.trim_matches([' ', '\t', '\n']).is_empty();
        Self {
            rendered,
            markers: Some(StyleMarkers { primary, alternate }),
            styled,
            code: None,
            accepted: AcceptedMeaning::Phrasing,
        }
    }

    fn link(link: super::links::EncodedLink) -> Self {
        Self {
            rendered: link.source,
            accepted: link
                .meaning
                .map_or(AcceptedMeaning::UnsupportedLink, AcceptedMeaning::Link),
            ..Self::plain(String::new())
        }
    }

    fn code(value: &'source str) -> Self {
        Self {
            rendered: String::new(),
            markers: None,
            styled: false,
            code: Some(Cow::Borrowed(value)),
            accepted: AcceptedMeaning::Phrasing,
        }
    }
}

#[cfg(test)]
fn render_inline_raw(nodes: &[Inline], options: MarkdownOptions, manual_links: bool) -> String {
    render_nodes(nodes.iter(), options, manual_links, Framing::Raw)
}

/// Row protection is part of the accepted representation, rather than an
/// unchecked rewrite after delimiter validation. Every public inline route
/// reaches this boundary once; recursively accepted children remain phrasing.
pub(super) fn render_inline_nodes<'source>(
    nodes: impl Iterator<Item = &'source Inline> + Clone,
    options: MarkdownOptions,
    manual_links: bool,
    interruptible: bool,
) -> String {
    render_nodes(
        nodes,
        options,
        manual_links,
        Framing::Rows {
            manual_links,
            interruptible,
        },
    )
}

fn render_nodes<'source>(
    nodes: impl Iterator<Item = &'source Inline> + Clone,
    options: MarkdownOptions,
    manual_links: bool,
    framing: Framing,
) -> String {
    render_inline_pieces(
        &mut coalesce_code_pieces(inline_pieces(nodes.clone(), options, manual_links, false)),
        framing,
    )
    .unwrap_or_else(|| render_unstyled_framed(nodes, options, manual_links, framing))
}

#[cfg(test)]
fn render_unstyled<'source>(
    nodes: impl Iterator<Item = &'source Inline>,
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_unstyled_framed(nodes, options, manual_links, Framing::Raw)
}

fn render_unstyled_framed<'source>(
    nodes: impl Iterator<Item = &'source Inline>,
    options: MarkdownOptions,
    manual_links: bool,
    framing: Framing,
) -> String {
    // Revisit the original borrowed nodes, preserving code coalescing and
    // typed links. Stripping bytes from a rejected Markdown candidate could
    // delete authored marker characters or change destination identities.
    let mut pieces = coalesce_code_pieces(inline_pieces(nodes, options, manual_links, true));
    if let Some(output) = render_inline_pieces(&mut pieces, framing) {
        return output;
    }
    // Unsupported phrasing (notably nested active Link wrappers, which
    // CommonMark cannot express) retains the established best-effort source
    // policy. It is explicitly unproved, never a candidate-derived witness.
    let mut output = String::new();
    for piece in pieces {
        append_safe_piece(&mut output, &piece.rendered);
    }
    framing.render(&output)
}

pub(super) fn render_link_label(
    children: &[Inline],
    options: MarkdownOptions,
    manual_links: bool,
    omit_styles: bool,
) -> String {
    if omit_styles {
        render_unstyled_framed(children.iter(), options, manual_links, Framing::Raw)
    } else {
        render_nodes(children.iter(), options, manual_links, Framing::Raw)
    }
}

fn inline_pieces<'source>(
    nodes: impl Iterator<Item = &'source Inline>,
    options: MarkdownOptions,
    manual_links: bool,
    omit_styles: bool,
) -> Vec<InlinePiece<'source>> {
    let mut nodes = phrasing_nodes(nodes, options, manual_links, omit_styles).peekable();
    let mut pieces = Vec::new();
    while let Some(child) = nodes.next() {
        match child {
            PhrasingNode::Node(Inline::Text { value }) => {
                // AST text segmentation must not change delimiter decisions.
                // Merge only transparent text siblings: crossing a style or
                // link would ignore real emitted Markdown punctuation.
                let mut text = Cow::Borrowed(value.as_str());
                while let Some(PhrasingNode::Node(Inline::Text { value })) = nodes.peek() {
                    text.to_mut().push_str(value);
                    nodes.next();
                }
                pieces.push(InlinePiece::plain(escape_text(&text)));
            }
            PhrasingNode::Node(Inline::Strong { children }) => {
                let mut segments = vec![children.as_slice()];
                while let Some(PhrasingNode::Node(Inline::Strong { children })) = nodes.peek() {
                    segments.push(children.as_slice());
                    nodes.next();
                }
                pieces.push(render_style(&segments, options, manual_links, ("**", "__")));
            }
            PhrasingNode::Node(Inline::Emphasis { children }) => {
                let mut segments = vec![children.as_slice()];
                while let Some(PhrasingNode::Node(Inline::Emphasis { children })) = nodes.peek() {
                    segments.push(children.as_slice());
                    nodes.next();
                }
                pieces.push(render_style(&segments, options, manual_links, ("*", "_")));
            }
            PhrasingNode::Node(Inline::Code { value } | Inline::Equation { value, .. }) => {
                pieces.push(InlinePiece::code(value));
            }
            PhrasingNode::Link {
                destination,
                title,
                children,
            } => pieces.push(InlinePiece::link(render_link(
                &destination,
                title,
                children,
                options,
                manual_links,
                omit_styles,
            ))),
            PhrasingNode::Node(Inline::Anchor {
                id,
                fragment_aliases,
                ..
            }) if options.preserve_anchors => {
                // Inline destinations cannot introduce source hard rows.
                // Block/fence destinations retain their separate framing.
                let mut seen = std::collections::HashSet::new();
                let markers = std::iter::once(id.as_str())
                    .chain(fragment_aliases.iter().map(mant_ir::FragmentAlias::as_str))
                    .filter(|target| seen.insert(*target))
                    .map(html_anchor)
                    .collect();
                pieces.push(InlinePiece::plain(markers));
            }
            PhrasingNode::Node(Inline::Anchor { .. }) => {}
            PhrasingNode::Node(Inline::LineBreak {}) => {
                pieces.push(InlinePiece::plain("\n".into()));
            }
            PhrasingNode::Node(Inline::Link { .. }) => {
                unreachable!("phrasing traversal resolves every link wrapper")
            }
        }
    }
    pieces
}

fn render_style<'source>(
    segments: &[&'source [Inline]],
    options: MarkdownOptions,
    manual_links: bool,
    markers: (&'static str, &'static str),
) -> InlinePiece<'source> {
    let rendered = render_nodes(
        segments.iter().flat_map(|nodes| nodes.iter()),
        options,
        manual_links,
        Framing::Raw,
    );
    // Destination-only annotations emit navigation syntax, never an empty
    // emphasis wrapper that can alter the adjacent phrasing delimiters.
    if segments
        .iter()
        .any(|nodes| mant_ir::has_printable_character(nodes))
    {
        InlinePiece::styled(rendered, markers.0, markers.1)
    } else {
        InlinePiece::plain(rendered)
    }
}

fn coalesce_code_pieces(pieces: Vec<InlinePiece<'_>>) -> Vec<InlinePiece<'_>> {
    let mut output: Vec<InlinePiece<'_>> = Vec::with_capacity(pieces.len());
    for piece in pieces {
        // Invisible anchors, empty styles and empty text roots cannot split
        // an emitted backtick run. Preserve every nonempty syntax/content piece.
        if piece.rendered.is_empty() && piece.code.as_deref().is_none_or(str::is_empty) {
            continue;
        }
        if let Some(value) = piece.code.as_deref()
            && let Some(previous) = output.last_mut().and_then(|piece| piece.code.as_mut())
        {
            previous.to_mut().push_str(value);
        } else {
            output.push(piece);
        }
    }
    for piece in &mut output {
        if let Some(value) = piece.code.take() {
            // Select delimiters once for the complete accepted code run.
            // Hard rows split code spans; no source separator is inserted.
            piece.rendered = code_rows(&value);
        }
    }
    output
}

fn code_rows(value: &str) -> String {
    // Inline::Code may own multiple logical hard rows, unlike the single
    // commonmark_code_span syntax helper. Keep that enhanced reading policy;
    // canonicalize CR/CRLF before splitting, so a CRLF cannot leave an extra
    // code-space at the end of its first row.
    let value = crate::text_safety::markdown_code_text(value, false);
    if value.is_empty() {
        return String::new();
    }
    if !value.contains('\n') {
        return code_span(&value);
    }
    let mut output = String::with_capacity(value.len());
    for (index, row) in value.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
        }
        if !row.is_empty() {
            super::code::append_code_span(&mut output, row);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::{Event, Parser, Tag};

    #[test]
    fn forced_unstyled_target_admission_keeps_literal_labels_without_new_links() {
        for (uri, expected_target) in [
            ("https://a.test/a%20b", Some("https://a.test/a%20b")),
            ("https://a.test/a%1Bb", Some("https://a.test/a%1Bb")),
            ("https://a.test/a b", None),
            ("https://a.test/\x0c", None),
            ("https://a.test/\u{85}", None),
        ] {
            let original = vec![
                Inline::Text { value: "!".into() },
                Inline::Strong {
                    children: vec![Inline::Link {
                        target: mant_ir::LinkTarget::External { uri: uri.into() },
                        title: Some("original".into()),
                        children: vec![Inline::Text {
                            value: "<https://other.test>".into(),
                        }],
                    }],
                },
                Inline::Code {
                    value: "END`".into(),
                },
            ];
            let frozen = original.clone();
            let mut markdown = render_unstyled(original.iter(), MarkdownOptions::default(), false);
            for _ in 0..2 {
                assert!(
                    !Parser::new(&markdown)
                        .any(|event| matches!(event, Event::Start(Tag::Image { .. })))
                );
                let document = crate::parse_markdown(&markdown, None).unwrap().document;
                let [mant_ir::Block::Paragraph { children, .. }] = document.blocks.as_slice()
                else {
                    panic!("one fallback owner")
                };
                assert_eq!(
                    mant_ir::inline_plain_text(children),
                    "!<https://other.test>END`"
                );
                let links = Parser::new(&markdown)
                    .filter_map(|event| match event {
                        Event::Start(Tag::Link {
                            dest_url, title, ..
                        }) => Some((dest_url.into_string(), title.into_string())),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    links,
                    expected_target
                        .map(|target| (target.to_owned(), "original".to_owned()))
                        .into_iter()
                        .collect::<Vec<_>>()
                );
                assert!(
                    Parser::new(&markdown).any(
                        |event| matches!(event, Event::Code(value) if value.as_ref() == "END`")
                    )
                );
                markdown = super::super::render_inline(children, MarkdownOptions::default());
            }
            assert_eq!(original, frozen);
        }
    }

    #[test]
    fn forced_unstyled_recovery_uses_decoded_admission_and_safe_metadata() {
        for (raw, normalized) in [
            ("\r\n", "\r\n"),
            ("\x0c", " "),
            ("\x1b", " "),
            ("\u{81}", "  "),
        ] {
            let source = [Inline::Strong {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://a.test/&amp;".into(),
                    },
                    title: Some(format!("before{raw}after\\")),
                    children: vec![Inline::Text {
                        value: format!("a{raw}b"),
                    }],
                }],
            }];
            let markdown = render_unstyled_framed(
                source.iter(),
                MarkdownOptions::default(),
                false,
                Framing::Rows {
                    manual_links: false,
                    interruptible: false,
                },
            );
            assert!(
                markdown
                    .chars()
                    .all(|character| !character.is_control() || matches!(character, '\n' | '\t'))
            );
            let parsed = crate::parse_markdown(&markdown, None).unwrap();
            assert_eq!(parsed.document.diagnostics, []);
            let [mant_ir::Block::Paragraph { children, .. }] = parsed.document.blocks.as_slice()
            else {
                panic!("paragraph")
            };
            let [
                Inline::Link {
                    target,
                    title,
                    children,
                },
            ] = children.as_slice()
            else {
                panic!("original link remains")
            };
            assert_eq!(
                target,
                &mant_ir::LinkTarget::External {
                    uri: "https://a.test/&amp;".into()
                }
            );
            assert_eq!(
                title.as_deref(),
                Some(format!("before{normalized}after\\").as_str())
            );
            assert_eq!(
                mant_ir::inline_plain_text(children),
                format!("a{normalized}b")
            );
        }
    }

    #[test]
    fn unstyled_fallback_reencodes_authored_markers_code_and_link_identities() {
        let link = |label: &str| Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.test/%E4%B8%AD(a)".into(),
            },
            title: Some("original owner".into()),
            children: vec![Inline::Text {
                value: label.into(),
            }],
        };
        let nodes = [
            Inline::Strong {
                children: vec![Inline::Text { value: "**".into() }],
            },
            Inline::Emphasis {
                children: vec![
                    Inline::Code {
                        value: "a``".into(),
                    },
                    link("x_y"),
                ],
            },
            Inline::Text { value: "!".into() },
            link("NEXT"),
        ];
        let markdown = render_unstyled(nodes.iter(), MarkdownOptions::default(), false);
        let events = Parser::new(&markdown).collect::<Vec<_>>();
        assert!(!events.iter().any(|event| matches!(
            event,
            Event::Start(Tag::Strong | Tag::Emphasis | Tag::Image { .. })
        )));
        let visible = events
            .iter()
            .filter_map(|event| match event {
                Event::Text(value) | Event::Code(value) => Some(value.as_ref()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(visible, "**a``x_y!NEXT");
        let owners = events
            .iter()
            .filter_map(|event| {
                if let Event::Start(Tag::Link {
                    dest_url, title, ..
                }) = event
                {
                    Some((dest_url.as_ref(), title.as_ref()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            owners,
            [("https://example.test/%E4%B8%AD(a)", "original owner"); 2]
        );
        let parsed = crate::parse_markdown(&markdown, None).unwrap();
        let [mant_ir::Block::Paragraph { children, .. }] = parsed.document.blocks.as_slice() else {
            panic!("one fallback phrasing owner")
        };
        assert_eq!(mant_ir::inline_plain_text(children), "**a``x_y!NEXT");
    }
}
