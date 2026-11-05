//! Typed destinations and transparent label traversal for Markdown links.

use std::borrow::Cow;

use mant_ir::{Inline, LinkTarget};

use super::context::{LinkMeaning, link_meaning, matches_link, render_link_label};
use super::{MarkdownOptions, flatten_inline};

pub(super) enum PhrasingNode<'source> {
    Node(&'source Inline),
    Link {
        destination: Cow<'source, str>,
        title: Option<&'source str>,
        children: &'source [Inline],
    },
}

/// A link whose target policy emits no wrapper is transparent phrasing.
/// Borrow its children into the current stream before any delimiter is
/// chosen, preserving source order and visible link boundaries.
pub(super) fn phrasing_nodes<'source>(
    mut roots: impl Iterator<Item = &'source Inline>,
    options: MarkdownOptions,
    manual_links: bool,
    omit_styles: bool,
) -> impl Iterator<Item = PhrasingNode<'source>> {
    let mut labels: Vec<std::slice::Iter<'source, Inline>> = Vec::new();
    std::iter::from_fn(move || {
        loop {
            let node = if let Some(label) = labels.last_mut() {
                let Some(node) = label.next() else {
                    labels.pop();
                    continue;
                };
                node
            } else {
                roots.next()?
            };
            if omit_styles && let Inline::Strong { children } | Inline::Emphasis { children } = node
            {
                labels.push(children.iter());
            } else if let Inline::Link {
                target,
                title,
                children,
            } = node
            {
                if let Some(destination) = link_destination(target, options, manual_links) {
                    return Some(PhrasingNode::Link {
                        destination,
                        title: title.as_deref(),
                        children,
                    });
                }
                labels.push(children.iter());
            } else {
                return Some(PhrasingNode::Node(node));
            }
        }
    })
}

/// Serialized bytes and the independent source-owned meaning accepted before
/// adding brackets. Final row serialization must check both facts together.
pub(super) struct EncodedLink {
    pub(super) source: String,
    pub(super) meaning: Option<LinkMeaning>,
}

pub(super) fn render_link(
    target: &str,
    title: Option<&str>,
    children: &[Inline],
    options: MarkdownOptions,
    manual_links: bool,
    omit_styles: bool,
) -> EncodedLink {
    let label = render_link_label(children, options, manual_links, omit_styles);
    let candidate = encode_link(target, title, &label, children);
    if candidate
        .meaning
        .as_ref()
        .is_some_and(|meaning| matches_link(meaning, &candidate.source))
    {
        return candidate;
    }
    // '[' changes CommonMark flanking even when accepted raw phrasing was
    // sound. Revisit original IR instead of deleting generated delimiters
    // from the rejected candidate (which may also be authored label text).
    let label = render_link_label(children, options, manual_links, true);
    let mut recovered = encode_link(target, title, &label, &[]);
    if recovered
        .meaning
        .as_ref()
        .is_some_and(|meaning| !matches_link(meaning, &recovered.source))
    {
        // This source has no portable representation. Do not let the outer
        // sequence certify its own bytes as if they were source-owned meaning.
        recovered.meaning = None;
    }
    recovered
}

fn encode_link(target: &str, title: Option<&str>, label: &str, children: &[Inline]) -> EncodedLink {
    // Preserve the existing representation policy for invalid source URIs
    // containing SP. Valid typed destinations retain every original scalar.
    let destination = target.replace(' ', "%20");
    let normalized_title = title.map(|title| {
        crate::text_safety::mask_terminal_controls(title)
            .0
            .map_or(Cow::Borrowed(title), Cow::Owned)
    });
    let title = normalized_title.as_deref();
    let meaning = link_meaning(label, &destination, title);
    if title.is_none()
        && !children.is_empty()
        && children
            .iter()
            .all(|node| matches!(node, Inline::Text { .. }))
        && (target.starts_with("http://") || target.starts_with("https://"))
        && flatten_inline(children) == target
        && !target.chars().any(char::is_whitespace)
        && !target.contains(['<', '>'])
    {
        return EncodedLink {
            source: format!("<{target}>"),
            meaning,
        };
    }
    let mut source = String::with_capacity(label.len() + destination.len() + 4);
    source.push('[');
    source.push_str(label);
    source.push_str("](");
    append_metadata(&mut source, &destination, false);
    if let Some(title) = title {
        source.push(' ');
        source.push('"');
        append_metadata(&mut source, title, true);
        source.push('"');
    }
    source.push(')');
    EncodedLink { source, meaning }
}

/// Metadata is parsed by `CommonMark`, but is never a visible hard row. Encode
/// literal entities once and represent control whitespace with entities, so
/// the visible-row serializer cannot turn title LF/CR into hard-break syntax.
fn append_metadata(output: &mut String, source: &str, title: bool) {
    for character in source.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '\\' => output.push_str("\\\\"),
            '"' if title => output.push_str("\\\""),
            '(' | ')' | '<' | '>' if !title => {
                output.push('\\');
                output.push(character);
            }
            '\n' => output.push_str("&#10;"),
            '\r' => output.push_str("&#13;"),
            '\t' => output.push_str("&#9;"),
            _ => output.push(character),
        }
    }
}

/// Decide wrapper policy once while preserving the typed destination.
/// No target is recovered from visible label text.
pub(in crate::encode) fn link_destination(
    target: &LinkTarget,
    options: MarkdownOptions,
    manual_links: bool,
) -> Option<Cow<'_, str>> {
    let destination = match target {
        // Keep existing external-source representation policy; invalid source
        // references remain visible and diagnosed rather than silently erased.
        LinkTarget::External { uri } => Some(Cow::Borrowed(uri.as_str())),
        LinkTarget::Manual { .. } if !manual_links => None,
        LinkTarget::Section { .. } if !options.preserve_anchors => None,
        _ => target.to_uri().map(Cow::Owned),
    }?;
    // A forbidden target remains invalid in the original IR and receives
    // its ordinary producer/validator diagnostic. Do not wash it into an
    // activatable URI through lossy escaping or normalization. Only the
    // label participates in the safe projection of such a target.
    (!destination.chars().any(char::is_control)).then_some(destination)
}

#[cfg(test)]
mod tests {
    #[test]
    fn direct_destination_writes_keep_original_escapes_percent_unicode_and_title() {
        let children = [mant_ir::Inline::Text {
            value: "label".into(),
        }];
        let markdown = super::render_link(
            "https://example.test/中(a) %25\\tail",
            Some("say \"中\""),
            &children,
            super::MarkdownOptions::default(),
            true,
            false,
        );
        let markdown = markdown.source;
        assert_eq!(
            markdown,
            r#"[label](https://example.test/中\(a\)%20%25\\tail "say \"中\"")"#
        );
        let links: Vec<_> = pulldown_cmark::Parser::new(&markdown)
            .filter_map(|event| match event {
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
                    dest_url, title, ..
                }) => Some((dest_url.into_string(), title.into_string())),
                _ => None,
            })
            .collect();
        assert_eq!(
            links,
            [(
                "https://example.test/中(a)%20%25\\tail".into(),
                "say \"中\"".into()
            )]
        );
    }
}
