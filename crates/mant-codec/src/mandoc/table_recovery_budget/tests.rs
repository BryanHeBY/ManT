use super::{Exhaustion, Limits, TableRecoveryBudget};
use mant_ir::{Inline, LinkTarget};

#[test]
fn admission_includes_the_limit_and_refused_work_is_not_refunded() {
    let mut budget = TableRecoveryBudget::with_limits(Limits {
        scan: 3,
        input: 3,
        attempts: 2,
        ..Limits::default()
    });
    assert!(budget.charge_scan(2));
    assert!(budget.charge_scan(1));
    assert!(budget.begin_candidate(1));
    // A declined semantic candidate still spent its attempt/input.
    assert!(budget.begin_candidate(2));
    assert_eq!(budget.input, 3);
    assert_eq!(budget.attempts(), 2);
    assert!(!budget.begin_candidate(0));
    assert_eq!(budget.attempts(), 3);
    assert_eq!(budget.exhaustion(), Some(Exhaustion::Attempts));
    assert!(!budget.charge_scan(0));
    assert_eq!(budget.scan, 3);
}

#[test]
fn input_and_scan_have_independent_cumulative_boundaries() {
    for reason in [Exhaustion::Input, Exhaustion::Scan] {
        let mut budget = TableRecoveryBudget::with_limits(Limits {
            scan: 2,
            input: 2,
            ..Limits::default()
        });
        assert!(budget.charge(1, reason));
        assert!(budget.charge(1, reason));
        assert!(!budget.charge(1, reason));
        assert_eq!(budget.exhaustion(), Some(reason));
    }
}

#[test]
fn overflow_cannot_restore_an_exhausted_allowance() {
    let mut budget = TableRecoveryBudget::default();
    assert!(!budget.charge_input(usize::MAX));
    assert_eq!(budget.input, usize::MAX);
    assert!(!budget.charge_input(1));
    assert_eq!(budget.input, usize::MAX);
}

fn labeled_output() -> Vec<Inline> {
    vec![Inline::Strong {
        children: vec![Inline::Link {
            target: LinkTarget::External {
                uri: "dest".to_owned(),
            },
            title: Some("t".to_owned()),
            children: vec![Inline::Text {
                value: "value".to_owned(),
            }],
        }],
    }]
}

#[test]
fn output_admission_counts_nested_nodes_and_nonvisible_destination_strings() {
    let nodes = labeled_output();
    let mut exact = TableRecoveryBudget::with_limits(Limits {
        output_nodes: 3,
        output_bytes: 10,
        ..Limits::default()
    });
    assert!(exact.charge_output(&nodes));
    assert_eq!((exact.output_nodes, exact.output_bytes), (3, 10));
    for (node_limit, byte_limit, reason) in [
        (2, 10, Exhaustion::OutputNodes),
        (3, 9, Exhaustion::OutputBytes),
    ] {
        let mut refused = TableRecoveryBudget::with_limits(Limits {
            output_nodes: node_limit,
            output_bytes: byte_limit,
            ..Limits::default()
        });
        assert!(!refused.charge_output(&nodes));
        assert_eq!(refused.exhaustion(), Some(reason));
        assert!(!refused.begin_candidate(0));
    }
}

#[test]
fn local_fragment_refusal_keeps_spent_work_and_allows_smaller_candidates() {
    let mut budget = TableRecoveryBudget::default();
    assert!(budget.charge_input(4));
    assert!(!budget.begin_candidate(super::MAX_FRAGMENT_BYTES + 1));
    assert_eq!(budget.refused_fragments(), 1);
    assert_eq!(budget.exhaustion(), None);
    assert_eq!(budget.input, 4);
    assert_eq!(budget.attempts(), 1);
    assert!(budget.begin_candidate(3));
    assert_eq!(budget.input, 7);
    assert_eq!(budget.attempts(), 2);
}

#[test]
fn exact_fragment_size_is_admitted_before_the_larger_synthetic_input_check() {
    let mut budget = TableRecoveryBudget::default();
    assert!(budget.begin_candidate(super::MAX_FRAGMENT_BYTES - 1));
    assert!(budget.begin_candidate(super::MAX_FRAGMENT_BYTES));
    assert!(!budget.begin_candidate(super::MAX_FRAGMENT_BYTES + 1));
    assert_eq!(budget.attempts(), 3);
    assert_eq!(budget.refused_fragments(), 1);
    assert_eq!(budget.exhaustion(), None);
}

#[test]
fn one_output_traversal_retains_exact_visible_copy_bytes_without_destination_markup() {
    let mut nodes = labeled_output();
    nodes.push(Inline::line_break());
    nodes.push(Inline::Code {
        value: "é".to_owned(),
    });
    let mut budget = TableRecoveryBudget::default();
    let bytes = budget.charge_output_with_text_bytes(&nodes).unwrap();
    assert_eq!(bytes, 8);
    assert_eq!(bytes, mant_ir::inline_plain_text(&nodes).len());
    assert_eq!(budget.output_bytes, 12);
    assert_eq!(budget.output_nodes, 5);
}
