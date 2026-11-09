//! Source preparation and safe export retain independent consumer evidence.
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::ResolvedContent;
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

#[path = "../support/source_admission.rs"]
mod admission;
use super::link_evidence;
use crate::json_roundtrip;

fn assert_search(content: &ResolvedContent, case: &admission::Case) {
    for row in case.rows {
        let result = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: (*row).into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope: SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 20,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(result.total, 1, "{}: exact visible search", case.id);
        let matches: Vec<_> = result
            .matches
            .iter()
            .flat_map(|hit| &hit.occurrences)
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, *row);
    }
}

#[test]
fn source_admission_keeps_ir0_meaning_and_search_after_json_and_two_readers() {
    assert_eq!(admission::CASES.len(), 8);
    for case in admission::CASES {
        let original = case.load();
        assert_search(&original, case);
        let expected = link_evidence::Meaning::capture(&case.export_witness(&original));
        for preserve_semantics in [false, true] {
            let mut current = json_roundtrip::json_string_roundtrip(&original);
            for round in 1..=2 {
                let markdown = render_markdown_with_options(
                    &current,
                    MarkdownOptions {
                        math: mant_codec::encode::MarkdownMath::Latex,
                        preserve_semantics,
                        preserve_anchors: false,
                    },
                );
                current = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
                case.assert_content(&current, false);
                expected.assert_preserved(
                    &current,
                    &format!("{}/{preserve_semantics}/{round}", case.id),
                );
                assert_search(&current, case);
            }
        }
    }
}
