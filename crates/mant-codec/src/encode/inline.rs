//! Converts renderer-neutral inline nodes to safe `CommonMark` phrasing.

mod code;
mod context;
mod links;
mod text;

use std::collections::HashSet;

use mant_ir::Inline;

use super::MarkdownOptions;
use context::{render_inline_raw_nodes, render_inline_raw_segments};
#[cfg(test)]
use text::escape_plain_text;

pub(crate) use code::code_span;
pub(super) use code::fenced_code;
pub(super) use links::link_destination;
pub(super) use text::{block_prefix_escape_position, protect_block_prefix};
pub(crate) use text::{escape_text, html_anchor, html_anchors};

pub(crate) fn render_inline(children: &[Inline], options: MarkdownOptions) -> String {
    render_inline_segments(&[children], options)
}

/// Encode source-owned fragments with one delimiter and escaping context.
/// A Joined seam belongs to the same inline stream even when its IR roots
/// remain separate for semantic ownership or report decoration.
pub(super) fn render_inline_segments(segments: &[&[Inline]], options: MarkdownOptions) -> String {
    render_inline_content_segments(segments, options, false)
}

/// Encode borrowed node selections without changing their source roots or
/// splitting the delimiter context at invisible metadata fragments.
pub(super) fn render_inline_node_refs(nodes: &[&Inline], options: MarkdownOptions) -> String {
    render_inline_rows(
        &render_inline_raw_nodes(nodes.iter().copied(), options, false),
        false,
    )
}

pub(super) fn render_heading_inline(children: &[Inline], options: MarkdownOptions) -> String {
    render_inline_content(children, options, true)
}

fn render_inline_content(
    children: &[Inline],
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_content_segments(&[children], options, manual_links)
}

fn render_inline_content_segments(
    segments: &[&[Inline]],
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_rows(
        &render_inline_raw_segments(segments, options, manual_links),
        manual_links,
    )
}

fn render_inline_rows(raw: &str, manual_links: bool) -> String {
    let lines = raw
        .split('\n')
        .map(|line| line.trim_matches([' ', '\t']))
        .map(|line| (!line.is_empty()).then(|| protect_block_prefix(line)))
        .collect::<Vec<_>>();
    let mut output = String::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(line) = line {
            output.push_str(line);
        }
        let Some(next) = lines.get(index + 1) else {
            continue;
        };
        if line.is_some() && next.is_some() {
            output.push_str("  \n");
        } else {
            // CommonMark's two-space form cannot represent a leading, trailing,
            // or consecutive hard break once empty source lines are retained.
            // At a block's first line the tag is an HTML block, not phrasing.
            // Reserve one exact spelling for the reader's narrow hard-row
            // block contract; ordinary raw HTML keeps its source policy.
            output.push_str(if index == 0 && line.is_none() && !manual_links {
                "<br />\n"
            } else {
                "<br>\n"
            });
        }
    }
    output
}

pub(super) fn flatten_inline(children: &[Inline]) -> String {
    let mut output = String::new();
    for child in children {
        match child {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                output.push_str(value);
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                output.push_str(&flatten_inline(children));
            }
            Inline::Anchor { .. } => {}
            Inline::LineBreak { indent_columns } => {
                output.push('\n');
                output
                    .push_str(&" ".repeat(mant_ir::geometry::padding(i32::from(*indent_columns))));
            }
        }
    }
    output
}

/// Fenced code cannot contain active HTML anchors. Project its zero-width
/// destinations immediately before the fence, preserving their source order
/// and aliases while the code text remains one preformatted block.
pub(super) fn preformatted_anchor_markers(children: &[Inline]) -> String {
    let mut markers = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<_> = children.iter().rev().collect();
    while let Some(child) = stack.pop() {
        match child {
            Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } => {
                for target in std::iter::once(id.as_str())
                    .chain(fragment_aliases.iter().map(mant_ir::FragmentAlias::as_str))
                {
                    if seen.insert(target) {
                        markers.push(html_anchor(target));
                    }
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => stack.extend(children.iter().rev()),
            _ => {}
        }
    }
    markers.join("\n")
}

#[cfg(test)]
mod tests {
    use super::escape_plain_text;

    #[test]
    fn markdown_row_padding_is_preserved_without_becoming_source_text() {
        let nodes = vec![
            mant_ir::Inline::Text {
                value: "Alpha".into(),
            },
            mant_ir::Inline::line_break_indented(3),
            mant_ir::Inline::Strong {
                children: vec![mant_ir::Inline::Text {
                    value: "Beta".into(),
                }],
            },
        ];
        assert_eq!(super::flatten_inline(&nodes), "Alpha\n   Beta");
        assert_eq!(
            super::render_inline(&nodes, super::MarkdownOptions::default()),
            "Alpha  \n&#160;&#160;&#160;**Beta**"
        );
        assert_eq!(mant_ir::inline_plain_text(&nodes), "Alpha\nBeta");
    }

    #[test]
    fn preformatted_styles_keep_anchor_order() {
        // Fenced output cannot embed anchors. Transparent style traversal
        // retains the original zero-width destination order before the fence.
        let children = vec![
            mant_ir::Inline::anchor("first"),
            mant_ir::Inline::Strong {
                children: vec![
                    mant_ir::Inline::anchor("second"),
                    mant_ir::Inline::Text {
                        value: "native".into(),
                    },
                ],
            },
        ];
        assert_eq!(
            super::preformatted_anchor_markers(&children),
            "<a id=\"first\"></a>\n<a id=\"second\"></a>"
        );
    }

    #[test]
    fn plain_text_escapes_only_delimiter_capable_underscores() {
        for (source, expected) in [
            ("PATH_SCRIPT", "PATH_SCRIPT"),
            ("a_b", "a_b"),
            ("路径_脚本", "路径_脚本"),
            ("_leading", "\\_leading"),
            ("trailing_", "trailing\\_"),
            ("a__b", "a\\_\\_b"),
        ] {
            assert_eq!(escape_plain_text(source), expected, "{source}");
        }
    }

    #[test]
    fn plain_text_escapes_literal_backticks() {
        assert_eq!(
            escape_plain_text("`bold' and ```"),
            "\\`bold' and \\`\\`\\`"
        );
    }
}
