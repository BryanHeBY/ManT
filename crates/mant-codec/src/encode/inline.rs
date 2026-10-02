//! Converts renderer-neutral inline nodes to safe `CommonMark` phrasing.

use std::{
    borrow::Cow,
    collections::{HashSet, VecDeque},
};

use mant_ir::{Inline, LinkTarget};

use super::MarkdownOptions;

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

pub(crate) fn escape_text(value: &str) -> String {
    let mut output = String::new();
    let mut remainder = value;
    while let Some((start, opening_width)) = find_angle_url(remainder) {
        output.push_str(&escape_plain_text(&remainder[..start]));
        let after_open = &remainder[start + opening_width..];
        let closing = if opening_width == 2 { ">>" } else { ">" };
        let Some(end) = after_open.find(closing) else {
            output.push_str(&escape_plain_text(&remainder[start..]));
            return output;
        };
        let url = &after_open[..end];
        if url.chars().any(char::is_whitespace) || url.contains(['<', '>']) {
            output.push_str(&escape_plain_text(&remainder[start..start + opening_width]));
            remainder = after_open;
            continue;
        }
        output.push('<');
        output.push_str(url);
        output.push('>');
        remainder = &after_open[end + closing.len()..];
    }
    output.push_str(&escape_plain_text(remainder));
    output
}

pub(super) fn fenced_code(value: &str, language: Option<&str>) -> String {
    let width = longest_backtick_run(value).saturating_add(1).max(3);
    let fence = "`".repeat(width);
    let language = language
        .map(|language| {
            language
                .chars()
                .filter(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '_')
                })
                .collect::<String>()
        })
        .filter(|language| !language.is_empty())
        .unwrap_or_default();
    // The reader removes exactly the framing newline before the closing
    // fence (markdown/layout.rs::trim_code_framing_newline). Give that
    // syntax its own byte; an authored final hard row belongs to the value.
    format!("{fence}{language}\n{value}\n{fence}")
}

pub(crate) fn code_span(value: &str) -> String {
    let width = longest_backtick_run(value).saturating_add(1).max(1);
    let delimiter = "`".repeat(width);
    let padding = (value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']))
        && !value.chars().all(|character| character == ' ');
    if padding {
        format!("{delimiter} {value} {delimiter}")
    } else {
        format!("{delimiter}{value}{delimiter}")
    }
}

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

fn render_inline_raw(nodes: &[Inline], options: MarkdownOptions, manual_links: bool) -> String {
    render_inline_raw_segments(&[nodes], options, manual_links)
}

fn render_inline_raw_segments(
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

fn render_inline_raw_nodes<'source>(
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

enum PhrasingNode<'source> {
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
fn phrasing_nodes<'source>(
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
            PhrasingNode::Node(Inline::Strong {
                children: styled_children,
            }) => {
                let mut segments = vec![styled_children.as_slice()];
                while let Some(PhrasingNode::Node(Inline::Strong { children })) = nodes.peek() {
                    segments.push(children.as_slice());
                    nodes.next();
                }
                let rendered = render_inline_raw_segments(&segments, options, manual_links);
                pieces.push(InlinePiece::styled(rendered, "**", "__"));
            }
            PhrasingNode::Node(Inline::Emphasis {
                children: styled_children,
            }) => {
                let mut segments = vec![styled_children.as_slice()];
                while let Some(PhrasingNode::Node(Inline::Emphasis { children })) = nodes.peek() {
                    segments.push(children.as_slice());
                    nodes.next();
                }
                let rendered = render_inline_raw_segments(&segments, options, manual_links);
                pieces.push(InlinePiece::styled(rendered, "*", "_"));
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
                pieces.push(InlinePiece::plain(html_anchors(id, fragment_aliases)));
            }
            PhrasingNode::Node(Inline::Anchor { .. }) => {}
            PhrasingNode::Node(Inline::LineBreak { indent_columns }) => {
                // CommonMark collapses ordinary leading spaces or treats
                // them as a code block. These cells are resolved row layout,
                // not authored source text, so use non-breaking entities.
                pieces.push(InlinePiece::plain(format!(
                    "\n{}",
                    "&#160;".repeat(mant_ir::geometry::padding(i32::from(*indent_columns)))
                )));
            }
            PhrasingNode::Node(Inline::Link { .. }) => {
                unreachable!("phrasing traversal resolves every link wrapper")
            }
        }
    }
    pieces
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
    let mut output = String::new();
    for (index, row) in value.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
        }
        if !row.is_empty() {
            output.push_str(&code_span(row));
        }
    }
    output
}

fn render_inline_pieces(pieces: &mut [InlinePiece<'_>]) -> String {
    let (preceding, following) = nonempty_neighbors(pieces);
    let mut pending = pieces
        .iter()
        .enumerate()
        .filter_map(|(index, piece)| {
            (piece.styled && !style_is_valid(pieces, index, &preceding, &following))
                .then_some(index)
        })
        .collect::<VecDeque<_>>();
    while let Some(index) = pending.pop_front() {
        if !pieces[index].styled || style_is_valid(pieces, index, &preceding, &following) {
            continue;
        }
        pieces[index].styled = false;
        for neighbor in [preceding[index], following[index]].into_iter().flatten() {
            if pieces[neighbor].styled {
                pending.push_back(neighbor);
            }
        }
    }

    let following = following_characters(pieces);
    let mut output = String::new();
    for (index, piece) in pieces.iter().enumerate() {
        if let Some(markers) = piece.markers.filter(|_| piece.styled) {
            output.push_str(&render_styled(
                &piece.rendered,
                markers.primary,
                markers.alternate,
                &output,
                following[index],
            ));
        } else {
            output.push_str(&piece.rendered);
        }
    }
    output
}

fn nonempty_neighbors(pieces: &[InlinePiece<'_>]) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
    let mut preceding = vec![None; pieces.len()];
    let mut current = None;
    for (index, piece) in pieces.iter().enumerate() {
        preceding[index] = current;
        if !piece.rendered.is_empty() {
            current = Some(index);
        }
    }
    let mut following = vec![None; pieces.len()];
    current = None;
    for (index, piece) in pieces.iter().enumerate().rev() {
        following[index] = current;
        if !piece.rendered.is_empty() {
            current = Some(index);
        }
    }
    (preceding, following)
}

fn style_is_valid(
    pieces: &[InlinePiece<'_>],
    index: usize,
    preceding: &[Option<usize>],
    following: &[Option<usize>],
) -> bool {
    let piece = &pieces[index];
    let core = piece.rendered.trim_matches([' ', '\t', '\n']);
    let before = piece
        .rendered
        .starts_with([' ', '\t', '\n'])
        .then_some(' ')
        .or_else(|| preceding[index].and_then(|index| pieces[index].last_output_character()));
    let after = piece
        .rendered
        .ends_with([' ', '\t', '\n'])
        .then_some(' ')
        .or_else(|| following[index].and_then(|index| pieces[index].first_output_character()));
    let markers = piece.markers.expect("styled pieces carry markers");
    [markers.primary, markers.alternate]
        .into_iter()
        .any(|marker| {
            style_marker_is_available(core, marker)
                && can_delimit_style(core, marker, before, after)
        })
}

fn following_characters(pieces: &[InlinePiece<'_>]) -> Vec<Option<char>> {
    let mut following = vec![None; pieces.len()];
    let mut current = None;
    for (index, piece) in pieces.iter().enumerate().rev() {
        following[index] = current;
        current = piece.first_output_character().or(current);
    }
    following
}

/// Render one styled span with the ordinary asterisk marker, switching to the
/// equivalent underscore marker when adjacent styles would form an ambiguous
/// run of `*`. This keeps the output pure Markdown while preserving emphasis
/// within ordinary words, where underscore delimiters are intentionally inert.
fn render_styled(
    rendered: &str,
    primary_marker: &str,
    alternate_marker: &str,
    preceding: &str,
    following: Option<char>,
) -> String {
    let core = rendered.trim_matches([' ', '\t', '\n']);
    if core.is_empty() {
        return rendered.to_owned();
    }
    let leading_width = rendered.len() - rendered.trim_start_matches([' ', '\t', '\n']).len();
    let trailing_width = rendered.len() - rendered.trim_end_matches([' ', '\t', '\n']).len();
    let leading = &rendered[..leading_width];
    let trailing = &rendered[rendered.len() - trailing_width..];
    let prefer_alternate = preceding.ends_with('*') || core.contains(primary_marker);
    let markers = if prefer_alternate {
        [alternate_marker, primary_marker]
    } else {
        [primary_marker, alternate_marker]
    };
    let preceding = preceding.chars().next_back();
    let marker = markers.into_iter().find(|marker| {
        style_marker_is_available(core, marker)
            && can_delimit_style(core, marker, preceding, following)
    });
    marker.map_or_else(
        || rendered.to_owned(),
        |marker| format!("{leading}{marker}{core}{marker}{trailing}"),
    )
}

fn render_link(
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
pub(super) fn link_destination(
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

fn can_delimit_style(
    core: &str,
    marker: &str,
    preceding: Option<char>,
    following: Option<char>,
) -> bool {
    // CommonMark classifies an entire delimiter run using the characters
    // outside that run. A generated inner style with the same marker joins
    // the outer run; its marker is not the next content character.
    let marker_character = char::from(marker.as_bytes()[0]);
    let Some(first) = core.trim_start_matches(marker_character).chars().next() else {
        return false;
    };
    let Some(last) = core.trim_end_matches(marker_character).chars().next_back() else {
        return false;
    };
    can_open_delimiter(marker, preceding, Some(first))
        && can_close_delimiter(marker, Some(last), following)
}

fn style_marker_is_available(core: &str, marker: &str) -> bool {
    if !core.contains(marker) {
        return true;
    }
    // A single emphasis marker can enclose generated strong runs. Keep odd
    // inner runs on the alternate marker: two nested single runs would be
    // parsed as strong instead of preserving emphasis.
    if marker.len() != 1 {
        return false;
    }
    let marker_character = char::from(marker.as_bytes()[0]);
    core.split(|character| character != marker_character)
        .filter(|run| !run.is_empty())
        .all(|run| run.len().is_multiple_of(2))
}

fn can_open_delimiter(marker: &str, preceding: Option<char>, following: Option<char>) -> bool {
    let (left_flanking, right_flanking) = delimiter_flanking(preceding, following);
    if marker.starts_with('*') {
        left_flanking
    } else {
        left_flanking && (!right_flanking || preceding.is_some_and(is_commonmark_punctuation))
    }
}

fn can_close_delimiter(marker: &str, preceding: Option<char>, following: Option<char>) -> bool {
    let (left_flanking, right_flanking) = delimiter_flanking(preceding, following);
    if marker.starts_with('*') {
        right_flanking
    } else {
        right_flanking && (!left_flanking || following.is_some_and(is_commonmark_punctuation))
    }
}

fn delimiter_flanking(preceding: Option<char>, following: Option<char>) -> (bool, bool) {
    let preceding_whitespace = preceding.is_none_or(char::is_whitespace);
    let following_whitespace = following.is_none_or(char::is_whitespace);
    let preceding_punctuation = preceding.is_some_and(is_commonmark_punctuation);
    let following_punctuation = following.is_some_and(is_commonmark_punctuation);
    let left_flanking = !following_whitespace
        && (!following_punctuation || preceding_whitespace || preceding_punctuation);
    let right_flanking = !preceding_whitespace
        && (!preceding_punctuation || following_whitespace || following_punctuation);
    (left_flanking, right_flanking)
}

fn is_commonmark_punctuation(character: char) -> bool {
    // CommonMark uses Unicode punctuation and symbol categories. Treating the
    // remaining non-alphanumeric, non-whitespace scalars as punctuation is a
    // conservative superset: an unusual combining mark can lose styling, but
    // it cannot make delimiter bytes visible in the projected text.
    !character.is_alphanumeric() && !character.is_whitespace()
}

pub(crate) fn html_anchor(id: &str) -> String {
    format!("<a id=\"{}\"></a>", escape_html_attribute(id))
}

pub(crate) fn html_anchors(id: &str, aliases: &[mant_ir::FragmentAlias]) -> String {
    std::iter::once(id)
        .chain(
            aliases
                .iter()
                .map(mant_ir::FragmentAlias::as_str)
                .filter(|alias| *alias != id),
        )
        .map(html_anchor)
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_plain_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    let mut previous = None;
    while let Some(character) = characters.next() {
        let intraword_underscore = character == '_'
            && previous.is_some_and(char::is_alphanumeric)
            && characters
                .peek()
                .is_some_and(|character| character.is_alphanumeric());
        if character == '&' {
            // A source entity spelling is prose, not serializer markup.  A
            // bare `&amp;` would be decoded by the next CommonMark consumer and
            // change the text the manual is documenting.  Entity-encoding the
            // ampersand keeps both ordinary `a & b` and literal `&amp;` source
            // text stable after reparsing.
            output.push_str("&amp;");
            previous = Some(character);
            continue;
        }
        if matches!(
            character,
            '\\' | '`' | '*' | '[' | ']' | '<' | '>' | '$' | '~' | '|' | '^' | ':'
        ) || (character == '_' && !intraword_underscore)
        {
            output.push('\\');
        }
        output.push(character);
        previous = Some(character);
    }
    output
}

pub(super) fn protect_block_prefix(line: &str) -> String {
    block_prefix_escape_position(line).map_or_else(
        || line.to_owned(),
        |width| format!("{}\\{}", &line[..width], &line[width..]),
    )
}

pub(super) fn block_prefix_escape_position(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let hashes = bytes.iter().take_while(|byte| **byte == b'#').count();
    if (hashes > 0 && bytes.get(hashes).is_none_or(u8::is_ascii_whitespace))
        || bytes.starts_with(b">")
        || bytes.starts_with(b"- ")
        || bytes.starts_with(b"+ ")
        || bytes.starts_with(b"* ")
        || (!bytes.is_empty() && bytes.iter().all(|byte| *byte == b'-'))
        || (!bytes.is_empty() && bytes.iter().all(|byte| *byte == b'='))
    {
        Some(0)
    } else {
        let digits = bytes
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        (digits > 0
            && bytes
                .get(digits..digits.saturating_add(2))
                .is_some_and(|suffix| matches!(suffix, b". " | b") ")))
        .then_some(digits)
    }
}

fn find_angle_url(value: &str) -> Option<(usize, usize)> {
    [
        ("<<http://", 2),
        ("<<https://", 2),
        ("<http://", 1),
        ("<https://", 1),
    ]
    .into_iter()
    .filter_map(|(needle, width)| value.find(needle).map(|index| (index, width)))
    .min_by_key(|(index, width)| (*index, usize::MAX - *width))
}

fn longest_backtick_run(value: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in value.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
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
