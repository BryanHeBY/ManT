//! Embed generated `MathML` in `CommonMark` without introducing Markdown syntax.

use crate::math::{MAX_MATH_OUTPUT_BYTES, MathError, MathLimit};

pub(super) fn markdown_fragment(markup: &str) -> Result<String, MathError> {
    let mut output = String::with_capacity(markup.len());
    let mut tag = false;
    for character in markup.chars() {
        let escaped = match character {
            '<' => {
                tag = true;
                None
            }
            '>' => {
                tag = false;
                None
            }
            '*' if !tag => Some("&#42;"),
            '_' if !tag => Some("&#95;"),
            '`' if !tag => Some("&#96;"),
            '[' if !tag => Some("&#91;"),
            ']' if !tag => Some("&#93;"),
            '\\' if !tag => Some("&#92;"),
            '$' if !tag => Some("&#36;"),
            '!' if !tag => Some("&#33;"),
            '|' if !tag => Some("&#124;"),
            '\n' if !tag => Some("&#10;"),
            '\r' if !tag => Some("&#13;"),
            '\t' if !tag => Some("&#9;"),
            _ => None,
        };
        let bytes = escaped.map_or(character.len_utf8(), str::len);
        if bytes > MAX_MATH_OUTPUT_BYTES.saturating_sub(output.len()) {
            return Err(MathError::LimitExceeded {
                limit: MathLimit::OutputBytes,
            });
        }
        if let Some(escaped) = escaped {
            output.push_str(escaped);
        } else {
            output.push(character);
        }
    }
    Ok(output)
}

pub(in crate::encode) fn is_phrasing(source: &str) -> bool {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    source.starts_with("<math ")
        && source.ends_with("</math>")
        && Parser::new(source).all(|event| {
            matches!(
                event,
                Event::Start(Tag::Paragraph)
                    | Event::End(TagEnd::Paragraph)
                    | Event::InlineHtml(_)
                    | Event::Text(_)
            )
        })
}
