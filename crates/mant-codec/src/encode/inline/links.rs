//! Typed destinations and transparent label traversal for Markdown links.

use std::borrow::Cow;

use mant_ir::{Inline, LinkTarget};

use super::context::{LinkMeaning, link_meaning, matches_link, render_link_label};
use super::{MarkdownOptions, flatten_inline};

pub(super) enum PhrasingNode<'source> {
    Node(&'source Inline),
    Link {
        destination: CheckedDestination,
        title: Option<&'source str>,
        children: &'source [Inline],
    },
}

/// A reusable URI admitted by the shared typed-target boundary. Label bytes
/// cannot construct this proof, and serialization never repairs its contents.
pub(in crate::encode) struct CheckedDestination(String);

impl CheckedDestination {
    fn from_target(target: &LinkTarget) -> Option<Self> {
        target.to_uri().map(Self)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

enum LinkPresentation {
    Wrapped(CheckedDestination),
    OmittedByPolicy,
    Invalid,
}

/// URI-valued metadata is an explicit semantic caller, unlike ordinary Text.
/// Reuse the same typed admission and metadata serializer as retained links.
pub(in crate::encode) fn render_uri(value: &str) -> Option<String> {
    let target = LinkTarget::from_uri(value);
    let destination = CheckedDestination::from_target(&target)?;
    Some(
        render_link(
            &destination,
            None,
            &[Inline::Text {
                value: value.into(),
            }],
            MarkdownOptions::default(),
            true,
            false,
        )
        .source,
    )
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
    target: &CheckedDestination,
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

fn encode_link(
    target: &CheckedDestination,
    title: Option<&str>,
    label: &str,
    children: &[Inline],
) -> EncodedLink {
    let destination = target.as_str();
    let normalized_title = title.map(|title| {
        crate::text_safety::mask_terminal_controls(title)
            .0
            .map_or(Cow::Borrowed(title), Cow::Owned)
    });
    let title = normalized_title.as_deref();
    let meaning = link_meaning(label, destination, title);
    if title.is_none()
        && !children.is_empty()
        && children
            .iter()
            .all(|node| matches!(node, Inline::Text { .. }))
        && (destination.starts_with("http://") || destination.starts_with("https://"))
        && flatten_inline(children) == destination
    {
        return EncodedLink {
            source: format!("<{destination}>"),
            meaning,
        };
    }
    let mut source = String::with_capacity(label.len() + destination.len() + 4);
    source.push('[');
    source.push_str(label);
    source.push_str("](");
    append_metadata(&mut source, destination, false);
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
) -> Option<CheckedDestination> {
    match link_presentation(target, options, manual_links) {
        LinkPresentation::Wrapped(destination) => Some(destination),
        LinkPresentation::OmittedByPolicy | LinkPresentation::Invalid => None,
    }
}

fn link_presentation(
    target: &LinkTarget,
    options: MarkdownOptions,
    manual_links: bool,
) -> LinkPresentation {
    if matches!(target, LinkTarget::Manual { .. }) && !manual_links
        || matches!(target, LinkTarget::Section { .. }) && !options.preserve_anchors
    {
        // Policy omits this wrapper regardless of validity. Do not allocate
        // a URI/proof that will never participate in the serialized output.
        return LinkPresentation::OmittedByPolicy;
    }
    let Some(destination) = CheckedDestination::from_target(target) else {
        // Invalid target facts and diagnostics belong to the original IR;
        // transparent labels preserve content without inventing another URI.
        return LinkPresentation::Invalid;
    };
    LinkPresentation::Wrapped(destination)
}

#[cfg(test)]
mod tests {
    #[test]
    fn direct_destination_writes_keep_original_escapes_percent_unicode_and_title() {
        let children = [mant_ir::Inline::Text {
            value: "label".into(),
        }];
        let uri = "https://example.test/%E4%B8%AD(a)%20%25%5Ctail";
        let target = super::CheckedDestination::from_target(&mant_ir::LinkTarget::External {
            uri: uri.into(),
        })
        .expect("the source URI is valid before serialization");
        let markdown = super::render_link(
            &target,
            Some("say \"中\""),
            &children,
            super::MarkdownOptions::default(),
            true,
            false,
        );
        let markdown = markdown.source;
        assert_eq!(
            markdown,
            r#"[label](https://example.test/%E4%B8%AD\(a\)%20%25%5Ctail "say \"中\"")"#
        );
        let links: Vec<_> = pulldown_cmark::Parser::new(&markdown)
            .filter_map(|event| match event {
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
                    dest_url, title, ..
                }) => Some((dest_url.into_string(), title.into_string())),
                _ => None,
            })
            .collect();
        assert_eq!(links, [(uri.into(), "say \"中\"".into())]);
    }
}
