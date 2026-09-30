//! Shared execution-boundary matrix (wave 2c, full recording) against the
//! pinned CVS mandoc oracle.
//!
//! Cases under `shared_execution_matrix/cases/*.1` (209) pin the unified
//! execution boundaries at `-Tutf8 -Owidth=78`:
//!
//! * matrix A (`a*`, 110): 16 accept/reject/cross-word `\p`/`\z` sequences
//!   across 7 contexts (independent `.No`, raw TEXT, same-line, no-fill,
//!   tag/hang/inset lists); `man1`/`man2` cover the man raw-TEXT forms.
//! * matrix B (`b*`, 19): formatter-generated glyphs (enclosure brackets,
//!   `.Fl` prefix, `.In`/`.Bx` spellings) against pending `\z` state — the
//!   generated writes must share the held-blank queue with authored glyphs,
//!   so the `encode1()` last-byte retreat (term.c:901-908) settles the
//!   pending glyph instead of replacing it.
//! * list gaps (`g_*`, 20): generated run-in list spacing across
//!   inset/diag/tag/hang/ohang against pending `\z` HEAD shapes.
//! * matrix C (`c_s*`, 36): control boundaries (`.ft`/`.ta`/`.Tg`/`.br`/
//!   `.sp`/`.mc`/`.Pp`/`.nf`/`.ti`) crossed with three initial buffer
//!   states (empty, armed `\z`, cells written).
//! * RF minimal repros (`rf*`, 22): the RF01-07 minimal failure examples
//!   from external review §3-9 — landed as known-red oracle pins so the
//!   fixes have a green target; expectations are oracle recordings and
//!   must never be edited to match ManT.
//!
//! The sibling `.expected` file holds the oracle projection, recorded by
//! `scripts/regen_shared_execution_matrix.sh`: backspace-pop projection,
//! NBSP read as the blank it occupies, furniture removed by position
//! windows only (row 0 header, trailing footer block). Interior blank rows
//! are paragraph structure and stay pinned; intra-row spacing survives
//! because only the page margin is trimmed. Layer 1 is exact row equality —
//! no whitespace collapsing and no blank-row squeezing.
//!
//! Assertion layers:
//! 1. row projection (execution result) — all cases;
//! 2. JSON contract round-trip — all green cases;
//! 3. consumers — first-cut `b*`/`g_*` cases only; extending layer 3 to
//!    the A/man/C/RF set is TODO(FIX3).

use std::fmt::Write as _;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/shared_execution_matrix/cases"
);

/// Cases whose layer-1 projection is still red against the oracle pin.
/// Entries are removed as the tracked fixes land; a known-red case that
/// turns green fails this test so the pin is promoted (layers 2-3 then
/// run automatically on the next execution).
const KNOWN_RED: &[&str] = &[
    // RF01: pending-\p breaks are dropped across control boundaries
    // (external review section 3). TODO(FIX1).
    "rf01b",
    "rf01c_nf",
    "rf01c_pp",
    "rf01c_sp0",
    "rf01c_sp1",
    // RF02: external review section 4. TODO(FIX1).
    "rf02",
    // RF03: external review section 5. TODO(FIX1).
    "rf03_1",
    // RF04: external review section 6 — generated glyph writes replace
    // the held glyph instead of settling it. TODO(FIX2).
    "rf04_bq1",
    "rf04_bq2",
    "rf04_bx",
    "rf04_fl",
    // RF05: external review section 7. TODO(FIX2).
    "rf05",
    // RF07: external review section 9. TODO(FIX2).
    "rf07",
];

/// First-cut case set with the consumer (layer 3) assertions already
/// pinned; the full recording lands layers 1-2 first (see header TODO).
fn has_consumer_layer(name: &str) -> bool {
    name.starts_with("b") || name.starts_with("g_")
}

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

/// Neutralize the known pre-existing `.Nd` dash gap on the ManT side only
/// (review appendix A: ManT em dash vs oracle en dash; tracked separately
/// from this matrix). Oracle expectations are never edited.
fn neutralize_known_dash_gap(rows: Vec<String>) -> Vec<String> {
    rows.into_iter()
        .map(|row| row.replace('\u{2014}', "\u{2013}"))
        .collect()
}

fn actual_rows(rendered: &str) -> Vec<String> {
    neutralize_known_dash_gap(project(rendered))
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
        209,
        "case set changed; regen via scripts/regen_shared_execution_matrix.sh"
    );
    names
}

/// Layer 1 (row projection) + layer 2 (JSON round-trip) + layer 3
/// (consumers, first-cut set) for every matrix case. Layer-1 reds must be
/// exactly the `KNOWN_RED` tracking list; green cases must keep the JSON
/// contract stable, and the first-cut consumer assertions must hold.
#[test]
fn shared_execution_matrix_matches_the_pinned_reference() {
    let mut failures = Vec::new();
    let mut red: Vec<String> = Vec::new();
    for name in matrix_case_names() {
        let (query, expected) = load_case(&name);

        // Layer 1: execution result (which cells survived, which rows ended).
        let rendered = mant_render::render_query_man(&query);
        let actual = actual_rows(&rendered);
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
            red.push(name.clone());
            continue;
        }
        if KNOWN_RED.contains(&name.as_str()) {
            failures.push(format!(
                "{name}: pinned known-red is green now; remove it from KNOWN_RED\n"
            ));
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
            actual_rows(&after),
            expected,
            "{name}: rendering moved after the JSON roundtrip"
        );

        // Layer 3: consumers — first-cut `b*`/`g_*` set only; extending to
        // the full recording is TODO(FIX3).
        if !has_consumer_layer(&name) {
            continue;
        }
        let text_rows = actual_rows(&mant_render::render_query_text(&query));
        assert_eq!(text_rows, expected, "{name}: text consumer rows diverged");
        let markdown = mant_codec::encode::render_markdown(&query);
        let body_markdown: Vec<&str> = markdown
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect();
        // The fixture template's section headings (NAME/DESCRIPTION/NEXT)
        // are furniture on both sides; body words must match exactly.
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
                !word.is_empty()
                    && !matches!(word.as_str(), "NAME" | "DESCRIPTION" | "NEXT")
                    && word != "-"
            })
            .collect();
        let expected_body: Vec<String> = expected
            .iter()
            .flat_map(|row| row.split(' '))
            .filter(|word| {
                !word.is_empty()
                    && !matches!(*word, "NAME" | "DESCRIPTION" | "NEXT")
            })
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
    let unexpected_reds: Vec<&str> = red
        .iter()
        .filter(|name| !KNOWN_RED.contains(&name.as_str()))
        .map(String::as_str)
        .collect();
    let still_listed: Vec<&str> = KNOWN_RED
        .iter()
        .copied()
        .filter(|listed| !red.iter().any(|name| name == listed))
        .collect();
    let mut report = String::new();
    if !unexpected_reds.is_empty() {
        let _ = writeln!(
            report,
            "new reds against the oracle pin: {:?}",
            unexpected_reds
        );
    }
    if !still_listed.is_empty() {
        let _ = writeln!(report, "KNOWN_RED entries no longer red: {:?}", still_listed);
    }
    assert!(
        unexpected_reds.is_empty() && still_listed.is_empty(),
        "KNOWN_RED tracking drifted\n{report}row diffs:\n{}",
        failures.join("")
    );
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
