//! Maps parsed inline Markdown events back to their canonical source spans.

use std::ops::Range;

/// Read `CommonMark` events with original artifact byte ranges and the codec's
/// narrow leading-hard-row contract.
///
/// Like `Parser::new`, this leaves optional Markdown extensions disabled.
/// Only a canonical `<br />` first line followed by supported phrasing and
/// attribute-free line-break HTML is interpreted as a paragraph. Other raw
/// HTML remains HTML events. Ordinary events stream directly; the current
/// HTML block is buffered while validating that complete contract. Returned
/// ranges always refer to `markdown`, including through list prefixes.
pub fn markdown_source_events(
    markdown: &str,
) -> impl Iterator<Item = (pulldown_cmark::Event<'_>, Range<usize>)> {
    crate::markdown::canonical_br::parse_events(markdown, pulldown_cmark::Options::empty())
}

/// Markdown event kind whose visible characters need source coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineMappingKind {
    /// Ordinary text, including renderer-inserted backslash escapes.
    Text,
    /// A `CommonMark` code span with a backtick delimiter and optional padding.
    Code,
}

/// One visible character and the smallest known canonical source span that
/// produces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedCharacter {
    /// Visible Unicode scalar emitted by the parsed inline event.
    pub value: char,
    /// Half-open UTF-8 byte span in the canonical Markdown source.
    pub source: Range<usize>,
    /// Whether the source span is exactly the visible character's UTF-8 bytes.
    pub linear: bool,
}

/// Map a parsed inline event into visible characters with canonical spans.
///
/// `source` must be the ordered byte range (`start <= end`) of the parsed event.
/// Source offsets are clamped to UTF-8 boundaries. If the event cannot be aligned
/// exactly (for example an entity), each character retains the conservative event
/// span and has `linear: false`.
#[must_use]
pub fn map_inline_characters(
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
            // char_indices supplied the original scalar's exact bytes. Only
            // CommonMark's line-ending normalization changes those bytes.
            linear: !matches!(character, '\r' | '\n'),
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
    if markdown.get(source.clone()) == Some(value) {
        return Some(
            value
                .char_indices()
                .map(|(offset, character)| MappedCharacter {
                    value: character,
                    source: source.start + offset..source.start + offset + character.len_utf8(),
                    linear: true,
                })
                .collect(),
        );
    }
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
            // find(char) proved the exact scalar bytes at found. Any preceding
            // source syntax belongs to this span and makes it non-linear.
            linear: found == search_start,
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
#[must_use]
pub fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::{
        InlineMappingKind, MappedCharacter, map_inline_characters, markdown_source_events,
    };

    #[test]
    fn scalar_mapping_preserves_exact_and_transformed_source_spans() {
        for (markdown, value, kind, expected) in [
            (
                "中🦀e\u{301}",
                "中🦀e\u{301}",
                InlineMappingKind::Text,
                vec![
                    ('中', 0..3, true),
                    ('🦀', 3..7, true),
                    ('e', 7..8, true),
                    ('\u{301}', 8..10, true),
                ],
            ),
            (
                "\\*中",
                "*中",
                InlineMappingKind::Text,
                vec![('*', 0..2, false), ('中', 2..5, true)],
            ),
            (
                "&copy;",
                "©",
                InlineMappingKind::Text,
                vec![('©', 0..6, false)],
            ),
            (
                "`中\r\n🦀`",
                "中 🦀",
                InlineMappingKind::Code,
                vec![('中', 1..4, true), (' ', 4..6, false), ('🦀', 6..10, true)],
            ),
            (
                "`中\r🦀`",
                "中 🦀",
                InlineMappingKind::Code,
                vec![('中', 1..4, true), (' ', 4..5, false), ('🦀', 5..9, true)],
            ),
            (
                "`中\n🦀`",
                "中 🦀",
                InlineMappingKind::Code,
                vec![('中', 1..4, true), (' ', 4..5, false), ('🦀', 5..9, true)],
            ),
            (
                "` 中🦀 `",
                "中🦀",
                InlineMappingKind::Code,
                vec![('中', 2..5, true), ('🦀', 5..9, true)],
            ),
            ("` `", " ", InlineMappingKind::Code, vec![(' ', 1..2, true)]),
        ] {
            let expected = expected
                .into_iter()
                .map(|(value, source, linear)| MappedCharacter {
                    value,
                    source,
                    linear,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                map_inline_characters(markdown, value, 0..markdown.len(), kind),
                expected,
                "{markdown:?} / {kind:?}"
            );
        }
    }

    #[test]
    fn exact_text_does_not_bypass_code_delimiters_or_source_clamping() {
        let markdown = "#中🦀";
        assert_eq!(
            map_inline_characters(markdown, "中", 2..7, InlineMappingKind::Text),
            [MappedCharacter {
                value: '中',
                source: 1..4,
                linear: true
            }]
        );
        for source in [0..markdown.len(), 0..usize::MAX] {
            let mapped = map_inline_characters(markdown, markdown, source, InlineMappingKind::Code);
            assert!(
                mapped
                    .iter()
                    .all(|character| !character.linear && character.source == (0..markdown.len()))
            );
            assert_eq!(
                mapped
                    .iter()
                    .map(|character| character.value)
                    .collect::<String>(),
                markdown
            );
        }
        assert_eq!(
            map_inline_characters("``x`", "x", 0..4, InlineMappingKind::Code),
            [MappedCharacter {
                value: 'x',
                source: 0..4,
                linear: false
            }]
        );
    }

    #[test]
    fn ordinary_events_keep_commonmark_options_and_exact_original_offsets() {
        for source in [
            "# TEXT\n\n**word** [link](https://example.org) `code`\n",
            "| a | b |\n| - | - |\n| c | d |\n",
            "$literal$ ~~plain~~\n",
            "<br>\nAFTER\n",
            "<br class=x>\nAFTER\n",
            "<div>literal</div>\n",
            "<br />\n<script>alert(1)</script>\n",
            "- item\n\n  ```\n  <br />\n  ```\n",
        ] {
            let raw = pulldown_cmark::Parser::new(source)
                .into_offset_iter()
                .collect::<Vec<_>>();
            assert_eq!(
                markdown_source_events(source).collect::<Vec<_>>(),
                raw,
                "{source}"
            );
        }
    }

    #[test]
    fn canonical_phrasing_ranges_address_unicode_in_the_original_nested_source() {
        let source = "- <br />\n  **中文e\u{301}🦀**<br>\n  [END](https://example.org)\n";
        let mut observed = vec![];
        for (event, range) in markdown_source_events(source) {
            assert!(source.is_char_boundary(range.start) && source.is_char_boundary(range.end));
            if let pulldown_cmark::Event::Text(value) = event {
                let mapped = map_inline_characters(source, &value, range, InlineMappingKind::Text);
                for character in mapped {
                    assert_eq!(
                        &source[character.source.clone()],
                        character.value.to_string()
                    );
                    observed.push(character.value);
                }
            }
        }
        assert_eq!(
            observed.into_iter().collect::<String>(),
            "中文e\u{301}🦀END"
        );
    }

    #[test]
    fn canonical_break_tags_are_shared_hard_events_without_source_newline_doubling() {
        let source = "<br />\nAlpha<br>Beta<br>\nGamma\n";
        let events = markdown_source_events(source).collect::<Vec<_>>();
        let breaks = events
            .iter()
            .filter_map(|(event, range)| {
                matches!(event, pulldown_cmark::Event::HardBreak).then_some(&source[range.clone()])
            })
            .collect::<Vec<_>>();
        assert_eq!(breaks, ["<br />", "<br>", "<br>"]);
        assert!(events.iter().all(|(event, _)| !matches!(
            event,
            pulldown_cmark::Event::InlineHtml(_)
                | pulldown_cmark::Event::Html(_)
                | pulldown_cmark::Event::SoftBreak
        )));
    }

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
