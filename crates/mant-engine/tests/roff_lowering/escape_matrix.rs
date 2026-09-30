//! Escape/device-semantics matrix against pinned CVS mandoc `-Tutf8` output.
//!
//! Wave-1 regression corpus: native write receipts (`\z` BACKBEFORE retreat
//! over a word separator, term.c:901-908), single-device UTF-8 escape
//! semantics (`\:` buffers ASCII_NBRZW, chars.c:53 with term.c:631-632;
//! `\!`/`\?`/`\r` leave no footprint, roff_escape.c:156-160), the `\p`
//! pass rejections (term.c:143-146 with 233-237), and the generated run-in
//! cell overstrike order (term.c:901-908 through encode1(U+00A0)).
//!
//! Each case under `escape_matrix/cases/*.1` records the **row-grouped**
//! output of the pinned reference (`-Tutf8`), produced only by
//! `scripts/regen_escape_matrix.sh` — never by hand. Unlike
//! `definition_matrix` (recorded `-Tascii`), this matrix MUST stay UTF-8:
//! its cases sit exactly on the device fork. Row grouping treats a row as
//! furniture only when it is a single all-caps word of three or more
//! letters (section heads); short all-caps rows are content here.

use std::fmt::Write as _;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/escape_matrix/cases"
);

/// The probe's row-grouping normalization: overstrike projection, blank
/// collapse, then drop page furniture a fixed-width device adds.
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
            // The en/em dash glyph is a tracked presentation deviation
            // (G7); row grouping normalizes it on both sides.
            projected.push('-');
        } else {
            projected.push(character);
        }
    }
    projected
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| {
            !line.is_empty()
                && !line.ends_with("(1)")
                && !(line.len() >= 3
                    && line.chars().all(|character| character.is_ascii_uppercase()))
        })
        .collect()
}

#[test]
fn escape_matrix_rows_match_the_pinned_utf8_reference() {
    let mut failures = Vec::new();
    let mut total = 0;
    let mut entries: Vec<_> = std::fs::read_dir(CASES)
        .expect("escape matrix case directory")
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
        total, 50,
        "case set changed; regen via scripts/regen_escape_matrix.sh"
    );
    assert!(
        failures.is_empty(),
        "row-grouping diffs:\n{}",
        failures.join("")
    );
}
