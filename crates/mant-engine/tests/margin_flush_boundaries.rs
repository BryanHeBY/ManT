//! Native margin flushes retain exact word seams through public consumers.

use mant_ir::ResolvedContent;
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/margin_flush_cases.rs"]
mod recorded;

fn rows(content: &ResolvedContent) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    let lines: Vec<_> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim_start_matches(' ') == "ORIGIN")
        .unwrap();
    let end = lines
        .iter()
        .position(|line| line.trim_start_matches(' ') == "TAIL")
        .unwrap();
    let prefix = " ".repeat(lines[start].find("ORIGIN").unwrap());
    lines[start..=end]
        .iter()
        .map(|line| line.strip_prefix(&prefix).unwrap_or(line).to_string())
        .collect()
}

fn search(content: &ResolvedContent, pattern: &str) -> u32 {
    let result = mant_query::search_query(
        content,
        &SearchQuery {
            pattern: pattern.into(),
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
    for hit in &result.matches {
        for occurrence in &hit.occurrences {
            assert_eq!(occurrence.matched_text, pattern);
        }
    }
    result.total
}

#[test]
fn margin_flush_word_seams_survive_json_search_and_two_markdown_readbacks() {
    for case in recorded::CASES {
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        for preserve_semantics in [false, true] {
            let mut current = json_roundtrip::json_string_roundtrip(&original);
            for round in 0..=2 {
                assert_eq!(rows(&current), ["ORIGIN", case.row, "TAIL"], "{}", case.id);
                assert_eq!(search(&current, case.row), 1, "{}/{round}", case.id);
                assert_eq!(search(&current, case.rejected_row), 0, "{}", case.id);
                if round < 2 {
                    let markdown = mant_codec::encode::render_markdown_with_options(
                        &current,
                        mant_codec::encode::MarkdownOptions {
                            preserve_semantics,
                            preserve_anchors: false,
                        },
                    );
                    current = json_roundtrip::json_string_roundtrip(
                        &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                    );
                }
            }
        }
    }
}
