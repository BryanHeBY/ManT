//! Preserve literal text and zero-width HTML destinations in Markdown syntax.

pub(crate) fn escape_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remainder = value;
    let mut closings = [ClosingCursor::default(); 2];
    while let Some((start, opening_width)) = find_angle_url(remainder) {
        append_plain_text(&mut output, &remainder[..start]);
        let after_open = &remainder[start + opening_width..];
        let closing = if opening_width == 2 { ">>" } else { ">" };
        let offset = value.len() - after_open.len();
        let Some(end) = closings[opening_width - 1].at_or_after(value, offset, closing) else {
            append_plain_text(&mut output, &remainder[start..]);
            return output;
        };
        let url = &value[offset..end];
        if url
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '<' | '>'))
        {
            append_plain_text(&mut output, &remainder[start..start + opening_width]);
            remainder = after_open;
            continue;
        }
        output.push('<');
        output.push_str(url);
        output.push('>');
        remainder = &value[end + closing.len()..];
    }
    append_plain_text(&mut output, remainder);
    output
}

/// Invalid openings may share a distant closing delimiter. Each delimiter
/// kind scans the immutable source monotonically, including a missing close.
#[derive(Clone, Copy, Default)]
struct ClosingCursor {
    next: Option<usize>,
    searched: bool,
}

impl ClosingCursor {
    fn at_or_after(&mut self, source: &str, start: usize, delimiter: &str) -> Option<usize> {
        if !self.searched || self.next.is_some_and(|index| index < start) {
            self.next = source[start..].find(delimiter).map(|index| start + index);
            self.searched = true;
        }
        self.next
    }
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
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '"' => output.push_str("&quot;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
    output
}

#[cfg(test)]
pub(super) fn escape_plain_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    append_plain_text(&mut output, value);
    output
}

fn append_plain_text(output: &mut String, value: &str) {
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
    let mut cursor = 0;
    while let Some(relative) = value[cursor..].find('<') {
        let index = cursor + relative;
        let candidate = &value[index..];
        if candidate.starts_with("<<http://") || candidate.starts_with("<<https://") {
            return Some((index, 2));
        }
        if candidate.starts_with("<http://") || candidate.starts_with("<https://") {
            return Some((index, 1));
        }
        cursor = index + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{escape_text, find_angle_url};

    #[test]
    fn angle_urls_keep_original_order_opening_width_and_invalid_text() {
        for (source, expected) in [
            ("ordinary 中 &amp; a_b", "ordinary 中 &amp;amp; a_b"),
            ("<http://a.test>", "<http://a.test>"),
            ("<<https://a.test>>", "<https://a.test>"),
            ("<<<http://a.test>>", "\\<<http://a.test>"),
            (
                "<https://a.test> <<http://b.test>>",
                "<https://a.test> <http://b.test>",
            ),
            ("<http://a test>", "\\<http\\://a test\\>"),
            ("<http://a\u{a0}test>", "\\<http\\://a\u{a0}test\\>"),
            ("<http://a<test>", "\\<http\\://a\\<test\\>"),
            ("<<https://unclosed", "\\<\\<https\\://unclosed"),
        ] {
            assert_eq!(escape_text(source), expected, "{source}");
        }
        assert_eq!(find_angle_url("中<<https://a> <http://b>"), Some((3, 2)));
        assert_eq!(find_angle_url("<no>é<http://a>"), Some((6, 1)));
        assert_eq!(find_angle_url("<no> <HTTP://a>"), None);
    }

    #[test]
    fn repeated_single_form_urls_keep_every_target_in_commonmark() {
        let source = "<http://example.test> ".repeat(4096);
        let markdown = escape_text(&source);
        assert_eq!(markdown, source);
        let links = pulldown_cmark::Parser::new(&markdown)
            .filter(|event| {
                matches!(event, pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
                    dest_url, ..
                }) if dest_url.as_ref() == "http://example.test")
            })
            .count();
        assert_eq!(links, 4096);
    }

    #[test]
    fn repeated_invalid_openings_keep_literal_text_and_closing_policy() {
        let spaced = format!("{}>", "<http:// ".repeat(4096));
        assert_eq!(
            escape_text(&spaced),
            format!("{}\\>", "\\<http\\:// ".repeat(4096))
        );
        let nested = format!("{}>", "<http://".repeat(4096));
        // The last candidate has no illegal character: the existing syntax
        // policy accepts it even without a host. Earlier nested ones remain
        // literal; optimizing scans must not introduce target validation.
        assert_eq!(
            escape_text(&nested),
            format!("{}<http://>", "\\<http\\://".repeat(4095))
        );
        // A double opening with no matching double close stops the old
        // recognizer; an inner single URL must not change that literal policy.
        assert_eq!(
            escape_text("<<http:// <https://example.test>"),
            "\\<\\<http\\:// \\<https\\://example.test\\>"
        );
        assert_eq!(
            escape_text("<<http://bad <http://a> >> <https://b>"),
            "\\<\\<http\\://bad <http://a> \\>\\> <https://b>"
        );
    }
}
