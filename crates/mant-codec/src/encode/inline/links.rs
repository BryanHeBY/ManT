//! Typed destinations and transparent label traversal for Markdown links.

use std::borrow::Cow;

use mant_ir::{Inline, InlineLayout, LinkTarget};

use super::context::{PieceKind, RenderedInline, RowCursor, render_inline_raw_with_cursor};
use super::{MarkdownOptions, flatten_inline};

pub(super) enum PhrasingNode<'source> {
    Owner(&'source InlineLayout),
    Node(&'source Inline),
    Link {
        destination: Cow<'source, str>,
        title: Option<&'source str>,
        children: &'source [Inline],
    },
}

pub(super) enum PhrasingSource<'source> {
    Owner(&'source InlineLayout),
    Node(&'source Inline),
}

/// A link whose target policy emits no wrapper is transparent phrasing.
/// Borrow its children into the current stream before any delimiter is
/// chosen, preserving source order and visible link boundaries.
pub(super) fn phrasing_nodes<'source>(
    mut roots: impl Iterator<Item = PhrasingSource<'source>>,
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
                match roots.next()? {
                    PhrasingSource::Owner(layout) => return Some(PhrasingNode::Owner(layout)),
                    PhrasingSource::Node(node) => node,
                }
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

pub(super) fn render_link<'source>(
    target: &str,
    title: Option<&str>,
    children: &'source [Inline],
    options: MarkdownOptions,
    manual_links: bool,
    cursor: &mut RowCursor<'source>,
) -> RenderedInline {
    let has_row_padding = cursor.pending_padding() > 0;
    let label = render_inline_raw_with_cursor(children, options, manual_links, cursor);
    if (target.starts_with("http://") || target.starts_with("https://"))
        && flatten_inline(children) == target
        && !target.chars().any(char::is_whitespace)
        && !target.contains(['<', '>'])
        && !has_row_padding
    {
        return RenderedInline::plain(format!("<{target}>"));
    }
    if !label.has_padding() {
        return RenderedInline::plain(wrap_link_label(target, title, &label.text));
    }
    let mut output = RenderedInline::default();
    label.for_each_part(|part, kind| {
        if kind == PieceKind::Content {
            // Close the annotation before a generated row prefix. Real hard
            // boundaries remain between the linked source rows, so no layout
            // cell becomes label text or a link activation range.
            let core = part.trim_matches('\n');
            let leading = part.len() - part.trim_start_matches('\n').len();
            let trailing = part.len() - part.trim_end_matches('\n').len();
            output.append(&part[..leading], PieceKind::Content);
            if !core.is_empty() {
                output.append(&wrap_link_label(target, title, core), PieceKind::Content);
            }
            if trailing > 0 && !core.is_empty() {
                output.append(&part[part.len() - trailing..], PieceKind::Content);
            }
        } else {
            output.append(part, kind);
        }
    });
    output
}

fn wrap_link_label(target: &str, title: Option<&str>, label: &str) -> String {
    let mut output = String::with_capacity(label.len() + target.len() + 4);
    output.push('[');
    output.push_str(label);
    output.push_str("](");
    for character in target.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '(' => output.push_str("\\("),
            ')' => output.push_str("\\)"),
            ' ' => output.push_str("%20"),
            _ => output.push(character),
        }
    }
    if let Some(title) = title {
        output.push_str(" \"");
        for character in title.chars() {
            if character == '"' {
                output.push('\\');
            }
            output.push(character);
        }
        output.push('"');
    }
    output.push(')');
    output
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
            &mut super::RowCursor::default(),
        )
        .text;
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
