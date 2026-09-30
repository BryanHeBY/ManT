//! Review §25 oracle-pinned matrix families: LK (links, targets and
//! positions, §25.3), AQ (quote enclosure topology, §25.4), MP (macro
//! post, §25.4) and CW (declared column geometry and boundaries, §25.2).
//!
//! Cases under `review25_matrix/cases/*.1` pin execution boundaries at
//! `-Tutf8 -Owidth=78` against the pinned CVS mandoc oracle, using the
//! same recording and normalization rule as the shared-execution matrix
//! (`scripts/regen_review25_matrix.sh` is the only writer of `.expected`
//! files): backspace-pop projection, NBSP read as the blank it occupies,
//! furniture removed by position windows only (row 0 header, trailing
//! footer block). Interior blank rows are paragraph structure and stay
//! pinned; intra-row spacing survives because only the page margin is
//! trimmed. Layer 1 is exact row equality.
//!
//! Assertion layers (review §25.5 chain, cut after layer 2 for this
//! matrix — consumers beyond text/JSON ride with the owning fix units):
//! 1. row projection (execution result) — all cases;
//! 2. JSON contract round-trip — all green cases;
//! 3. per-family extras — the LK target-decode contract, the CW
//!    ANSI-parity projection, and the non-oracle boundary tests
//!    (lk01 Markdown contract, cw14 hand-crafted IR, cw18 scale).
//!
//! `KNOWN_RED` tracks exactly which cases are still red against the
//! oracle pin; entries are removed as the dispatched fix units land
//! (LK → NF-LINK, MP → NF-POST, CW03+ → NF-COLUMN). A known-red case
//! that turns green fails the family test so the pin is promoted; an
//! unlisted red fails it so new regressions cannot hide.

use std::fmt::Write as _;

mod column;
mod column_contracts;
mod link;
mod post;
mod quote;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/review25_matrix/cases"
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

/// Neutralize the known pre-existing `.Nd` dash gap on the `ManT` side only
/// (review appendix A: `ManT` em dash vs oracle en dash; tracked separately
/// from this matrix). Oracle expectations are never edited.
pub(super) fn neutralize_known_dash_gap(rows: Vec<String>) -> Vec<String> {
    rows.into_iter()
        .map(|row| row.replace('\u{2014}', "\u{2013}"))
        .collect()
}

pub(super) fn actual_rows(rendered: &str) -> Vec<String> {
    neutralize_known_dash_gap(project(rendered))
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
        .unwrap_or_else(|error| panic!("review25 matrix case directory: {error}"))
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
        "{prefix} case set changed; regen via scripts/regen_review25_matrix.sh"
    );
    names
}

/// One family's evaluation state: layer 1 per case, then layer 2 for the
/// green ones, with an optional family extra that can turn a case red.
type ConsumerCheck<'a> = &'a dyn Fn(&mant_ir::ResolvedContent, &[String]) -> Option<String>;

pub(super) struct MatrixRun {
    failures: Vec<String>,
    red: Vec<String>,
}

impl MatrixRun {
    pub(super) fn new() -> Self {
        Self {
            failures: Vec::new(),
            red: Vec::new(),
        }
    }

    /// Evaluate one case: layer 1 (row projection), then `extra`
    /// (family-specific projection such as ANSI parity), then layer 2
    /// (JSON round-trip) for cases that are green and not pinned red.
    pub(super) fn evaluate(
        &mut self,
        name: &str,
        known_red: &[&str],
        extra: Option<ConsumerCheck<'_>>,
    ) {
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
            self.red.push(name.to_owned());
            return;
        }

        if let Some(extra) = extra
            && let Some(report) = extra(&query, &expected)
        {
            self.failures.push(format!("{name}: {report}\n"));
            self.red.push(name.to_owned());
            return;
        }

        if known_red.contains(&name) {
            self.failures.push(format!(
                "{name}: pinned known-red is green now; remove it from KNOWN_RED\n"
            ));
            return;
        }

        // Layer 2: IR semantics — the JSON contract round-trips: decoding
        // the serialized bundle back into the IR and re-rendering yields
        // the same contract and byte-identical rendering of the rows.
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
    }

    /// Close the family: reds must be exactly the `known_red` tracking
    /// list, in both directions.
    pub(super) fn finish(self, known_red: &[&str], family: &str) {
        let unexpected_reds: Vec<&str> = self
            .red
            .iter()
            .filter(|name| !known_red.contains(&name.as_str()))
            .map(String::as_str)
            .collect();
        let still_listed: Vec<&str> = known_red
            .iter()
            .copied()
            .filter(|listed| !self.red.iter().any(|name| name == listed))
            .collect();
        let mut report = String::new();
        if !unexpected_reds.is_empty() {
            let _ = writeln!(
                report,
                "new reds against the oracle pin: {unexpected_reds:?}"
            );
        }
        if !still_listed.is_empty() {
            let _ = writeln!(report, "KNOWN_RED entries no longer red: {still_listed:?}");
        }
        assert!(
            unexpected_reds.is_empty() && still_listed.is_empty(),
            "{family} KNOWN_RED tracking drifted\n{report}row diffs:\n{}",
            self.failures.join("")
        );
    }
}
