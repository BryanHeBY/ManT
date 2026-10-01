//! Pinned macro execution and consumer contracts for links, enclosures,
//! macro post handlers and declared column layouts.
//!
//! `macro_consumer_matrix/cases/*.1` and sibling snapshots pin the CVS
//! `-Tutf8 -Owidth=78` output. Record expectations only through
//! `scripts/roff/fixtures/regen_macro_consumer_matrix.sh`. The projection applies
//! backspace replacement, maps NBSP to its occupied blank, removes page
//! furniture by position, and trims each row's outer whitespace. Internal
//! spaces and blank rows remain exact; these snapshots do not measure row
//! origins or trailing spaces. Dedicated consumer tests cover those facts.
//!
//! Native row cases require equality and JSON text round trips. Approved
//! responsive and reading layouts have explicit family contracts rather
//! than native device-row equality. Family checks also cover decoded
//! targets, styled text and source-neutral IR.

use std::fmt::Write as _;

mod column;
mod column_contracts;
mod link;
mod post;
mod quote;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/macro_consumer_matrix/cases"
);

pub(super) fn project(output: &str) -> Vec<String> {
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

pub(super) fn actual_rows(rendered: &str) -> Vec<String> {
    project(rendered)
}

pub(super) fn load_case(name: &str) -> (mant_ir::ResolvedContent, Vec<String>) {
    let source_path = format!("{CASES}/{name}.1");
    let source = std::fs::read_to_string(&source_path)
        .unwrap_or_else(|error| panic!("{name}: read case source: {error}"));
    let expected = std::fs::read_to_string(format!("{CASES}/{name}.expected"))
        .unwrap_or_else(|error| panic!("{name}: read snapshot: {error}"))
        .lines()
        .map(str::to_owned)
        .collect();
    let query = mant_loader::load_roff_bytes(source.as_bytes())
        .unwrap_or_else(|error| panic!("{name}: lower case: {error}"));
    (query, expected)
}

/// All checked-in case names with the given family prefix, pinned to the
/// expected count so silent case loss or duplication fails the run.
pub(super) fn case_names(prefix: &str, expected_count: usize) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(CASES)
        .unwrap_or_else(|error| panic!("macro consumer matrix case directory: {error}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "1")
                && path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.starts_with(prefix))
        })
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names.len(),
        expected_count,
        "{prefix} case set changed; regen via scripts/roff/fixtures/regen_macro_consumer_matrix.sh"
    );
    names
}

/// One family's evaluation state: layer 1 per case, then layer 2 for the
/// matching ones, with an optional family consumer check.
type ConsumerCheck<'a> = &'a dyn Fn(&mant_ir::ResolvedContent, &[String]) -> Option<String>;

pub(super) struct MatrixRun {
    failures: Vec<String>,
}

impl MatrixRun {
    pub(super) fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    /// Evaluate one case: layer 1 (row projection), then `extra`
    /// (family-specific projection such as ANSI parity), then layer 2
    /// (JSON text round-trip) for matching cases.
    pub(super) fn evaluate(&mut self, name: &str, extra: Option<ConsumerCheck<'_>>) {
        let (query, expected) = load_case(name);

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
            self.failures.push(report);
            return;
        }

        if let Some(extra) = extra
            && let Some(report) = extra(&query, &expected)
        {
            self.failures.push(format!("{name}: {report}\n"));
            return;
        }

        // Layer 2: IR semantics — the JSON contract round-trips: decoding
        // the serialized bundle back into the IR and re-rendering yields
        // the same contract and byte-identical rendering of the rows.
        let json = mant_render::render_query_json(&query, false)
            .unwrap_or_else(|error| panic!("{name}: render json: {error}"));
        let value: serde_json::Value = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("{name}: parse json: {error}"));
        let roundtripped = serde_json::from_str::<mant_protocol::QueryBundle>(&json)
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
            &serde_json::from_str::<mant_protocol::QueryBundle>(&json)
                .unwrap_or_else(|error| panic!("{name}: bundle decode: {error}"))
                .into(),
        );
        assert_eq!(
            actual_rows(&after),
            expected,
            "{name}: rendering moved after the JSON roundtrip"
        );
    }

    /// All cases are required passes; report their row differences together.
    pub(super) fn finish(self, family: &str) {
        assert!(
            self.failures.is_empty(),
            "{family} differs from the pinned oracle:\n{}",
            self.failures.join("")
        );
    }
}
