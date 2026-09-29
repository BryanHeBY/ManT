//! Definition row-machine matrix against pinned CVS mandoc output.
//!
//! Each case under `definition_matrix/cases/*.1` is an mdoc document
//! exercising the NOBREAK head field row machine (hang/tag heads, `.sp`,
//! `.br`, `.nf`/`.fi`, width sweeps, `\:` and `\p` markers). The sibling
//! `.expected` file holds the **row-grouped** output of the fixed CVS
//! reference binary (`target/mandoc-migration/reference/mandoc -Tascii`),
//! recorded once by `scripts/regen_definition_matrix.sh`: metadata lines
//! and indentation widths are presentation, but which words share a
//! physical row is the row machine's observable decision. Regenerate the
//! expectations only with that script, never by hand.

use std::fmt::Write as _;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/definition_matrix/cases"
);

/// The probe's row-grouping normalization: overstrike projection, blank
/// collapse, then drop page furniture a fixed-width device adds.
fn normalize_mant(output: &str) -> Vec<String> {
    let mut projected = String::with_capacity(output.len());
    for character in output.chars() {
        if character == '\u{8}' {
            projected.pop();
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
                && !line
                    .chars()
                    .all(|character| character.is_ascii_uppercase() || character == ' ')
        })
        .collect()
}

#[test]
fn definition_matrix_rows_match_the_pinned_reference() {
    let mut failures = Vec::new();
    let mut total = 0;
    let mut entries: Vec<_> = std::fs::read_dir(CASES)
        .expect("definition matrix case directory")
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
        total, 54,
        "case set changed; regen via scripts/regen_definition_matrix.sh"
    );
    assert!(
        failures.is_empty(),
        "row-grouping diffs:\n{}",
        failures.join("")
    );
}
