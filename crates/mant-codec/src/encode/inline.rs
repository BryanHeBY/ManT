//! Converts renderer-neutral inline nodes to safe `CommonMark` phrasing.

mod code;
mod context;
mod links;
mod text;

use std::{borrow::Cow, collections::HashSet};

use mant_ir::{Inline, InlineContentRef, InlineLayout};

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

pub(crate) fn render_inline_content(
    content: InlineContentRef<'_>,
    options: MarkdownOptions,
) -> String {
    // Ordinary CommonMark carries author content and hard rows. Exceptional
    // reading origins remain on the IR owner and never become text cells.
    render_inline(content.content, options)
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

#[cfg(test)]
pub(super) fn render_heading_inline(children: &[Inline], options: MarkdownOptions) -> String {
    render_inline_content_segments(&[children], options, true)
}

pub(super) fn render_heading_content(
    content: InlineContentRef<'_>,
    options: MarkdownOptions,
) -> String {
    render_inline_content_segments(&[content.content], options, true)
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
    let mut lines = raw.split('\n').map(protect_author_row_edges).peekable();
    let mut output = String::with_capacity(raw.len());
    let mut index = 0;
    while let Some(line) = lines.next() {
        if !line.is_empty() {
            if let Some(position) = block_prefix_escape_position(&line) {
                output.push_str(&line[..position]);
                output.push('\\');
                output.push_str(&line[position..]);
            } else {
                output.push_str(&line);
            }
        }
        let Some(next) = lines.peek() else {
            continue;
        };
        if !line.is_empty() && !next.is_empty() {
            output.push_str("  \n");
        } else {
            // CommonMark's two-space form cannot represent a leading, trailing,
            // or consecutive hard break once empty source lines are retained.
            // At a block's first line the tag is an HTML block, not phrasing.
            // Reserve one exact spelling for the reader's narrow hard-row
            // block contract; ordinary raw HTML keeps its source policy.
            output.push_str(if index == 0 && line.is_empty() && !manual_links {
                "<br />\n"
            } else {
                "<br>\n"
            });
        }
        index += 1;
    }
    output
}

fn protect_author_row_edges(line: &str) -> Cow<'_, str> {
    let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
    let trailing = line.trim_end_matches([' ', '\t']).len().max(leading);
    if leading == 0 && trailing == line.len() {
        return Cow::Borrowed(line);
    }
    let mut output = String::with_capacity(line.len());
    append_author_whitespace(&mut output, &line[..leading]);
    output.push_str(&line[leading..trailing]);
    append_author_whitespace(&mut output, &line[trailing..]);
    Cow::Owned(output)
}

fn append_author_whitespace(output: &mut String, edge: &str) {
    for character in edge.chars() {
        output.push_str(if character == ' ' { "&#32;" } else { "&#9;" });
    }
}

pub(super) fn flatten_inline(children: &[Inline]) -> String {
    mant_ir::inline_plain_text(children)
}

pub(super) fn flatten_inline_content(content: InlineContentRef<'_>) -> String {
    literal_row_layout(&flatten_inline(content.content), content.layout)
}

pub(super) fn literal_row_layout(text: &str, layout: &InlineLayout) -> String {
    if layout.is_empty() {
        return text.to_owned();
    }
    let mut output = String::with_capacity(text.len());
    for (index, row) in text.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
        }
        // An empty open tail or anchor-only root cannot become literal text
        // solely because its addressable row carries a layout correction.
        if !row.is_empty() {
            output.push_str(&" ".repeat(mant_ir::geometry::padding(layout.row_indent(index))));
            output.push_str(row);
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
    fn row_stream_preserves_author_whitespace_edge_breaks_and_block_protection() {
        for (source, body, heading) in [
            ("", "", ""),
            (" \t", "&#32;&#9;", "&#32;&#9;"),
            ("\nX", "<br />\nX", "<br>\nX"),
            ("X\n", "X<br>\n", "X<br>\n"),
            ("X\n\nY", "X<br>\n<br>\nY", "X<br>\n<br>\nY"),
            (
                " X \n\tY ",
                "&#32;X&#32;  \n&#9;Y&#32;",
                "&#32;X&#32;  \n&#9;Y&#32;",
            ),
            (
                "1. X\n# Y\n-",
                "1\\. X  \n\\# Y  \n\\-",
                "1\\. X  \n\\# Y  \n\\-",
            ),
        ] {
            assert_eq!(super::render_inline_rows(source, false), body);
            assert_eq!(super::render_inline_rows(source, true), heading);
        }
    }

    #[test]
    fn ordinary_markdown_omits_reading_padding_while_literal_projection_retains_it() {
        let nodes = vec![
            mant_ir::Inline::Text {
                value: "Alpha".into(),
            },
            mant_ir::Inline::line_break(),
            mant_ir::Inline::Strong {
                children: vec![mant_ir::Inline::Text {
                    value: "Beta".into(),
                }],
            },
        ];
        let layout = mant_ir::InlineLayout {
            row_hints: vec![mant_ir::RowLayoutHint {
                row: 1,
                indent_columns: 3,
            }],
        };
        let content = mant_ir::InlineContentRef {
            content: &nodes,
            layout: &layout,
        };
        assert_eq!(super::flatten_inline_content(content), "Alpha\n   Beta");
        assert_eq!(
            super::render_inline_content(content, super::MarkdownOptions::default()),
            "Alpha  \n**Beta**"
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
