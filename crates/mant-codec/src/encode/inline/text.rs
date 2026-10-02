//! Preserve literal text and zero-width HTML destinations in Markdown syntax.

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

pub(super) fn escape_plain_text(value: &str) -> String {
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

pub(in crate::encode) fn protect_block_prefix(line: &str) -> String {
    block_prefix_escape_position(line).map_or_else(
        || line.to_owned(),
        |width| format!("{}\\{}", &line[..width], &line[width..]),
    )
}

pub(in crate::encode) fn block_prefix_escape_position(line: &str) -> Option<usize> {
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
