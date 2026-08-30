//! Converts renderer-neutral inline nodes to safe `CommonMark` phrasing.

use std::collections::VecDeque;

use mant_ir::{Inline, LinkTarget};

use super::MarkdownOptions;

pub(super) fn render_inline(children: &[Inline], options: MarkdownOptions) -> String {
    render_inline_raw(children, options)
        .split('\n')
        .map(|line| line.trim_matches([' ', '\t']))
        .filter(|line| !line.is_empty())
        .map(protect_block_prefix)
        .collect::<Vec<_>>()
        .join("  \n")
}

pub(super) fn flatten_inline(children: &[Inline]) -> String {
    let mut output = String::new();
    for child in children {
        match child {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                output.push_str(&flatten_inline(children));
            }
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
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
    let boundary = if value.ends_with('\n') { "" } else { "\n" };
    format!("{fence}{language}\n{value}{boundary}{fence}")
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

struct InlinePiece {
    rendered: String,
    markers: Option<StyleMarkers>,
    styled: bool,
}

impl InlinePiece {
    fn plain(rendered: String) -> Self {
        Self {
            rendered,
            markers: None,
            styled: false,
        }
    }

    fn styled(rendered: String, primary: &'static str, alternate: &'static str) -> Self {
        let styled = !rendered.trim_matches([' ', '\t']).is_empty();
        Self {
            rendered,
            markers: Some(StyleMarkers { primary, alternate }),
            styled,
        }
    }

    fn first_output_character(&self) -> Option<char> {
        if self.styled && !self.rendered.starts_with([' ', '\t']) {
            Some('*')
        } else {
            self.rendered.chars().next()
        }
    }

    fn last_output_character(&self) -> Option<char> {
        if self.styled && !self.rendered.ends_with([' ', '\t']) {
            Some('*')
        } else {
            self.rendered.chars().next_back()
        }
    }
}

fn render_inline_raw(nodes: &[Inline], options: MarkdownOptions) -> String {
    let mut pieces = Vec::with_capacity(nodes.len());
    let mut index = 0;
    while let Some(child) = nodes.get(index) {
        match child {
            Inline::Text { value } => pieces.push(InlinePiece::plain(escape_text(value))),
            Inline::Strong {
                children: styled_children,
            } => {
                let mut rendered = render_inline_raw(styled_children, options);
                index += 1;
                while let Some(Inline::Strong { children }) = nodes.get(index) {
                    rendered.push_str(&render_inline_raw(children, options));
                    index += 1;
                }
                pieces.push(InlinePiece::styled(rendered, "**", "__"));
                continue;
            }
            Inline::Emphasis {
                children: styled_children,
            } => {
                let mut rendered = render_inline_raw(styled_children, options);
                index += 1;
                while let Some(Inline::Emphasis { children }) = nodes.get(index) {
                    rendered.push_str(&render_inline_raw(children, options));
                    index += 1;
                }
                pieces.push(InlinePiece::styled(rendered, "*", "_"));
                continue;
            }
            Inline::Code { value } => pieces.push(InlinePiece::plain(code_span(value))),
            Inline::Link {
                target,
                title,
                children,
            } => match target {
                LinkTarget::External { uri } => {
                    pieces.push(InlinePiece::plain(render_link(
                        uri,
                        title.as_deref(),
                        children,
                        options,
                    )));
                }
                LinkTarget::Email { address } => pieces.push(InlinePiece::plain(render_link(
                    &format!("mailto:{address}"),
                    title.as_deref(),
                    children,
                    options,
                ))),
                LinkTarget::Document { name, fragment } => {
                    let mut destination = format!("{name}.md");
                    if let Some(fragment) = fragment {
                        destination.push('#');
                        destination.push_str(fragment);
                    }
                    pieces.push(InlinePiece::plain(render_link(
                        &destination,
                        title.as_deref(),
                        children,
                        options,
                    )));
                }
                LinkTarget::Section { id } if options.preserve_anchors => {
                    pieces.push(InlinePiece::plain(render_link(
                        &format!("#{id}"),
                        title.as_deref(),
                        children,
                        options,
                    )));
                }
                LinkTarget::Manual { .. } | LinkTarget::Section { .. } => {
                    pieces.push(InlinePiece::plain(render_inline_raw(children, options)));
                }
            },
            Inline::Anchor { id } if options.preserve_anchors => {
                pieces.push(InlinePiece::plain(html_anchor(id)));
            }
            Inline::Anchor { .. } => {}
            Inline::LineBreak => pieces.push(InlinePiece::plain("\n".to_owned())),
        }
        index += 1;
    }
    render_inline_pieces(&mut pieces)
}

fn render_inline_pieces(pieces: &mut [InlinePiece]) -> String {
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

fn nonempty_neighbors(pieces: &[InlinePiece]) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
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
    pieces: &[InlinePiece],
    index: usize,
    preceding: &[Option<usize>],
    following: &[Option<usize>],
) -> bool {
    let piece = &pieces[index];
    let core = piece.rendered.trim_matches([' ', '\t']);
    let before = piece
        .rendered
        .starts_with([' ', '\t'])
        .then_some(' ')
        .or_else(|| preceding[index].and_then(|index| pieces[index].last_output_character()));
    let after = piece
        .rendered
        .ends_with([' ', '\t'])
        .then_some(' ')
        .or_else(|| following[index].and_then(|index| pieces[index].first_output_character()));
    let markers = piece.markers.expect("styled pieces carry markers");
    [markers.primary, markers.alternate]
        .into_iter()
        .any(|marker| !core.contains(marker) && can_delimit_style(core, marker, before, after))
}

fn following_characters(pieces: &[InlinePiece]) -> Vec<Option<char>> {
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
    let core = rendered.trim_matches([' ', '\t']);
    if core.is_empty() {
        return rendered.to_owned();
    }
    let leading_width = rendered.len() - rendered.trim_start_matches([' ', '\t']).len();
    let trailing_width = rendered.len() - rendered.trim_end_matches([' ', '\t']).len();
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
        !core.contains(marker) && can_delimit_style(core, marker, preceding, following)
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
) -> String {
    let label = render_inline_raw(children, options);
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

fn can_delimit_style(
    core: &str,
    marker: &str,
    preceding: Option<char>,
    following: Option<char>,
) -> bool {
    let Some(first) = core.chars().next() else {
        return false;
    };
    let Some(last) = core.chars().next_back() else {
        return false;
    };
    can_open_delimiter(marker, preceding, Some(first))
        && can_close_delimiter(marker, Some(last), following)
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
    let bytes = line.as_bytes();
    let hashes = bytes.iter().take_while(|byte| **byte == b'#').count();
    let insertion = if (hashes > 0 && bytes.get(hashes).is_none_or(u8::is_ascii_whitespace))
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
    };
    insertion.map_or_else(
        || line.to_owned(),
        |width| format!("{}\\{}", &line[..width], &line[width..]),
    )
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
