use super::{CoverageCheckView, CoverageIssueView, transfer_coverage};
use crate::annotated::{AnnotatedSource, AnnotationCheckState, AnnotationScope};

fn checks() -> Vec<CoverageCheckView> {
    (1..=3)
        .flat_map(|producer| {
            (1..=8).map(move |dimension| CoverageCheckView {
                producer,
                dimension,
                state: 2,
                reserved: 0,
            })
        })
        .collect()
}

#[test]
fn coverage_accepts_zero_or_multiple_issues_only_for_unverified_slots() {
    let mut checks = checks();
    assert!(transfer_coverage(&checks, &[], &[], &[]).is_ok());
    checks[0].state = 3;
    let issue = CoverageIssueView {
        producer: 1,
        dimension: 1,
        reason: 2,
        scope: 1,
        ..CoverageIssueView::default()
    };
    assert!(transfer_coverage(&checks, &[], &[], &[]).is_err());
    let coverage = transfer_coverage(&checks, &[issue, issue], &[], &[]).unwrap();
    assert_eq!(coverage.issues.len(), 2);
    assert_eq!(coverage.checks[0].state, AnnotationCheckState::Unverified);
    checks[0].state = 1;
    assert!(transfer_coverage(&checks, &[issue], &[], &[]).is_err());
}

#[test]
fn coverage_rejects_duplicate_slots_native_pending_and_unknown_scopes() {
    let mut table = checks();
    table[1] = table[0];
    assert!(transfer_coverage(&table, &[], &[], &[]).is_err());
    table = checks();
    table[0].state = 4;
    assert!(transfer_coverage(&table, &[], &[], &[]).is_err());
    table[0].state = 3;
    let issue = CoverageIssueView {
        producer: 1,
        dimension: 1,
        reason: 2,
        scope: 2,
        scope_key: 1,
        ..CoverageIssueView::default()
    };
    assert!(transfer_coverage(&table, &[issue], &[], &[]).is_err());
    let document_issue = CoverageIssueView { scope: 1, ..issue };
    assert!(transfer_coverage(&table, &[document_issue], &[], &[]).is_err());
}

#[test]
fn source_scope_requires_a_known_source_key_and_valid_authored_triplet() {
    let mut checks = checks();
    checks[6].state = 3;
    let source = AnnotatedSource {
        key: 1,
        identity_kind: 1,
        format: 1,
        coordinate_kind: 1,
        logical_name: String::new(),
        decoded_length: 0,
        hash: None,
    };
    let issue = CoverageIssueView {
        producer: 1,
        dimension: 7,
        reason: 2,
        scope: 5,
        scope_key: 1,
        ..CoverageIssueView::default()
    };
    assert!(transfer_coverage(&checks, &[issue], &[], &[]).is_err());
    let coverage =
        transfer_coverage(&checks, &[issue], &[], std::slice::from_ref(&source)).unwrap();
    assert_eq!(coverage.issues[0].scope, AnnotationScope::Source(1));
    // The native result check validates this triplet against the exact
    // source map; this transfer layer enforces its shape and SourceKey.
    let authored = CoverageIssueView {
        source: 1,
        line: 1,
        column: 1,
        ..issue
    };
    assert!(transfer_coverage(&checks, &[authored], &[], std::slice::from_ref(&source)).is_ok());
    let partial = CoverageIssueView {
        column: 0,
        ..authored
    };
    assert!(transfer_coverage(&checks, &[partial], &[], std::slice::from_ref(&source)).is_err());
    let mismatched = CoverageIssueView {
        source: 2,
        ..authored
    };
    let second = AnnotatedSource {
        key: 2,
        ..source.clone()
    };
    assert!(transfer_coverage(&checks, &[mismatched], &[], &[source, second]).is_err());
}
