//! Preserve literal text and zero-width HTML destinations in Markdown syntax.

pub(crate) fn html_anchor(id: &str) -> String {
    format!("<a id=\"{}\"></a>", escape_html_attribute(id))
}

pub(in crate::encode) fn render_html_anchors<'a>(
    fragments: impl Iterator<Item = &'a str>,
    anchor: impl FnMut(&str) -> Option<String>,
) -> String {
    let mut seen = std::collections::HashSet::new();
    fragments
        .filter(|fragment| seen.insert(*fragment))
        .filter_map(anchor)
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_html_attribute(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '"' => output.push_str("&quot;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '\r' => output.push_str("&#13;"),
            '\n' => output.push_str("&#10;"),
            '\t' => output.push_str("&#9;"),
            character if character.is_control() => {
                output.extend(std::iter::repeat_n(' ', character.len_utf8()));
            }
            _ => output.push(character),
        }
    }
    output
}

pub(crate) fn escape_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    append_plain_text(&mut output, value);
    output
}

fn append_plain_text(output: &mut String, value: &str) {
    let mut characters = value.chars().peekable();
    let mut previous = None;
    while let Some(character) = characters.next() {
        if !crate::text_safety::is_safe_source_character(character) {
            output.extend(std::iter::repeat_n(' ', character.len_utf8()));
            previous = Some(' ');
            continue;
        }
        if character == '\r' {
            // CR decoded from an entity is content, not a new Markdown
            // source line. LF alone participates in hard-row framing.
            output.push_str("&#13;");
            previous = Some(character);
            continue;
        }
        if character == '\u{feff}' {
            // Decoded BOM content must not turn back into the source BOM
            // when a public inline fragment starts with this scalar.
            output.push_str("&#65279;");
            previous = Some(character);
            continue;
        }
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
}

pub(in crate::encode) fn protect_block_prefix(line: &str) -> String {
    block_prefix_escape_position(line).map_or_else(
        || line.to_owned(),
        |width| format!("{}\\{}", &line[..width], &line[width..]),
    )
}

pub(in crate::encode) fn block_prefix_escape_position(line: &str) -> Option<usize> {
    let line = line
        .split_once('\n')
        .map_or(line, |(first, _)| first)
        .trim_end_matches([' ', '\t']);
    let bytes = line.as_bytes();
    let marker = *bytes.first()?;
    let hashes = bytes.iter().take_while(|byte| **byte == b'#').count();
    if (hashes > 0 && bytes.get(hashes).is_none_or(u8::is_ascii_whitespace))
        || marker == b'>'
        || (matches!(marker, b'-' | b'+' | b'*')
            && bytes.get(1).copied().is_none_or(block_separator))
        || is_underline(bytes, marker)
    {
        Some(0)
    } else {
        let digits = bytes
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        ((1..=9).contains(&digits)
            && matches!(bytes.get(digits), Some(b'.' | b')'))
            && bytes.get(digits + 1).copied().is_none_or(block_separator))
        .then_some(digits)
    }
}

fn block_separator(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

fn is_underline(bytes: &[u8], marker: u8) -> bool {
    // An unbroken run can be a Setext underline. Three thematic markers
    // may also be separated by spaces or tabs, without becoming a list.
    (matches!(marker, b'-' | b'=') && bytes.iter().all(|&byte| byte == marker))
        || (matches!(marker, b'-' | b'*' | b'_')
            && bytes
                .iter()
                .filter(|&&byte| byte == marker)
                .nth(2)
                .is_some()
            && bytes
                .iter()
                .all(|&byte| byte == marker || block_separator(byte)))
}

#[cfg(test)]
mod tests {
    use super::escape_text;
    use pulldown_cmark::{Event, Parser, Tag};

    #[test]
    fn text_angle_urls_are_literal_even_when_repeated_or_incomplete() {
        for source in [
            "<http://a.test>",
            "<<https://a.test>>",
            "<<<http://a.test>>",
            "<https://a.test> <<http://b.test>>",
            "<http://a test>",
            "<http://a\u{a0}test>",
            "<http://a<test>",
            "<<https://unclosed",
        ] {
            let encoded = escape_text(source);
            let mut visible = String::new();
            for event in Parser::new(&encoded) {
                assert!(!matches!(
                    event,
                    Event::Start(Tag::Link { .. } | Tag::Image { .. })
                ));
                if let Event::Text(text) = event {
                    visible.push_str(&text);
                }
            }
            assert_eq!(visible, source);
        }
        let source = "<http://example.test> ".repeat(4096);
        let encoded = escape_text(&source);
        assert_eq!(encoded.matches("\\<").count(), 4096);
        assert!(
            !Parser::new(&encoded).any(|event| matches!(event, Event::Start(Tag::Link { .. })))
        );
    }
}
