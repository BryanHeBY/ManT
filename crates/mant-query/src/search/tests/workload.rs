//! Count actual mapping work so dense-hit regressions do not rely on timings.
use super::{MAX_OCCURRENCES_PER_MATCH, query, request};
use crate::{ResolvedContent, search_query};
use mant_ir::{Block, Inline, LayoutHint};
use mant_protocol::{SearchCase, SearchScope, SearchSyntax};

use super::super::{LineIndex, mapping::work, occurrence_line_ranges};

fn literal_content(value: &str) -> ResolvedContent {
    let mut content = query();
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::Preformatted {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }];
    content
}

#[test]
fn dense_long_lines_only_map_the_retained_page_and_occurrence_cap() {
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        for syntax in [SearchSyntax::Literal, SearchSyntax::Regex] {
            for count in [255, 256, 257, 4096, 16_384] {
                let value = "a ".repeat(count);
                let content = literal_content(&value);
                let artifact_bytes = mant_codec::encode::render_addressable_markdown(&content)
                    .text()
                    .len();
                for offset in [0, 1, u32::MAX] {
                    let request = mant_protocol::SearchQuery {
                        scope,
                        syntax,
                        offset,
                        limit: 1,
                        context_lines: 0,
                        case: SearchCase::Sensitive,
                        ..request("a")
                    };
                    let (result, work) =
                        work::measure(|| search_query(&content, &request).unwrap());
                    if count >= 4096
                        && scope == SearchScope::Visible
                        && syntax == SearchSyntax::Literal
                    {
                        eprintln!("dense hits={count}, offset={offset}: {work:?}");
                    }
                    assert_eq!(result.total, 1, "{request:?}");
                    assert_eq!(result.returned, u32::from(offset == 0));
                    assert!(!result.truncated);
                    assert_eq!(result.next_offset, None);
                    assert!(work.validity_checks >= count);
                    assert_eq!(work.validity_line_visits, work.validity_checks);
                    assert_eq!(work.line_index_bytes, artifact_bytes);
                    // An anchor-free line is borrowed even when the matcher
                    // finds thousands of occurrences. A skipped group never
                    // constructs presentation or scalar-coordinate indexes.
                    assert_eq!(work.presentation_bytes_copied, 0);
                    if offset != 0 {
                        assert_eq!(result.matches, []);
                        assert_eq!(work.range_mappings, 0);
                        assert_eq!(work.presentation_builds, 0);
                        assert_eq!(work.scalar_index_bytes, 0);
                        continue;
                    }
                    let hit = &result.matches[0];
                    let retained = count.min(MAX_OCCURRENCES_PER_MATCH);
                    assert_eq!(hit.ordinal, 1);
                    assert_eq!(hit.occurrence_count, u32::try_from(count).unwrap());
                    assert_eq!(hit.occurrences.len(), retained);
                    assert_eq!(hit.occurrences_truncated, count > retained);
                    assert_eq!(hit.preview, value.trim_end());
                    assert_eq!(work.range_mappings, retained);
                    assert_eq!(work.presentation_builds, 1);
                    assert_eq!(work.scalar_index_bytes, value.len());
                    // All whole-line scanning is bounded by the indexed
                    // artifact plus one selected line, independent of hits.
                    assert!(work.scalar_index_bytes <= work.line_index_bytes);
                    for (index, occurrence) in hit.occurrences.iter().enumerate() {
                        assert_eq!(occurrence.matched_text, "a");
                        assert_eq!(
                            occurrence.markdown.start_column,
                            u32::try_from(index * 2 + 1).unwrap()
                        );
                        assert_eq!(
                            occurrence.markdown.end_column,
                            occurrence.markdown.start_column + 1
                        );
                        assert_eq!(occurrence.line_ranges.len(), 1);
                        assert_eq!(
                            occurrence.line_ranges[0].start_byte,
                            u32::try_from(index * 2).unwrap()
                        );
                        assert_eq!(
                            occurrence.line_ranges[0].end_byte,
                            u32::try_from(index * 2 + 1).unwrap()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dense_unicode_regexes_index_scalar_columns_once_per_selected_line() {
    let value = "中e\u{301} a ".repeat(4096);
    let content = literal_content(&value);
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        for offset in [0, 1, 99] {
            let request = mant_protocol::SearchQuery {
                scope,
                offset,
                syntax: SearchSyntax::Regex,
                case: SearchCase::Sensitive,
                context_lines: 0,
                limit: 1,
                ..request("中e\\x{301}|a")
            };
            let (result, work) = work::measure(|| search_query(&content, &request).unwrap());
            assert_eq!(result.total, 1);
            assert_eq!(result.returned, u32::from(offset == 0));
            if offset != 0 {
                assert_eq!(work.scalar_index_bytes, 0);
                assert_eq!(work.range_mappings, 0);
                assert_eq!(work.presentation_builds, 0);
                continue;
            }
            let hit = &result.matches[0];
            assert_eq!(hit.occurrence_count, 8192);
            assert!(hit.occurrences_truncated);
            assert_eq!(work.range_mappings, MAX_OCCURRENCES_PER_MATCH);
            assert_eq!(work.scalar_index_bytes, value.len());
            assert_eq!(work.presentation_builds, 1);
            assert_eq!(work.presentation_bytes_copied, 0);
            for (index, occurrence) in hit.occurrences.iter().enumerate() {
                let repetition = index / 2;
                let (matched, scalar, byte) = if index % 2 == 0 {
                    ("中e\u{301}", repetition * 6, repetition * 9)
                } else {
                    ("a", repetition * 6 + 4, repetition * 9 + 7)
                };
                assert_eq!(occurrence.matched_text, matched);
                assert_eq!(
                    occurrence.markdown.start_column,
                    u32::try_from(scalar + 1).unwrap()
                );
                assert_eq!(
                    occurrence.markdown.end_column,
                    u32::try_from(scalar + 1 + matched.chars().count()).unwrap()
                );
                let range = &occurrence.line_ranges[0];
                assert_eq!(range.start_byte, u32::try_from(byte).unwrap());
                assert_eq!(range.end_byte, u32::try_from(byte + matched.len()).unwrap());
                assert_eq!(&hit.preview[byte..byte + matched.len()], matched);
            }
        }
    }
}

#[test]
fn dense_matches_on_an_anchored_line_copy_its_presentation_once() {
    let mut content = query();
    let Block::DefinitionList { items, .. } =
        &mut content.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        unreachable!()
    };
    items[0].terms[0] = vec![
        Inline::anchor("option-acls"),
        Inline::Code {
            value: "--acls".into(),
        },
        Inline::Text {
            value: " needle".repeat(4096),
        },
    ]
    .into();
    for offset in [0, 1] {
        let request = mant_protocol::SearchQuery {
            offset,
            scope: SearchScope::Markdown,
            case: SearchCase::Sensitive,
            context_lines: 0,
            limit: 1,
            ..request("needle")
        };
        let (result, work) = work::measure(|| search_query(&content, &request).unwrap());
        assert_eq!(result.total, 1);
        if offset == 0 {
            let hit = &result.matches[0];
            assert_eq!(hit.occurrence_count, 4096);
            assert_eq!(work.range_mappings, MAX_OCCURRENCES_PER_MATCH);
            assert_eq!(work.presentation_builds, 1);
            assert_eq!(work.presentation_bytes_copied, hit.preview.len());
            assert!(!hit.preview.contains("<a id="));
            assert!(work.scalar_index_bytes <= work.line_index_bytes);
        } else {
            assert_eq!(result.returned, 0);
            assert_eq!(work.range_mappings, 0);
            assert_eq!(work.presentation_builds, 0);
            assert_eq!(work.presentation_bytes_copied, 0);
            assert_eq!(work.scalar_index_bytes, 0);
        }
    }
}

#[test]
fn cheap_validity_and_cached_columns_preserve_all_unicode_byte_boundaries() {
    let text = "前<a id=\"first\"></a>中 e\u{301} \t\u{a0}\n \t\u{a0}\n<a id=\"last\"></a>\n尾";
    let anchors = ["<a id=\"first\"></a>", "<a id=\"last\"></a>"]
        .into_iter()
        .map(|anchor| {
            let start = text.find(anchor).unwrap();
            start..start + anchor.len()
        })
        .collect();
    let lines = LineIndex::with_anchors(text, anchors);
    let boundaries: Vec<_> = text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .collect();
    for &start in &boundaries {
        for &end in boundaries.iter().filter(|end| **end > start) {
            let first_line = lines.line_index_at_byte(start);
            let last_line = lines.line_index_at_byte(end - 1);
            assert_eq!(
                lines.has_presented_bytes(start..end, first_line, last_line),
                !occurrence_line_ranges(start..end, &lines).is_empty(),
                "range {start}..{end}"
            );
        }
        let position = lines.position(start);
        let prefix = &text[..start];
        assert_eq!(
            position.line_index,
            prefix.bytes().filter(|byte| *byte == b'\n').count()
        );
        assert_eq!(
            position.column,
            prefix.rsplit('\n').next().unwrap().chars().count() + 1
        );
    }
}
