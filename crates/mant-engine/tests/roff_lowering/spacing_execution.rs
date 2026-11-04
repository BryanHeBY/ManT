//! Spacing mode, one-shot joins and the actual native buffer cursor are distinct.
use std::collections::BTreeMap;

use mant_ir::ResolvedContent;
use serde::Deserialize;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;
use super::oracle_receipt::{OracleReceipt, assert_receipts};

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    core_count: usize,
    control_count: usize,
    pairwise_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    cohort: String,
    source: String,
    source_sha256: String,
    rows: Vec<String>,
    reading_rows: Option<Vec<String>>,
    profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
struct Profile {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
    diagnostics: String,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("spacing_execution/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(
        (
            fixture.core_count,
            fixture.control_count,
            fixture.pairwise_count
        ),
        (144, 35, 12)
    );
    assert_eq!(fixture.cases.len(), 191);
    for case in &fixture.cases {
        assert_receipts(
            &case.id,
            &case.source_sha256,
            case.profiles.iter().map(|(profile, run)| {
                (
                    profile.as_str(),
                    OracleReceipt {
                        status: run.status,
                        stdout_sha256: &run.stdout_sha256,
                        stderr_sha256: Some(&run.stderr_sha256),
                        diagnostics: &run.diagnostics,
                    },
                )
            }),
        );
    }
    fixture.cases
}

fn roundtrip(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let decoded = json_string_roundtrip(&content);
    assert_eq!(content.document, decoded.document);
    decoded
}

fn rows(content: &ResolvedContent) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim_start_matches(' ') == "ORIGIN")
        .unwrap();
    let end = lines
        .iter()
        .position(|line| line.trim_start_matches(' ') == "TAIL")
        .unwrap();
    let origin = lines[start].find("ORIGIN").unwrap();
    let prefix = " ".repeat(origin);
    // Only the measured device origin is removed. An NBRZW cursor can cause
    // an additional real leading SP on the SECOND row; it must remain here.
    lines[start..=end]
        .iter()
        .map(|line| {
            line.strip_prefix(prefix.as_str())
                .unwrap_or(line)
                .to_string()
        })
        .collect()
}

fn verify(check: impl Fn(&Case) + std::panic::RefUnwindSafe) {
    verify_cases(cases(), |case| &case.id, check);
}

#[test]
fn sm_on_executes_for_same_mode_changed_mode_and_toggle_with_exact_cursor_rows() {
    // All 191 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint before
    // these expectations. mdoc_term.c::termp_sm_pre clears NOSPACE whenever
    // final NONOSPACE is off and p->col > 0, even on->on. term.c::term_word
    // buffers ASCII_NBRZW but an empty word does not advance p->col; \c's
    // NONEWLINE is independent of that NOSPACE side effect (573-589,662-667).
    verify(|case| {
        let expected = case.reading_rows.as_ref().unwrap_or(&case.rows);
        assert_eq!(rows(&roundtrip(&case.source)), *expected, "{}", case.id);
    });
}

#[test]
fn exact_report_controls_keep_leading_sp_hard_rows_and_no_on_controls_distinct() {
    for (id, expected) in [
        (
            "control-on-visible-ns",
            vec!["ORIGIN", "FIRST SECOND", "TAIL"],
        ),
        (
            "control-on-visible-c",
            vec!["ORIGIN", "FIRST SECOND", "TAIL"],
        ),
        ("control-on-empty-word", vec!["ORIGIN", "SECOND", "TAIL"]),
        ("control-on-nbrzw", vec!["ORIGIN", " SECOND", "TAIL"]),
        (
            "control-on-flushed",
            vec!["ORIGIN", "FIRST", "SECOND", "TAIL"],
        ),
        (
            "control-without-sm-on",
            vec!["ORIGIN", "FIRSTSECOND", "TAIL"],
        ),
        (
            "control-off-source-lines",
            vec!["ORIGIN", "FIRSTSECOND", "TAIL"],
        ),
        ("control-on-to-off", vec!["ORIGIN", "FIRSTSECOND", "TAIL"]),
        ("control-off-to-off", vec!["ORIGIN", "FIRSTSECOND", "TAIL"]),
        (
            "control-on-toggle-off",
            vec!["ORIGIN", "FIRSTSECOND", "TAIL"],
        ),
        (
            "control-on-off-off-visible-text",
            vec!["ORIGIN", "FIRST SECOND", "TAIL"],
        ),
    ] {
        let case = cases().into_iter().find(|case| case.id == id).unwrap();
        assert_eq!(case.rows, expected, "independent pristine record: {id}");
        assert_eq!(rows(&roundtrip(&case.source)), expected, "{id}");
    }
}

#[test]
fn both_actual_markdown_readbacks_keep_every_core_and_control_space_and_row() {
    verify(|case| {
        // These 179 direct cases carry exact row semantics through ordinary
        // paragraphs and complete nf code owners. The 12 container supplements
        // above assert native producer rows; Markdown uses its existing list
        // and display presentation policy rather than those device columns.
        if case.cohort == "pairwise" {
            return;
        }
        let mut content = roundtrip(&case.source);
        for round in 1..=2 {
            let markdown = mant_codec::encode::render_markdown(&content);
            content = mant_loader::load_markdown_text(&markdown, None).unwrap();
            assert_eq!(
                rows(&content),
                case.rows,
                "{} round {round}: {markdown}",
                case.id
            );
        }
    });
}

#[test]
fn repeated_off_preserves_ordinary_words_nospace_without_an_explicit_join() {
    // These exact controls first ran all five pinned profiles. Sm off changes
    // NONOSPACE, while term_word leaves NOSPACE set under that mode. Repeating
    // off performs no NOSPACE rewrite: mdoc_term.c::termp_sm_pre (1820-1832),
    // term.c::term_word (573-589). An explicit Ns would mask this regression.
    for (cursor, expected) in [
        ("visible", vec!["ORIGIN", "FIRSTSECOND", "TAIL"]),
        ("empty", vec!["ORIGIN", "SECOND", "TAIL"]),
        ("nbrzw", vec!["ORIGIN", "SECOND", "TAIL"]),
        ("blank", vec!["ORIGIN", " SECOND", "TAIL"]),
    ] {
        let id = format!("control-off-off-filled-{cursor}-text");
        let case = cases().into_iter().find(|case| case.id == id).unwrap();
        assert!(!case.source.contains(" Ns"));
        assert!(!case.source.contains(r"\c"));
        assert_eq!(case.rows, expected, "independent pristine record: {id}");
        assert_eq!(rows(&roundtrip(&case.source)), expected, "{id}");
    }
}
