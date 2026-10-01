//! Field flush and buffer retirement matrix against pinned CVS mandoc output.
//!
//! Each case under `field_retirement_matrix/cases/*.1` is an mdoc document exercising
//! the real-`term_flushln()` retirement points (`.mc` across the list
//! kinds and the `.Xc` flags-timing probes; TAG tail-width thresholds
//! for `emptyno`/`embedded`/`nbrsp`/`nbrsp_tilde`/`mixed`/`tab`/`tabq`
//! boundary pairs).
//! The sibling `.expected` file holds the **row-grouped** output of the
//! fixed CVS reference binary
//! (`target/mandoc-migration/reference/mandoc -Tascii`), recorded once by
//! `scripts/roff/fixtures/regen_field_retirement_matrix.sh`: which words
//! share a physical row is the row machine's observable decision. The
//! `.mc <arg>` margin-note cases are deliberately absent: the per-line
//! trailing margin character (term.c:450-465) is a registered deviation,
//! not row-machine behavior. Regenerate the expectations only with that
//! script, never by hand.

use std::fmt::Write as _;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/field_retirement_matrix/cases"
);

/// The probe's row-grouping normalization: overstrike projection, glyph
/// normalization (NBSP and ASCII-compatible dash glyphs),
/// then drop only the page furniture by **position windows** — row 0 is
/// always the header/label row, trailing blank rows are page-edge framing.
/// This renderer has no footer block, so no trailing non-empty block is
/// ever removed: body rows like `Linux command` or `printf(3)` always
/// survive (the reference side applies the footer window in the regen
/// script, where the trailing non-empty block is the footer mandoc always
/// prints). Interior blank rows are paragraph structure and stay pinned.
fn normalize_mant(output: &str) -> Vec<String> {
    let mut projected = String::with_capacity(output.len());
    for character in output.chars() {
        if character == '\u{8}' {
            projected.pop();
        } else if character == '\u{a0}' {
            // The generated run-in cell is NBSP on this device; row
            // grouping treats it as the blank it occupies.
            projected.push(' ');
        } else if character == '\u{2013}' || character == '\u{2014}' {
            // This ASCII row-group snapshot normalizes Unicode dashes on
            // both sides; precise glyphs have dedicated consumer tests.
            projected.push('-');
        } else {
            projected.push(character);
        }
    }
    let mut rows: Vec<String> = projected
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>();
    if rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    if !rows.is_empty() {
        rows.remove(0);
    }
    while rows.first().is_some_and(String::is_empty) {
        rows.remove(0);
    }
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

#[test]
fn field_retirement_matrix_rows_match_the_pinned_reference() {
    let mut failures = Vec::new();
    let mut total = 0;
    let mut entries: Vec<_> = std::fs::read_dir(CASES)
        .expect("field retirement matrix case directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "1"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no matrix cases under {CASES}");
    for source_path in entries {
        let name = source_path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let expected_path = source_path.with_extension("expected");
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_else(|error| {
            panic!("missing snapshot {}: {error}", expected_path.display())
        });
        let expected_rows: Vec<String> = expected.lines().map(str::to_owned).collect();
        let source = std::fs::read_to_string(&source_path).expect("read case source");
        let query = mant_loader::load_roff_bytes(source.as_bytes())
            .unwrap_or_else(|error| panic!("{name}: lower case: {error}"));
        let rendered = mant_render::render_query_man(&query);
        let actual = normalize_mant(&rendered);
        total += 1;
        if actual != expected_rows {
            let mut report = format!("{name}\n");
            for (index, (actual_row, expected_row)) in
                actual.iter().zip(expected_rows.iter()).enumerate()
            {
                if actual_row != expected_row {
                    let _ = writeln!(
                        report,
                        "  row {index}: ManT {actual_row:?} vs reference {expected_row:?}"
                    );
                }
            }
            if actual.len() != expected_rows.len() {
                let _ = writeln!(
                    report,
                    "  row count: ManT {} vs reference {}",
                    actual.len(),
                    expected_rows.len()
                );
            }
            failures.push(report);
        }
    }
    assert_eq!(
        total, 161,
        "case set changed; regen via scripts/roff/fixtures/regen_field_retirement_matrix.sh"
    );
    assert!(
        failures.is_empty(),
        "row-grouping diffs:\n{}",
        failures.join("")
    );
}
