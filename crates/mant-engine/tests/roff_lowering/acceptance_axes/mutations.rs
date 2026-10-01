//! Comparator mutation self-checks (shared-execution guide section 5.8).
//!
//! The inputs are the frozen small samples of the committed case set: the
//! recorded oracle windows plus the semantic facts registered on each
//! card. Nothing here observes a product rendering. Every mutation below
//! must be caught by the axis the guide's table assigns to it, and the two
//! negative controls (allowed common-margin change, registered responsive
//! soft wrap) must not report content loss while their row checks keep
//! running.

use super::acceptance_cases::case_by_name;
use super::axis_model::{
    AcceptanceCase, Axis, AxisKind, AxisPolicy, ContentPolicy, GoldCard, IdentityPolicy,
    IndentPolicy, Observed, Owner, RowsPolicy, UnitStyle,
};
use super::{evaluate_case, load_case, validate_against_oracle};

fn rows(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_owned()).collect()
}

fn identity_pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(uri, label)| ((*uri).to_owned(), (*label).to_owned()))
        .collect()
}

/// Mutation 1: deleting the pending prefix glyph from `PY`.
fn deleted_pending_prefix() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("word_owner_pending_prefix");
    let files = load_case(case.id);
    let observed = Observed {
        rows: rows(&["     Y", "", ""]),
        identities: identity_pairs(&[("https://ex.org", "Y")]),
        ownership: vec![("Y".to_owned(), Owner::Link(0))],
        scalar_ranges: Vec::new(),
        styles: Vec::new(),
        sources: Vec::new(),
    };
    (
        "deleting P from PY",
        AxisKind::Content,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 2: resurrecting the rejected link suffix after `P`.
fn resurrected_rejected_suffix() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("rejected_suffix_revival");
    let files = load_case(case.id);
    let observed = Observed {
        rows: rows(&["     P", "     : https://ex.org AFTER", ""]),
        identities: identity_pairs(&[("https://ex.org", "https://ex.org")]),
        ..Observed::from_rows(&[])
    };
    (
        ": https://ex.org AFTER after P",
        AxisKind::ForbiddenContent,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 3: deleting the separator between AFTER and `BodyWord`.
fn deleted_word_separator() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("hang_final_gap");
    let files = load_case(case.id);
    let observed = Observed::from_rows(&rows(&["     D", "      AFTERBodyWord", ""]));
    (
        "AFTER BodyWord loses its separator",
        AxisKind::Separator,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 4: inserting a space into the oracle-proven join.
fn broken_proven_join() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("join_proven_hang");
    let files = load_case(case.id);
    let observed = Observed::from_rows(&rows(&["     D", "               E AFTER BodyWord", ""]));
    (
        "proven AFTERBodyWord join gets a space",
        AxisKind::Separator,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 5: turning the nonbreaking run-in blanks into ordinary blanks.
fn nonbreaking_became_breakable() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("run_in_nonbreaking");
    let files = load_case(case.id);
    let observed = Observed::from_rows(&rows(&["     HEAD  BodyWord", ""]));
    (
        "run-in NBSP cells become plain blanks",
        AxisKind::Separator,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 6: merging the authored hard rows of the two column cells.
fn merged_hard_rows() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("column_tail_hard_row");
    let files = load_case(case.id);
    let observed = Observed::from_rows(&rows(&["     D       RightWord", ""]));
    (
        "D and RightWord share one row",
        AxisKind::HardRows,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 7: collapsing the two explicit blank rows into one.
fn collapsed_explicit_blanks() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("tag_explicit_vspace");
    let files = load_case(case.id);
    let observed = Observed::from_rows(&rows(&[
        "     D",
        "",
        "     AFTER",
        "           BodyWord",
        "",
    ]));
    (
        "two explicit blank rows become one",
        AxisKind::BlankCount,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 8: appending an extra blank tail row to the cell window.
fn extra_tail_row() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("column_tail_hard_row");
    let files = load_case(case.id);
    let observed = Observed::from_rows(&rows(&["     D", "             RightWord", "", ""]));
    (
        "extra blank tail row",
        AxisKind::RowCount,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 9a: swapping the two column cells.
fn swapped_cells() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("column_cell_order");
    let files = load_case(case.id);
    let observed = Observed::from_rows(&rows(&["     RIGHT    LEFT", ""]));
    (
        "LEFT/RIGHT cells swapped",
        AxisKind::Content,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 9b: swapping the two same-name label occurrences.
fn swapped_label_occurrences() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("same_uri_occurrences");
    let files = load_case(case.id);
    let observed = Observed {
        identities: identity_pairs(&[("https://ex.org", "second"), ("https://ex.org", "first")]),
        rows: files.oracle_rows.clone(),
        ..Observed::from_rows(&[])
    };
    (
        "first/second occurrences swapped",
        AxisKind::Identity,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 10: moving the pending prefix into the link's visible range.
fn prefix_moved_into_link() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("word_owner_pending_prefix");
    let files = load_case(case.id);
    let observed = Observed {
        rows: rows(&["     PY", "", ""]),
        identities: identity_pairs(&[("https://ex.org", "PY")]),
        ownership: vec![
            ("P".to_owned(), Owner::Link(0)),
            ("Y".to_owned(), Owner::Link(0)),
        ],
        scalar_ranges: Vec::new(),
        styles: Vec::new(),
        sources: Vec::new(),
    };
    (
        "P joins the link's visible range",
        AxisKind::Ownership,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 11a: merging the two same-URI occurrences into one.
fn merged_same_uri_occurrences() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("same_uri_occurrences");
    let files = load_case(case.id);
    let observed = Observed {
        identities: identity_pairs(&[("https://ex.org", "firstsecond")]),
        rows: files.oracle_rows.clone(),
        ..Observed::from_rows(&[])
    };
    (
        "two same-URI occurrences merged",
        AxisKind::Identity,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 11b: deleting the empty typed identity.
fn deleted_empty_identity() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("rejected_suffix_revival");
    let files = load_case(case.id);
    let observed = Observed {
        rows: rows(&["     P", "", ""]),
        identities: Vec::new(),
        ..Observed::from_rows(&[])
    };
    (
        "empty typed identity deleted",
        AxisKind::Identity,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 12a: tampering a scalar label boundary by one scalar.
fn tampered_scalar_boundary() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("same_uri_occurrences");
    let files = load_case(case.id);
    let observed = Observed {
        scalar_ranges: vec![(Owner::Link(0), 5, 11), (Owner::Link(1), 27, 33)],
        rows: files.oracle_rows.clone(),
        ..Observed::from_rows(&[])
    };
    (
        "first label end 10 becomes 11",
        AxisKind::ScalarRange,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 12b: tampering one unit's style.
fn tampered_style() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("styled_units");
    let files = load_case(case.id);
    let observed = Observed {
        styles: vec![
            ("glowing".to_owned(), UnitStyle::Strong),
            ("rigid".to_owned(), UnitStyle::Strong),
            ("plain".to_owned(), UnitStyle::Plain),
        ],
        rows: files.oracle_rows.clone(),
        ..Observed::from_rows(&[])
    };
    (
        "glowing becomes strong",
        AxisKind::Style,
        evaluate_case(&case, &files, &observed),
    )
}

/// Mutation 12c: tampering one unit's authored source line.
fn tampered_source_line() -> (&'static str, AxisKind, super::axis_model::AxisReport) {
    let case = case_by_name("hang_final_gap");
    let files = load_case(case.id);
    let observed = Observed {
        sources: vec![("BodyWord".to_owned(), 15)],
        rows: files.oracle_rows.clone(),
        ..Observed::from_rows(&[])
    };
    (
        "BodyWord authored line 14 becomes 15",
        AxisKind::Source,
        evaluate_case(&case, &files, &observed),
    )
}

/// Every mutation of the guide's table is caught by its assigned axis.
#[test]
fn comparator_mutations_are_caught_by_the_expected_axis() {
    let checks = [
        deleted_pending_prefix(),
        resurrected_rejected_suffix(),
        deleted_word_separator(),
        broken_proven_join(),
        nonbreaking_became_breakable(),
        merged_hard_rows(),
        collapsed_explicit_blanks(),
        extra_tail_row(),
        swapped_cells(),
        swapped_label_occurrences(),
        prefix_moved_into_link(),
        merged_same_uri_occurrences(),
        deleted_empty_identity(),
        tampered_scalar_boundary(),
        tampered_style(),
        tampered_source_line(),
    ];
    assert_eq!(
        checks.len(),
        16,
        "the guide's twelve mutation rows map to these checks"
    );
    for (label, expected, report) in checks {
        assert!(
            report.fails_on(expected),
            "{label}: not caught by the {} axis",
            expected.name()
        );
    }
}

/// A control card exercising exact rows with common-margin omission: only
/// the page margin changes, so the row comparison runs and stays clean.
fn margin_control_case() -> AcceptanceCase {
    const NOTE: &str = "control card: exercises margin normalization only";
    AcceptanceCase {
        id: "margin-control",
        family: "comparator-control",
        policy: AxisPolicy {
            content: ContentPolicy::Exact,
            rows: RowsPolicy::ExactHardRows,
            indent: IndentPolicy::OmitCommonMargin,
            identity: IdentityPolicy::NotApplicable,
        },
        gold: GoldCard {
            accepted_units: Some(Axis::must(vec!["PY"])),
            row_count: Some(Axis::must(3)),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::ForbiddenContent, NOTE),
            (AxisKind::Separator, NOTE),
            (AxisKind::HardRows, NOTE),
            (AxisKind::BlankCount, NOTE),
            (AxisKind::Identity, NOTE),
            (AxisKind::Ownership, NOTE),
            (AxisKind::ScalarRange, NOTE),
            (AxisKind::Style, NOTE),
            (AxisKind::Source, NOTE),
        ],
    }
}

/// Negative controls: allowed layout variation must not read as content
/// loss, while the registered row checks still run.
#[test]
fn allowed_margin_and_soft_wrap_changes_do_not_report_content_loss() {
    // Changing only the common page margin stays invisible under the
    // exact-rows-with-margin-omission policy: content and rows both pass.
    let control = margin_control_case();
    let files = load_case("word_owner_pending_prefix");
    validate_against_oracle(&control, &files);
    let re_margined = Observed::from_rows(&rows(&["        PY", "", ""]));
    let report = evaluate_case(&control, &files, &re_margined);
    assert!(
        report.failures.is_empty(),
        "margin-only change reported failures: {:?}",
        report
            .failures
            .iter()
            .map(|failure| failure.kind.name())
            .collect::<Vec<_>>()
    );

    // A registered responsive soft wrap keeps every word and boundary, so
    // content and separator stay clean, while the pinned row count still
    // runs and reports the wrap.
    let case = case_by_name("hang_final_gap");
    let hang_files = load_case(case.id);
    let wrapped = Observed::from_rows(&rows(&["     D", "     AFTER", "     BodyWord", ""]));
    let report = evaluate_case(&case, &hang_files, &wrapped);
    assert!(
        !report.fails_on(AxisKind::Content),
        "responsive wrap reported content loss: {}",
        report.details_for(AxisKind::Content)
    );
    assert!(
        !report.fails_on(AxisKind::Separator),
        "responsive wrap reported a separator loss: {}",
        report.details_for(AxisKind::Separator)
    );
    assert!(
        report.fails_on(AxisKind::RowCount),
        "the pinned row count did not run against the wrapped rows"
    );
}

/// The mutation samples' own cards stay valid against the oracle, so a
/// drifted snapshot cannot silently weaken a mutation check.
#[test]
fn mutation_sample_cards_match_the_recorded_oracle() {
    for name in super::acceptance_cases::AUXILIARY_EXAMPLES {
        let case = case_by_name(name);
        let files = load_case(name);
        validate_against_oracle(&case, &files);
    }
}
