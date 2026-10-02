//! Typed destinations and transparent label traversal for Markdown links.

use std::borrow::Cow;

use mant_ir::{Inline, LinkTarget};

use super::context::render_inline_raw;
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
            if let Inline::Link {
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

pub(super) fn render_link(
    target: &str,
    title: Option<&str>,
    children: &[Inline],
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    let label = render_inline_raw(children, options, manual_links);
    if (target.starts_with("http://") || target.starts_with("https://"))
        && flatten_inline(children) == target
        && !target.chars().any(char::is_whitespace)
        && !target.contains(['<', '>'])
    {
        return format!("<{target}>");
    }
    let target = target
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace(' ', "%20");
    title.map_or_else(
        || format!("[{label}]({target})"),
        |title| format!("[{label}]({target} \"{}\")", title.replace('"', "\\\"")),
    )
}

/// Decide wrapper policy once while preserving the typed destination.
/// No target is recovered from visible label text.
pub(in crate::encode) fn link_destination(
    target: &LinkTarget,
    options: MarkdownOptions,
    manual_links: bool,
) -> Option<Cow<'_, str>> {
    match target {
        // Keep existing external-source representation policy; invalid source
        // references remain visible and diagnosed rather than silently erased.
        LinkTarget::External { uri } => Some(Cow::Borrowed(uri.as_str())),
        LinkTarget::Manual { .. } if !manual_links => None,
        LinkTarget::Section { .. } if !options.preserve_anchors => None,
        _ => target.to_uri().map(Cow::Owned),
    }
}
