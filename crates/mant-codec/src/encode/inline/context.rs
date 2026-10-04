//! One borrowed phrasing stream for source-owned fragments and transparent labels.

mod rendered;
mod rows;
mod styles;

use std::borrow::Cow;

use mant_ir::{Inline, InlineContentRef};

use super::links::{PhrasingNode, PhrasingSource, phrasing_nodes, render_link};
use super::{MarkdownOptions, code_span, escape_text, html_anchors};
pub(super) use rendered::{PieceKind, RenderedInline};
pub(super) use rows::RowCursor;
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
    kind: PieceKind,
}

impl<'source> InlinePiece<'source> {
    fn plain(rendered: String) -> Self {
        Self {
            rendered,
            markers: None,
            styled: false,
            code: None,
            kind: PieceKind::Content,
        }
    }

    fn styled(rendered: String, primary: &'static str, alternate: &'static str) -> Self {
        let styled = !rendered.trim_matches([' ', '\t', '\n']).is_empty();
        Self {
            rendered,
            markers: Some(StyleMarkers { primary, alternate }),
            styled,
            code: None,
            kind: PieceKind::Content,
        }
    }

    fn code(value: &'source str) -> Self {
        Self {
            rendered: String::new(),
            markers: None,
            styled: false,
            code: Some(Cow::Borrowed(value)),
            kind: PieceKind::Content,
        }
    }

    fn padding(rendered: String) -> Self {
        Self {
            kind: PieceKind::Layout,
            ..Self::plain(rendered)
        }
    }

    fn navigation(rendered: String) -> Self {
        Self {
            kind: PieceKind::Navigation,
            ..Self::plain(rendered)
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

pub(super) fn render_inline_raw_with_cursor<'source>(
    nodes: &'source [Inline],
    options: MarkdownOptions,
    manual_links: bool,
    cursor: &mut RowCursor<'source>,
) -> RenderedInline {
    render_inline_raw_segments_with_cursor(&[nodes], options, manual_links, cursor)
}

pub(super) fn render_inline_raw_segments(
    segments: &[&[Inline]],
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_raw_segments_with_cursor(
        segments,
        options,
        manual_links,
        &mut RowCursor::default(),
    )
    .text
}

pub(super) fn render_inline_raw_content_segments(
    segments: &[InlineContentRef<'_>],
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_raw_sources(
        segments.iter().flat_map(|root| {
            std::iter::once(PhrasingSource::Owner(root.layout))
                .chain(root.content.iter().map(PhrasingSource::Node))
        }),
        options,
        manual_links,
        &mut RowCursor::default(),
    )
    .text
}

pub(super) fn render_inline_raw_owner_nodes(
    roots: &[super::InlineRootNodes<'_>],
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_raw_sources(
        roots.iter().flat_map(|root| {
            std::iter::once(PhrasingSource::Owner(root.layout))
                .chain(root.nodes.iter().copied().map(PhrasingSource::Node))
        }),
        options,
        manual_links,
        &mut RowCursor::default(),
    )
    .text
}

fn render_inline_raw_segments_with_cursor<'source>(
    segments: &[&'source [Inline]],
    options: MarkdownOptions,
    manual_links: bool,
    cursor: &mut RowCursor<'source>,
) -> RenderedInline {
    render_inline_raw_sources(
        segments
            .iter()
            .flat_map(|nodes| nodes.iter())
            .map(PhrasingSource::Node),
        options,
        manual_links,
        cursor,
    )
}

pub(super) fn render_inline_raw_nodes<'source>(
    nodes: impl Iterator<Item = &'source Inline>,
    options: MarkdownOptions,
    manual_links: bool,
) -> String {
    render_inline_raw_sources(
        nodes.map(PhrasingSource::Node),
        options,
        manual_links,
        &mut RowCursor::default(),
    )
    .text
}

fn render_inline_raw_sources<'source>(
    nodes: impl Iterator<Item = PhrasingSource<'source>>,
    options: MarkdownOptions,
    manual_links: bool,
    cursor: &mut RowCursor<'source>,
) -> RenderedInline {
    render_inline_pieces(&mut coalesce_code_pieces(inline_pieces(
        nodes,
        options,
        manual_links,
        cursor,
    )))
}

fn inline_pieces<'source>(
    nodes: impl Iterator<Item = PhrasingSource<'source>>,
    options: MarkdownOptions,
    manual_links: bool,
    cursor: &mut RowCursor<'source>,
) -> Vec<InlinePiece<'source>> {
    let mut nodes = phrasing_nodes(nodes, options, manual_links).peekable();
    let mut pieces = Vec::new();
    while let Some(child) = nodes.next() {
        match child {
            PhrasingNode::Owner(layout) => cursor.owner(layout),
            PhrasingNode::Node(Inline::Text { value }) => {
                // AST text segmentation must not change delimiter decisions.
                // Merge only transparent text siblings: crossing a style or
                // link would ignore real emitted Markdown punctuation.
                let mut text = Cow::Borrowed(value.as_str());
                while let Some(PhrasingNode::Node(Inline::Text { value })) = nodes.peek() {
                    text.to_mut().push_str(value);
                    nodes.next();
                }
                append_text_rows(&mut pieces, &text, cursor);
            }
            PhrasingNode::Node(Inline::Strong {
                children: styled_children,
            }) => {
                let rendered = if matches!(
                    nodes.peek(),
                    Some(PhrasingNode::Node(Inline::Strong { .. }))
                ) {
                    let mut segments = vec![styled_children.as_slice()];
                    while let Some(PhrasingNode::Node(Inline::Strong { children })) = nodes.peek() {
                        segments.push(children.as_slice());
                        nodes.next();
                    }
                    render_inline_raw_segments_with_cursor(&segments, options, manual_links, cursor)
                } else {
                    render_inline_raw_with_cursor(styled_children, options, manual_links, cursor)
                };
                append_annotation(&mut pieces, &rendered, Some(("**", "__")));
            }
            PhrasingNode::Node(Inline::Emphasis {
                children: styled_children,
            }) => {
                let rendered = if matches!(
                    nodes.peek(),
                    Some(PhrasingNode::Node(Inline::Emphasis { .. }))
                ) {
                    let mut segments = vec![styled_children.as_slice()];
                    while let Some(PhrasingNode::Node(Inline::Emphasis { children })) = nodes.peek()
                    {
                        segments.push(children.as_slice());
                        nodes.next();
                    }
                    render_inline_raw_segments_with_cursor(&segments, options, manual_links, cursor)
                } else {
                    render_inline_raw_with_cursor(styled_children, options, manual_links, cursor)
                };
                append_annotation(&mut pieces, &rendered, Some(("*", "_")));
            }
            PhrasingNode::Node(Inline::Code { value } | Inline::Equation { value, .. }) => {
                append_code_rows(&mut pieces, value, cursor);
            }
            PhrasingNode::Link {
                destination,
                title,
                children,
            } => append_annotation(
                &mut pieces,
                &render_link(&destination, title, children, options, manual_links, cursor),
                None,
            ),
            PhrasingNode::Node(Inline::Anchor {
                id,
                fragment_aliases,
                ..
            }) if options.preserve_anchors => {
                pieces.push(InlinePiece::navigation(html_anchors(id, fragment_aliases)));
            }
            PhrasingNode::Node(Inline::Anchor { .. }) => {}
            PhrasingNode::Node(Inline::LineBreak {}) => {
                cursor.newline();
                pieces.push(InlinePiece::plain("\n".into()));
            }
            PhrasingNode::Node(Inline::Link { .. }) => {
                unreachable!("phrasing traversal resolves every link wrapper")
            }
        }
    }
    pieces
}

fn append_annotation(
    pieces: &mut Vec<InlinePiece<'_>>,
    rendered: &RenderedInline,
    markers: Option<(&'static str, &'static str)>,
) {
    if !rendered.has_padding() && !rendered.navigation_only() {
        pieces.push(markers.map_or_else(
            || InlinePiece::plain(rendered.text.clone()),
            |(primary, alternate)| InlinePiece::styled(rendered.text.clone(), primary, alternate),
        ));
        return;
    }
    rendered.for_each_part(|part, kind| {
        pieces.push(match kind {
            PieceKind::Layout => InlinePiece::padding(part.into()),
            PieceKind::Navigation => InlinePiece::navigation(part.into()),
            PieceKind::Content => markers.map_or_else(
                || InlinePiece::plain(part.into()),
                |(primary, alternate)| InlinePiece::styled(part.into(), primary, alternate),
            ),
        });
    });
}

fn append_text_rows(pieces: &mut Vec<InlinePiece<'_>>, value: &str, cursor: &mut RowCursor<'_>) {
    for (index, row) in value.split('\n').enumerate() {
        if index > 0 {
            cursor.newline();
            pieces.push(InlinePiece::plain("\n".into()));
        }
        let padding = if row.trim_matches([' ', '\t']).is_empty() {
            String::new()
        } else {
            cursor.visible()
        };
        pieces.push(InlinePiece::padding(padding));
        pieces.push(InlinePiece::plain(escape_text(row)));
    }
}

fn append_code_rows<'a>(
    pieces: &mut Vec<InlinePiece<'a>>,
    value: &'a str,
    cursor: &mut RowCursor<'_>,
) {
    for (index, row) in value.split('\n').enumerate() {
        if index > 0 {
            cursor.newline();
            pieces.push(InlinePiece::plain("\n".into()));
        }
        if !row.is_empty() {
            pieces.push(InlinePiece::padding(cursor.visible()));
            pieces.push(InlinePiece::code(row));
        }
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
            // Source hard rows already separate pieces. Select delimiters
            // once for the complete accepted code run on this row.
            piece.rendered = code_span(&value);
        }
    }
    output
}
