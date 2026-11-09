use flate2::read::GzDecoder;
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::ResolvedContent;
use serde::Deserialize;

use super::super::case_failures::verify_cases;
use super::roundtrip;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    profiles: BTreeMap<String, (i32, String, String)>,
    rows: Vec<String>,
}

fn description_rows(text: &str) -> Vec<&str> {
    text.split_once("\nDESCRIPTION\n")
        .unwrap()
        .1
        .lines()
        .collect()
}

fn assert_rows(content: &ResolvedContent, case: &Case) {
    let text = mant_render::render_query_man(content);
    assert_eq!(description_rows(&text), case.rows, "{}", case.id);
}

#[test]
fn source_blanks_across_marker_boundaries_follow_accepted_native_cells() {
    // Every exact source ran pristine ASCII/UTF-8/HTML/tree/lint.
    // term_flushln() sweeps only SP at the resume cursor (term.c:205-207);
    // encode1() can retract a later blank without consuming an earlier
    // one (901-908). term_field() prints the remaining advance (389-427).
    let fixture: Fixture =
        serde_json::from_reader(GzDecoder::new(&include_bytes!("marker-blanks.json.gz")[..]))
            .unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), 256);
    assert_eq!(
        fixture
            .cases
            .iter()
            .filter(|case| case.profiles["lint"].0 == 1)
            .count(),
        8
    );
    verify_cases(
        fixture.cases,
        |case| &case.id,
        |case| {
            assert_eq!(
                case.profiles.keys().map(String::as_str).collect::<Vec<_>>(),
                ["ascii", "html", "lint", "tree", "utf8"]
            );
            for (profile, (status, stdout, stderr)) in &case.profiles {
                assert_eq!(stdout.len(), 64);
                assert_eq!(stderr.len(), 64);
                assert!(*status == 0 || (profile == "lint" && *status == 1));
            }
            let native = roundtrip(&case.source);
            assert_rows(&native, case);
            for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
                let mut markdown = render_markdown_with_options(&native, options);
                for _ in 0..2 {
                    let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
                    assert_rows(&decoded, case);
                    markdown = render_markdown_with_options(&decoded, options);
                }
            }
        },
    );
}
use std::collections::BTreeMap;
