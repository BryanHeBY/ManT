//! Canonical complete-source matrices for formatter acceptance and ownership.

use mant_ir::{LinkTarget, ReferenceScope, ReferenceTargetType, ResolvedContent};
use mant_protocol::{ReferenceProjection, ReferenceProjectionMode};

#[derive(serde::Deserialize)]
struct Case {
    id: String,
    source: String,
    scope: String,
    scope_reason: String,
    terminal_heading: String,
    utf8_rows: Option<Vec<String>>,
    reading_utf8_rows: Option<Vec<String>>,
    reading_rule: String,
    authored_targets: Vec<String>,
    portable_identity_scope: String,
}

fn fixtures(data: &str, expected_count: usize) -> Vec<Case> {
    let cases = data
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), expected_count);
    cases
}

fn round_trip(case: &Case) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(case.source.as_bytes())
        .unwrap_or_else(|error| panic!("{}: {error}\n{}", case.id, case.source));
    let json = mant_render::render_query_json(&query, false).unwrap();
    assert!(
        !json.contains("\\u0000mant:"),
        "{}: private execution owner escaped JSON\n{}",
        case.id,
        case.source
    );
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json)
        .unwrap_or_else(|error| panic!("{}: {error}\n{}", case.id, case.source));
    restored.into()
}

fn body_rows(text: &str, end_heading: &str) -> Vec<String> {
    let rows = text.lines().collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let end = rows.iter().position(|row| *row == end_heading).unwrap();
    assert_eq!(rows[end - 1], "", "section separator: {text:?}");
    rows[start..end - 1]
        .iter()
        .map(|row| (*row).to_owned())
        .collect()
}

fn glyphs(rows: &[String]) -> String {
    rows.iter()
        .flat_map(|row| row.chars())
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn targets(query: &ResolvedContent) -> Vec<String> {
    let inventory = mant_query::project_references(
        query.document.as_ref().unwrap(),
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::External],
            ..Default::default()
        },
    );
    inventory
        .records
        .into_iter()
        .map(|record| match record.target {
            LinkTarget::External { uri } => uri,
            target => panic!("external projection returned {target:?}"),
        })
        .collect()
}

fn verify(case: &Case) {
    // Each exact source ran registered pristine ASCII/UTF-8/HTML/tree/lint
    // before the fixture was written. term_word() records spaces, NBRZW,
    // BACKBEFORE and BREAK in order; term_fill() alone accepts/rejects fields.
    // Xo/Xc have NULL pre/post. term_vspace() emits a real completed row.
    // mdoc_lk_pre() executes description/colon/URI; mdoc_html.c retains href
    // identity even when terminal field acceptance removes its visible text.
    let query = round_trip(case);
    let plain = mant_render::render_query_man(&query);
    let styled =
        mant_render::render_query_text_with(&query, |_, text| format!("\x1b[1m{text}\x1b[0m"));
    assert_eq!(
        styled.replace("\x1b[1m", "").replace("\x1b[0m", ""),
        plain,
        "{}: legal ANSI decoration changed layout\n{}",
        case.id,
        case.source
    );
    let markdown = mant_codec::encode::render_markdown(&query);
    assert!(
        !markdown.contains('\0'),
        "{}: private marker escaped portable text\n{}",
        case.id,
        case.source
    );
    // Recovery inputs still cross the portable parser and real wire decoder.
    // Their recorded native diagnostics limit equivalence assertions, not
    // the consumer safety boundary shared by every complete source.
    let portable = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let portable_json = mant_render::render_query_json(&portable, false).unwrap();
    let _: mant_protocol::QueryBundle = serde_json::from_str(&portable_json).unwrap();
    assert!(!portable_json.contains("\\u0000mant:"), "{}", case.id);
    if case.scope == "recovery-only" {
        // Invalid Ta/RS/tbl recovery and the margin-character side artifact
        // retain source and diagnostics. They are execution-safety coverage,
        // not successful native-content or hard-row equivalence assertions.
        return;
    }
    let expected = case.utf8_rows.as_ref().unwrap();
    let actual = body_rows(&plain, &case.terminal_heading);
    assert_eq!(
        glyphs(&actual),
        glyphs(expected),
        "{}: native accepted content differs\n{}\noracle: {expected:?}\nactual: {actual:?}",
        case.id,
        case.source
    );
    if case.scope == "hard-rows" {
        // Both headers have five common device columns. The fixture also
        // records the separately frozen G-IND temporary device origin
        // exceptions, derived from source and oracle pairs, never product.
        // No rule removes empty physical rows or authored word separators.
        let expected = case.reading_utf8_rows.as_ref().unwrap();
        assert_eq!(
            &actual, expected,
            "{}: native hard rows differ ({}; {})\n{}",
            case.id, case.scope_reason, case.reading_rule, case.source
        );
    } else {
        assert_eq!(case.scope, "accepted-content");
    }
    assert_eq!(
        targets(&query),
        case.authored_targets,
        "{}: authored identity occurrences differ\n{}",
        case.id,
        case.source
    );
    if case.portable_identity_scope == "rich-inline" {
        assert_eq!(
            targets(&portable),
            case.authored_targets,
            "{}: rich portable identity occurrences differ\n{}\n{markdown}",
            case.id,
            case.source
        );
    } else {
        assert_eq!(case.portable_identity_scope, "display-and-safety");
        // Fences are literal export and UR's native angle suffix can form
        // another autolink. Still execute the real parser and wire boundary;
        // those older contracts do not grant typed-occurrence equivalence.
    }
}

fn verify_matrix(cases: Vec<Case>) {
    // Keep running independent sources after a discrepancy so a single
    // matrix run reports every failing combination with its exact oracle.
    // Each source owns a fresh parse, wire round trip and output projection.
    let mut failures = Vec::new();
    for case in cases {
        if std::panic::catch_unwind(|| verify(&case)).is_err() {
            failures.push(case.id);
        }
    }
    assert!(failures.is_empty(), "failing exact sources: {failures:?}");
}

#[test]
fn formatter_matrix_preserves_native_acceptance_hard_rows_and_identity() {
    verify_matrix(fixtures(
        include_str!("fixtures/native_formatter_matrix.jsonl"),
        2520,
    ));
}

#[test]
fn link_matrix_preserves_native_acceptance_hard_rows_and_each_occurrence() {
    verify_matrix(fixtures(
        include_str!("fixtures/native_links_matrix.jsonl"),
        868,
    ));
}
