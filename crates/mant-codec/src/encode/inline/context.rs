//! One borrowed phrasing stream for source-owned fragments and transparent labels.

mod styles;

use std::borrow::Cow;

use mant_ir::Inline;

use super::links::{PhrasingNode, phrasing_nodes, render_link};
use super::{MarkdownOptions, code_span, escape_text, html_anchor};
use styles::render_inline_pieces;

#[derive(Clone, Copy)]
struct StyleMarkers {
    primary: &'static str,
    alternate: &'static str,
}

struct InlinePiece<'source> {
    rendered: String,
    markers: Option<StyleMarkers>,
    styled: bool,
    code: Option<Cow<'source, str>>,
}

impl<'source> InlinePiece<'source> {
    fn plain(rendered: String) -> Self {
        Self {
            rendered,
            markers: None,
            styled: false,
            code: None,
        }
    }

    fn styled(rendered: String, primary: &'static str, alternate: &'static str) -> Self {
        let styled = !rendered.trim_matches([' ', '\t', '\n']).is_empty();
        Self {
            rendered,
            markers: Some(StyleMarkers { primary, alternate }),
            styled,
            code: None,
        }
    }

    fn code(value: &'source str) -> Self {
        Self {
            rendered: String::new(),
            markers: None,
            styled: false,
            code: Some(Cow::Borrowed(value)),
        }
    }

    fn first_output_character(&self) -> Option<char> {
        if self.styled && !self.rendered.starts_with([' ', '\t', '\n']) {
            Some('*')
        } else {
            self.rendered.chars().next()
        }
    }

    fn last_output_character(&self) -> Option<char> {
        if self.styled && !self.rendered.ends_with([' ', '\t', '\n']) {
            Some('*')
        } else {
            self.rendered.chars().next_back()
        }
    }
}

pub(super) fn render_inline_raw(
    nodes: &[Inline],
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_raw_segments(&[nodes], options, manual_links)
}

pub(super) fn render_inline_raw_segments(
    segments: &[&[Inline]],
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_raw_nodes(
        segments.iter().flat_map(|nodes| nodes.iter()),
        options,
        manual_links,
    )
}

pub(super) fn render_inline_raw_nodes<'source>(
    nodes: impl Iterator<Item = &'source Inline>,
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_pieces(&mut coalesce_code_pieces(inline_pieces(
        nodes,
        options,
        manual_links,
    )))
}

fn inline_pieces<'source>(
    nodes: impl Iterator<Item = &'source Inline>,
    options: MarkdownOptions,
    manual_links: bool,
) -> Vec<InlinePiece<'source>> {
    let mut nodes = phrasing_nodes(nodes, options, manual_links).peekable();
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
            } => pieces.push(InlinePiece::plain(render_link(
                &destination,
                title,
                children,
                options,
                manual_links,
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
    let rendered = render_inline_raw_segments(segments, options, manual_links);
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
    if value.is_empty() {
        return String::new();
    }
    if !value.contains('\n') {
        return code_span(value);
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
