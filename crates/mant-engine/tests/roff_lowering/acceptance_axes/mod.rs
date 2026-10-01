//! Axis-based acceptance assertions for the roff shared execution boundary.
//!
//! Cases under `acceptance_axes/cases` (11) carry complete roff sources and
//! one committed `.expected` snapshot each: the structural
//! `DESCRIPTION`..`NEXT` window of the pinned CVS oracle at
//! `-Tutf8 -Owidth=78`, recorded by
//! `scripts/roff/fixtures/regen_acceptance_axes_gold.sh` (the sole writer,
//! with pristine-oracle preflight and a `--check` mode). ASCII, tree and
//! lint profiles of the same runs are recorded below `target` as evidence;
//! no expectation is ever taken from a product rendering.
//!
//! The six `*_prefix`/`*_revival`/`*_row`/`*_vspace`/`*_gap`/`*_payload`
//! cases anchor the main review examples; the remaining five back the
//! comparator mutation samples. Row facts of every card are validated
//! against the recorded oracle before any product comparison runs.

mod acceptance_cases;
mod axis_model;
mod main_examples;
mod mutations;

use axis_model::{
    AcceptanceCase, AxisKind, HardRowRelation, Owner, Status, is_blank_row, region_words,
    row_containing, separator_holds,
};

pub(crate) const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/acceptance_axes/cases"
);
pub(crate) const CASE_COUNT: usize = 11;

/// One committed case: its complete source and recorded oracle rows.
pub(crate) struct CaseFiles {
    pub(crate) source: String,
    pub(crate) oracle_rows: Vec<String>,
}

pub(crate) fn load_case(name: &str) -> CaseFiles {
    let source = std::fs::read_to_string(format!("{CASES}/{name}.1"))
        .unwrap_or_else(|error| panic!("{name}: read case source: {error}"));
    let oracle_rows = std::fs::read_to_string(format!("{CASES}/{name}.expected"))
        .unwrap_or_else(|error| panic!("{name}: read oracle snapshot: {error}"))
        .lines()
        .map(str::to_owned)
        .collect();
    CaseFiles {
        source,
        oracle_rows,
    }
}

pub(crate) fn case_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(CASES)
        .expect("acceptance axes case directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "1"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names.len(),
        CASE_COUNT,
        "case set changed; regen via scripts/roff/fixtures/regen_acceptance_axes_gold.sh"
    );
    names
}

/// Validate the declared row facts of a card against the recorded oracle
/// window, so a re-recorded snapshot can never silently drift away from
/// the registered expectations.
pub(crate) fn validate_against_oracle(case: &AcceptanceCase, files: &CaseFiles) {
    let rows = &files.oracle_rows;
    case.assert_policy_consistency();
    if let Some(axis) = &case.gold.accepted_units {
        assert_eq!(
            region_words(rows),
            axis.expect,
            "{}: declared accepted units differ from the recorded oracle",
            case.id
        );
    }
    if let Some(axis) = &case.gold.forbidden_units {
        let words = region_words(rows);
        for unit in &axis.expect {
            assert!(
                !words.iter().any(|word| word == unit),
                "{}: forbidden unit {unit:?} appears in the recorded oracle",
                case.id
            );
        }
    }
    if let Some(axis) = &case.gold.row_count {
        assert_eq!(
            rows.len(),
            axis.expect,
            "{}: declared row count differs from the recorded oracle",
            case.id
        );
    }
    if let Some(axis) = &case.gold.separators {
        for expect in &axis.expect {
            assert!(
                separator_holds(rows, expect),
                "{}: declared separator does not hold on the recorded oracle",
                case.id
            );
        }
    }
    validate_hard_rows(case, rows);
    validate_blank_counts(case, rows);
    validate_scalar_ranges(case, rows);
    validate_sources(case, files);
}

fn validate_hard_rows(case: &AcceptanceCase, rows: &[String]) {
    let Some(axis) = &case.gold.hard_rows else {
        return;
    };
    for expect in &axis.expect {
        let (Some(left), Some(right)) = (
            row_containing(rows, expect.left),
            row_containing(rows, expect.right),
        ) else {
            panic!("{}: hard-row units are not in the recorded oracle", case.id);
        };
        let holds = match expect.relation {
            HardRowRelation::DifferentRows => left != right,
            HardRowRelation::SameRow => left == right,
        };
        assert!(
            holds,
            "{}: declared hard-row relation does not hold on the recorded oracle",
            case.id
        );
    }
}

fn validate_blank_counts(case: &AcceptanceCase, rows: &[String]) {
    let Some(axis) = &case.gold.blank_counts else {
        return;
    };
    for expect in &axis.expect {
        let actual = axis_model::blank_count_between(rows, expect.after, expect.before);
        assert_eq!(
            actual,
            Some(expect.count),
            "{}: declared blank count differs from the recorded oracle",
            case.id
        );
    }
}

fn validate_scalar_ranges(case: &AcceptanceCase, rows: &[String]) {
    let Some(axis) = &case.gold.scalar_ranges else {
        return;
    };
    let Some(identities) = &case.gold.identities else {
        panic!("{}: scalar ranges need identity labels", case.id);
    };
    let row = rows
        .iter()
        .find(|row| !is_blank_row(row))
        .unwrap_or_else(|| panic!("{}: oracle window has no content row", case.id));
    let scalars: Vec<char> = row.chars().collect();
    for range in &axis.expect {
        let Owner::Link(index) = range.owner else {
            panic!("{}: scalar range without a link owner", case.id);
        };
        let label = identities.expect[index].label;
        let observed: String = scalars[range.start..range.end].iter().collect();
        assert_eq!(
            observed, label,
            "{}: declared scalar range does not locate label {label:?} in the oracle row",
            case.id
        );
    }
}

fn validate_sources(case: &AcceptanceCase, files: &CaseFiles) {
    let Some(axis) = &case.gold.sources else {
        return;
    };
    let lines: Vec<&str> = files.source.lines().collect();
    for expect in &axis.expect {
        let line = lines
            .get(expect.line as usize - 1)
            .unwrap_or_else(|| panic!("{}: source has no line {}", case.id, expect.line));
        assert!(
            line.contains(expect.unit),
            "{}: authored line {} does not carry unit {:?}",
            case.id,
            expect.line,
            expect.unit
        );
    }
}

/// Drive the registered axes of one case against a comparison report:
/// `must` axes have to hold, `after_repair` axes have to be detected, and
/// every unregistered axis needs its applicability note.
pub(crate) fn assert_registered_axes(case: &AcceptanceCase, report: &axis_model::AxisReport) {
    for kind in AxisKind::ALL {
        let Some(status) = case.status_for(kind) else {
            assert!(
                case.note_for(kind).is_some(),
                "{}/{}: the {} axis is neither registered nor noted",
                case.family,
                case.id,
                kind.name()
            );
            continue;
        };
        match status {
            Status::MustHold => assert!(
                !report.fails_on(kind),
                "{}/{}: the {} axis failed: {}",
                case.family,
                case.id,
                kind.name(),
                report.details_for(kind)
            ),
            Status::HoldsAfterRepair => assert!(
                report.fails_on(kind),
                "{}/{}: the comparator no longer detects the registered {} divergence; \
                 flip its registration to must",
                case.family,
                case.id,
                kind.name()
            ),
        }
    }
}

/// Build a report for one case and observed rendering.
pub(crate) fn evaluate_case(
    case: &AcceptanceCase,
    files: &CaseFiles,
    observed: &axis_model::Observed,
) -> axis_model::AxisReport {
    axis_model::evaluate(case, &files.oracle_rows, observed)
}
