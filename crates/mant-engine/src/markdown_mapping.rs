//! Maps parsed inline Markdown events back to their canonical source spans.

use std::ops::Range;

/// Markdown event kind whose visible characters need source coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineMappingKind {
    /// Ordinary text, including renderer-inserted backslash escapes.
    Text,
    /// A `CommonMark` code span with a backtick delimiter and optional padding.
    Code,
}

/// One visible character and the smallest known canonical source span that
/// produces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MappedCharacter {
    pub(crate) value: char,
    pub(crate) source: Range<usize>,
    pub(crate) linear: bool,
}

/// Map a parsed inline event into visible characters with canonical spans.
pub(crate) fn map_inline_characters(
    markdown: &str,
    value: &str,
    source: Range<usize>,
    kind: InlineMappingKind,
) -> Vec<MappedCharacter> {
    let source =
        floor_char_boundary(markdown, source.start)..floor_char_boundary(markdown, source.end);
    if kind == InlineMappingKind::Code {
        return map_code_span(markdown, value, source.clone())
            .unwrap_or_else(|| map_opaque_characters(value, source));
    }
    try_map_aligned_text(markdown, value, source.clone())
        .unwrap_or_else(|| map_opaque_characters(value, source))
}

fn map_code_span(
    markdown: &str,
    value: &str,
    source: Range<usize>,
) -> Option<Vec<MappedCharacter>> {
    let rendered = markdown.get(source.clone())?;
    let delimiter_width = rendered.bytes().take_while(|byte| *byte == b'`').count();
    if delimiter_width == 0
        || rendered.len() < delimiter_width.saturating_mul(2)
        || !rendered.ends_with(&"`".repeat(delimiter_width))
    {
        return None;
    }

    let inner =
        source.start.saturating_add(delimiter_width)..source.end.saturating_sub(delimiter_width);
    let mut candidates = Vec::with_capacity(2);
    if markdown
        .get(inner.clone())
        .is_some_and(|content| content.starts_with(' ') && content.ends_with(' '))
        && inner.end.saturating_sub(inner.start) >= 2
    {
        candidates.push(inner.start + 1..inner.end - 1);
    }
    candidates.push(inner);

    candidates.into_iter().find_map(|content| {
        let mapped = map_code_content(markdown, content)?;
        mapped
            .iter()
            .map(|character| character.value)
            .eq(value.chars())
            .then_some(mapped)
    })
}

fn map_code_content(markdown: &str, source: Range<usize>) -> Option<Vec<MappedCharacter>> {
    let content = markdown.get(source.clone())?;
    let mut mapped = Vec::with_capacity(content.chars().count());
    let mut characters = content.char_indices().peekable();
    while let Some((relative, character)) = characters.next() {
        let start = source.start + relative;
        let (value, end) = if character == '\r' {
            if characters.peek().is_some_and(|(_, next)| *next == '\n') {
                let (next_relative, next) = characters.next()?;
                (' ', source.start + next_relative + next.len_utf8())
            } else {
                (' ', start + character.len_utf8())
            }
        } else if character == '\n' {
            (' ', start + character.len_utf8())
        } else {
            (character, start + character.len_utf8())
        };
        let character_source = start..end;
        mapped.push(MappedCharacter {
            value,
            linear: markdown.get(character_source.clone()) == Some(&value.to_string()),
            source: character_source,
        });
    }
    Some(mapped)
}

fn try_map_aligned_text(
    markdown: &str,
    value: &str,
    source: Range<usize>,
) -> Option<Vec<MappedCharacter>> {
    let mut mapped = Vec::with_capacity(value.chars().count());
    let mut cursor = source.start;
    let source_end = source.end;
    for character in value.chars() {
        let search_start = floor_char_boundary(markdown, cursor);
        let search_end = source_end.max(search_start);
        let found = markdown[search_start..search_end]
            .find(character)
            .map(|relative| search_start + relative)?;
        let character_end = floor_char_boundary(
            markdown,
            found.saturating_add(character.len_utf8()).min(search_end),
        );
        let character_source = search_start..character_end;
        mapped.push(MappedCharacter {
            value: character,
            linear: markdown.get(character_source.clone()) == Some(&character.to_string()),
            source: character_source,
        });
        cursor = character_end;
    }
    Some(mapped)
}

fn map_opaque_characters(value: &str, source: Range<usize>) -> Vec<MappedCharacter> {
    value
        .chars()
        .map(|value| MappedCharacter {
            value,
            source: source.clone(),
            linear: false,
        })
        .collect()
}

/// Largest char boundary not exceeding `offset`; stable stand-in for
/// `str::floor_char_boundary` on the workspace MSRV.
pub(crate) fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::{InlineMappingKind, map_inline_characters};

    #[test]
    fn code_span_mapping_skips_delimiters_and_commonmark_padding() {
        let markdown = "`` `x ``";
        let mapped =
            map_inline_characters(markdown, "`x", 0..markdown.len(), InlineMappingKind::Code);

        assert_eq!(mapped[0].source, 3..4);
        assert_eq!(mapped[1].source, 4..5);
    }

    #[test]
    fn code_span_mapping_normalizes_line_endings_without_losing_source() {
        let markdown = "`a\nb`";
        let mapped =
            map_inline_characters(markdown, "a b", 0..markdown.len(), InlineMappingKind::Code);

        assert_eq!(mapped[1].value, ' ');
        assert_eq!(mapped[1].source, 2..3);
        assert!(!mapped[1].linear);
    }

    #[test]
    fn all_space_code_spans_need_no_commonmark_padding() {
        let markdown = "` `";
        let mapped =
            map_inline_characters(markdown, " ", 0..markdown.len(), InlineMappingKind::Code);

        assert_eq!(mapped[0].source, 1..2);
    }

    #[test]
    fn text_mapping_attributes_renderer_escapes_to_the_visible_character() {
        let markdown = r"\*";
        let mapped =
            map_inline_characters(markdown, "*", 0..markdown.len(), InlineMappingKind::Text);

        assert_eq!(mapped[0].source, 0..2);
        assert!(!mapped[0].linear);
    }

    #[test]
    fn unaligned_text_uses_one_explicit_conservative_event_span() {
        let markdown = "&copy;";
        let mapped =
            map_inline_characters(markdown, "©", 0..markdown.len(), InlineMappingKind::Text);

        assert_eq!(mapped[0].source, 0..markdown.len());
        assert!(!mapped[0].linear);
    }
}
