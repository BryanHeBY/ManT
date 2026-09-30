//! Shared execution-boundary matrix (wave 2c) against pinned CVS mandoc.
//!
//! Cases under `shared_execution_matrix/cases/*.1` pin the two unified
//! execution boundaries:
//!
//! * matrix B (`b*`): formatter-generated glyphs (enclosure brackets, `.Fl`
//!   prefix, `.In`/`.Bx` spellings, inset/diag gaps) against pending `\z`
//!   state — the generated writes must share the held-blank queue with
//!   authored glyphs, so the `encode1()` last-byte retreat
//!   (term.c:901-908) settles the pending glyph instead of replacing it.
//! * list gaps (`g_*`): generated run-in list spacing across
//!   inset/diag/tag/hang/ohang against pending `\z` HEAD shapes.
//!
//! The sibling `.expected` file holds the oracle projection
//! (`-Tutf8 -Owidth=78`), recorded by `scripts/regen_shared_execution_matrix.sh`:
//! backspace-pop projection, NBSP read as the blank it occupies, furniture
//! removed by position windows only (row 0 header, trailing footer block).
//! Interior blank rows are paragraph structure and stay pinned; intra-row
//! spacing survives because only the page margin is trimmed.

use std::fmt::Write as _;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/shared_execution_matrix/cases"
);

fn project(output: &str) -> Vec<String> {
    let mut projected = String::with_capacity(output.len());
    for character in output.chars() {
        if character == '\u{8}' {
            projected.pop();
        } else if character == '\u{a0}' {
            // The generated run-in cell is NBSP on this device; glyph
            // comparison reads it as the blank it occupies.
            projected.push(' ');
        } else {
            projected.push(character);
        }
    }
    let mut rows: Vec<String> = projected
        .lines()
        .map(|line| line.trim().to_owned())
        .collect();
    if rows.is_empty() {
        return rows;
    }
    rows.remove(0); // header/label row
    while rows.first().is_some_and(String::is_empty) {
        rows.remove(0);
    }
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

fn load_case(name: &str) -> (mant_ir::ResolvedContent, Vec<String>) {
    let source_path = format!("{CASES}/{name}.1");
    let source = std::fs::read_to_string(&source_path).unwrap_or_else(|error| {
        panic!("{name}: read case source: {error}");
    });
    let expected = std::fs::read_to_string(format!("{CASES}/{name}.expected"))
        .unwrap_or_else(|error| panic!("{name}: read snapshot: {error}"))
        .lines()
        .map(str::to_owned)
        .collect();
    let query = mant_loader::load_roff_bytes(source.as_bytes())
        .unwrap_or_else(|error| panic!("{name}: lower case: {error}"));
    (query, expected)
}

fn matrix_case_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(CASES)
        .expect("shared execution matrix case directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "1"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names.len(),
        39,
        "case set changed; regen via scripts/regen_shared_execution_matrix.sh"
    );
    names
}

/// Layer 1 (execution result) + layer 2 (IR semantics) + layer 3
/// (consumers) for every matrix case: the row machine's decision must match
/// the oracle, the JSON contract must round-trip without moving content,
/// and the markdown consumer must keep every surviving glyph exactly once
/// without resurrecting rejected content.
#[test]
fn shared_execution_matrix_matches_the_pinned_reference() {
    let mut failures = Vec::new();
    for name in matrix_case_names() {
        let (query, expected) = load_case(&name);

        // Layer 1: execution result (which cells survived, which rows ended).
        let rendered = mant_render::render_query_man(&query);
        let actual = project(&rendered);
        if actual != expected {
            let mut report = format!("{name}\n");
            for (index, (actual_row, expected_row)) in
                actual.iter().zip(expected.iter()).enumerate()
            {
                if actual_row != expected_row {
                    let _ = writeln!(
                        report,
                        "  row {index}: ManT {actual_row:?} vs reference {expected_row:?}"
                    );
                }
            }
            if actual.len() != expected.len() {
                let _ = writeln!(
                    report,
                    "  row count: ManT {} vs reference {}",
                    actual.len(),
                    expected.len()
                );
            }
            failures.push(report);
            continue;
        }

        // Layer 2: IR semantics — the JSON contract round-trips: decoding
        // the serialized bundle back into the IR and re-rendering yields
        // the same contract, and byte-identical rendering of the rows.
        let json = mant_render::render_query_json(&query, false)
            .unwrap_or_else(|error| panic!("{name}: render json: {error}"));
        let value: serde_json::Value = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("{name}: parse json: {error}"));
        let roundtripped = serde_json::from_value::<mant_protocol::QueryBundle>(value.clone())
            .unwrap_or_else(|error| panic!("{name}: bundle decode: {error}"));
        let json_again = mant_render::render_query_json(&(roundtripped.into()), false)
            .unwrap_or_else(|error| panic!("{name}: re-render json: {error}"));
        let value_again: serde_json::Value = serde_json::from_str(&json_again)
            .unwrap_or_else(|error| panic!("{name}: parse re-rendered json: {error}"));
        assert_eq!(
            value, value_again,
            "{name}: JSON roundtrip changed the contract"
        );
        let after = mant_render::render_query_man(
            &serde_json::from_value::<mant_protocol::QueryBundle>(value.clone())
                .unwrap_or_else(|error| panic!("{name}: bundle decode: {error}"))
                .into(),
        );
        assert_eq!(
            project(&after),
            expected,
            "{name}: rendering moved after the JSON roundtrip"
        );

        // Layer 3: consumers — the text renderer reproduces the oracle
        // rows, and the markdown consumer keeps every surviving word
        // exactly once (after dropping heading furniture and escape
        // syntax) without resurrecting rejected content.
        let text_rows = project(&mant_render::render_query_text(&query));
        assert_eq!(text_rows, expected, "{name}: text consumer rows diverged");
        let markdown = mant_codec::encode::render_markdown(&query);
        let body_markdown: Vec<&str> = markdown
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect();
        // The fixture template's section headings (DESCRIPTION/NEXT) are
        // furniture on both sides; body words must match exactly.
        let body_words: Vec<String> = body_markdown
            .iter()
            .flat_map(|line| {
                strip_markdown_links(line)
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .map(|word| word.replace(['\\', '*', '_', '`', '[', ']'], ""))
            .filter(|word| {
                !word.is_empty() && word != "DESCRIPTION" && word != "NEXT" && word != "-"
            })
            .collect();
        let expected_body: Vec<String> = expected
            .iter()
            .flat_map(|row| row.split(' '))
            .filter(|word| !word.is_empty() && *word != "DESCRIPTION" && *word != "NEXT")
            .map(|word| word.replace(['[', ']'], ""))
            .collect();
        for word in &expected_body {
            assert!(
                body_words.contains(word),
                "{name}: markdown consumer lost {word:?} in:\n{body_markdown:?}"
            );
        }
        assert_eq!(
            body_words.len(),
            expected_body.len(),
            "{name}: markdown consumer added, duplicated, or resurrected words in:\n{body_markdown:?}"
        );
    }
    assert!(failures.is_empty(), "row diffs:\n{}", failures.join(""));
}

/// Drop `](target)` link tails so link labels compare as plain words.
fn strip_markdown_links(line: &str) -> String {
    let mut stripped = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find("](") {
        stripped.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after.find(')') {
            Some(close) => rest = &after[close + 1..],
            None => rest = "",
        }
    }
    stripped.push_str(rest);
    stripped
}
