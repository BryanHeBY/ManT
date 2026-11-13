//! Work accounting survives failed validation and presentation candidates.
use super::*;
use mant_ir::{
    EquationAtomRole, EquationFont, EquationKind, EquationOperandScope, EquationOperator,
};

fn atom(value: &str) -> EquationExpression {
    EquationExpression {
        kind: EquationKind::Text,
        font: EquationFont::None,
        operator: EquationOperator::None,
        atom_role: EquationAtomRole::Identifier,
        operand_scope: EquationOperandScope::Atomic,
        text: Some(value.into()),
        left: None,
        right: None,
        top: None,
        bottom: None,
        children: Vec::new(),
    }
}

fn sequence(nodes: usize) -> EquationExpression {
    EquationExpression {
        kind: EquationKind::Sequence,
        atom_role: EquationAtomRole::Unknown,
        text: None,
        children: vec![atom("x"); nodes - 1],
        ..atom("")
    }
}

fn nested(depth: usize) -> EquationExpression {
    let mut expression = atom("x");
    for _ in 1..depth {
        expression = EquationExpression {
            kind: EquationKind::Sequence,
            atom_role: EquationAtomRole::Unknown,
            text: None,
            children: vec![expression],
            ..atom("")
        };
    }
    expression
}

fn set_remaining_budget(context: &MathContext, nodes: usize, bytes: usize) {
    let mut state = context.state.borrow_mut();
    state.nodes = MAX_MARKDOWN_MATH_NODES - nodes;
    state.input_bytes = MAX_MARKDOWN_MATH_INPUT_BYTES - bytes;
}

#[test]
fn invalid_nodes_and_stale_caches_charge_the_visited_prefix() {
    let mut early = atom("invalid");
    early.operator = EquationOperator::Fraction;
    let mut late = sequence(3);
    late.children[1].operator = EquationOperator::Fraction;
    for (expression, nodes, bytes) in [(early, 1, 0), (late, 3, 1), (sequence(3), 3, 2)] {
        let context = MathContext::export(true, MarkdownMath::Latex);
        assert!(context.latex(&expression, "stale").is_none());
        assert_eq!(context.state.borrow().nodes, nodes);
        assert_eq!(context.state.borrow().input_bytes, bytes);
        assert!(!context.state.borrow().exhausted);
        let tail = atom("tail");
        assert!(context.latex(&tail, "tail").is_some());
        assert_eq!(context.state.borrow().nodes, nodes + 1);
        let (issues, summary) = context.finish().unwrap();
        assert_eq!(summary.encoded_formulas, 1);
        assert_eq!(summary.fallback_formulas, 1);
        assert_eq!(issues[0].kind, MarkdownMathIssueKind::InvalidEquation);
    }
}

#[test]
fn an_invalid_last_node_keeps_first_error_priority_before_global_exhaustion() {
    let mut expression = sequence(3);
    expression.children[1].operator = EquationOperator::Fraction;
    let context = MathContext::export(true, MarkdownMath::Latex);
    set_remaining_budget(&context, 3, MAX_MARKDOWN_MATH_INPUT_BYTES);
    assert!(context.latex(&expression, "cache").is_none());
    assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES);
    assert_eq!(context.state.borrow().input_bytes, 1);
    assert!(!context.state.borrow().exhausted);

    let tail = atom("tail");
    assert!(context.latex(&tail, "tail").is_none());
    assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES);
    assert_eq!(context.state.borrow().input_bytes, 1);
    let (issues, summary) = context.finish().unwrap();
    assert!(summary.budget_exhausted);
    assert_eq!(summary.encoded_formulas, 0);
    assert_eq!(summary.fallback_formulas, 2);
    assert_eq!(issues[0].kind, MarkdownMathIssueKind::InvalidEquation);
    assert_eq!(
        issues[1].kind,
        MarkdownMathIssueKind::BudgetLimit(MarkdownMathLimit::Nodes)
    );
}

#[test]
fn local_limits_charge_failures_and_have_one_classification_across_policies() {
    for (expression, expected, nodes, bytes) in [
        (
            sequence(crate::math::MAX_MATH_NODES + 1),
            crate::math::MathLimit::Nodes,
            crate::math::MAX_MATH_NODES,
            crate::math::MAX_MATH_NODES - 1,
        ),
        (
            nested(crate::math::MAX_MATH_DEPTH + 1),
            crate::math::MathLimit::Depth,
            crate::math::MAX_MATH_DEPTH + 1,
            0,
        ),
        (
            atom(&"x".repeat(crate::math::MAX_MATH_INPUT_BYTES + 1)),
            crate::math::MathLimit::InputBytes,
            1,
            crate::math::MAX_MATH_INPUT_BYTES + 1,
        ),
    ] {
        expression
            .validate(EquationValidationLimits::default())
            .unwrap();
        for policy in [MathPolicy::Strict, MathPolicy::BestEffort] {
            let context = MathContext::new(policy, MarkdownMath::Latex);
            assert!(context.latex(&expression, "cache").is_none());
            assert_eq!(context.state.borrow().nodes, nodes);
            assert_eq!(context.state.borrow().input_bytes, bytes);
            assert_eq!(
                context.finish(),
                Err(MarkdownMathError::Encoding {
                    formula: 0,
                    error: crate::math::MathError::LimitExceeded { limit: expected },
                })
            );
        }
        let context = MathContext::export(true, MarkdownMath::Latex);
        assert!(context.latex(&expression, "cache").is_none());
        assert_eq!(context.state.borrow().nodes, nodes);
        assert_eq!(context.state.borrow().input_bytes, bytes);
        let (issues, summary) = context.finish().unwrap();
        assert!(!summary.budget_exhausted);
        assert_eq!(summary.fallback_formulas, 1);
        assert_eq!(
            issues[0].kind,
            MarkdownMathIssueKind::EncodingLimit(expected)
        );
    }
}

#[test]
fn global_limits_win_at_ties_and_after_an_oversized_field_crosses_the_remainder() {
    for (remaining_nodes, remaining_bytes, expression, expected) in [
        (
            2,
            MAX_MARKDOWN_MATH_INPUT_BYTES,
            sequence(3),
            MarkdownMathLimit::Nodes,
        ),
        (
            MAX_MARKDOWN_MATH_NODES,
            2,
            atom("xyz"),
            MarkdownMathLimit::InputBytes,
        ),
        (
            crate::math::MAX_MATH_NODES,
            MAX_MARKDOWN_MATH_INPUT_BYTES,
            sequence(crate::math::MAX_MATH_NODES + 1),
            MarkdownMathLimit::Nodes,
        ),
        (
            MAX_MARKDOWN_MATH_NODES,
            crate::math::MAX_MATH_INPUT_BYTES,
            atom(&"x".repeat(crate::math::MAX_MATH_INPUT_BYTES + 1)),
            MarkdownMathLimit::InputBytes,
        ),
        (
            MAX_MARKDOWN_MATH_NODES,
            crate::math::MAX_MATH_INPUT_BYTES + 1,
            atom(&"x".repeat(crate::math::MAX_MATH_INPUT_BYTES + 2)),
            MarkdownMathLimit::InputBytes,
        ),
    ] {
        for policy in [MathPolicy::Strict, MathPolicy::BestEffort] {
            let context = MathContext::new(policy, MarkdownMath::Latex);
            set_remaining_budget(&context, remaining_nodes, remaining_bytes);
            assert!(context.latex(&expression, "cache").is_none());
            assert_eq!(context.finish(), Err(limit(expected)));
        }
        let context = MathContext::export(true, MarkdownMath::Latex);
        set_remaining_budget(&context, remaining_nodes, remaining_bytes);
        assert!(context.latex(&expression, "cache").is_none());
        assert!(context.state.borrow().exhausted);
        if expected == MarkdownMathLimit::Nodes {
            assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES);
        } else {
            assert_eq!(
                context.state.borrow().input_bytes,
                MAX_MARKDOWN_MATH_INPUT_BYTES + 1
            );
        }
        let spent = (
            context.state.borrow().nodes,
            context.state.borrow().input_bytes,
        );
        let tail = atom("tail");
        assert!(context.latex(&tail, "tail").is_none());
        assert_eq!(
            (
                context.state.borrow().nodes,
                context.state.borrow().input_bytes
            ),
            spent
        );
        let (issues, summary) = context.finish().unwrap();
        assert!(summary.budget_exhausted);
        assert_eq!(summary.encoded_formulas, 0);
        assert_eq!(summary.fallback_formulas, 1);
        assert_eq!(issues[0].kind, MarkdownMathIssueKind::BudgetLimit(expected));
    }
}

#[test]
fn memoized_successes_and_failures_skip_scans_while_new_attempts_pay() {
    let small = sequence(3);
    let value = small.readable_text();
    let context = MathContext::export(true, MarkdownMath::Latex);
    assert!(context.latex(&small, &value).is_some());
    assert!(context.latex(&small, &value).is_some());
    assert_eq!(context.state.borrow().nodes, 3);
    assert_eq!(context.state.borrow().input_bytes, 2);

    let large = sequence(crate::math::MAX_MATH_NODES + 1);
    let duplicate = large.clone();
    let context = MathContext::export(true, MarkdownMath::Latex);
    assert!(context.latex(&large, "cache").is_none());
    assert!(context.latex(&large, "cache").is_none());
    assert_eq!(context.state.borrow().nodes, crate::math::MAX_MATH_NODES);
    assert!(context.latex(&duplicate, "cache").is_none());
    assert_eq!(
        context.state.borrow().nodes,
        2 * crate::math::MAX_MATH_NODES
    );
    assert_eq!(
        context.state.borrow().input_bytes,
        2 * (crate::math::MAX_MATH_NODES - 1)
    );

    let context = MathContext::export(false, MarkdownMath::Latex);
    assert!(context.latex(&large, "cache").is_none());
    assert!(context.latex(&large, "cache").is_none());
    assert_eq!(
        context.state.borrow().nodes,
        2 * crate::math::MAX_MATH_NODES
    );
    assert_eq!(
        context.state.borrow().input_bytes,
        2 * (crate::math::MAX_MATH_NODES - 1)
    );
}

#[test]
fn presentation_candidates_keep_their_charge_after_final_readable_selection() {
    let expressions = [atom("x"), atom("y"), atom("z")];
    let context = MathContext::export(true, MarkdownMath::Latex);
    set_remaining_budget(&context, 2, MAX_MARKDOWN_MATH_INPUT_BYTES);
    context.reading(
        &expressions[0],
        "x",
        MarkdownMathIssueKind::DelimiterContext,
    );
    assert!(context.latex(&expressions[0], "x").is_some());
    context.reading(
        &expressions[0],
        "x",
        MarkdownMathIssueKind::DelimiterContext,
    );
    assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES - 1);
    assert_eq!(context.state.borrow().input_bytes, 1);
    assert!(context.latex(&expressions[1], "y").is_some());
    assert!(context.latex(&expressions[2], "z").is_none());
    assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES);
    let (_, summary) = context.finish().unwrap();
    assert!(summary.budget_exhausted);
    assert_eq!(summary.encoded_formulas, 1);
    assert_eq!(summary.fallback_formulas, 2);
}

#[test]
fn reading_contexts_charge_failed_validation_before_their_fallback() {
    let expression = sequence(5);
    let value = expression.readable_text();
    let nodes = [Inline::Equation { expression, value }];
    for kind in [
        MarkdownMathIssueKind::LiteralContext,
        MarkdownMathIssueKind::TableContext,
    ] {
        let context = MathContext::export(true, MarkdownMath::Latex);
        set_remaining_budget(&context, 4, MAX_MARKDOWN_MATH_INPUT_BYTES);
        context.reading_inlines(&nodes, kind);
        assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES);
        assert_eq!(context.state.borrow().input_bytes, 3);
        assert!(context.state.borrow().exhausted);
        let tail = atom("tail");
        assert!(context.latex(&tail, "tail").is_none());
        assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES);
        let (issues, summary) = context.finish().unwrap();
        assert_eq!(summary.fallback_formulas, 1);
        assert_eq!(
            issues[0].kind,
            MarkdownMathIssueKind::BudgetLimit(MarkdownMathLimit::Nodes)
        );
    }
}

#[test]
fn exhausted_work_stops_new_scans_but_keeps_known_owner_updates() {
    let expressions = [
        atom("x"),
        atom("y"),
        sequence(crate::math::MAX_MATH_NODES + 1),
    ];
    let context = MathContext::export(true, MarkdownMath::Latex);
    set_remaining_budget(&context, 1, MAX_MARKDOWN_MATH_INPUT_BYTES);
    assert!(context.latex(&expressions[0], "x").is_some());
    assert!(context.latex(&expressions[1], "y").is_none());
    assert!(context.latex(&expressions[2], "cache").is_none());
    assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES);
    assert_eq!(context.state.borrow().input_bytes, 1);
    assert!(context.latex(&expressions[0], "x").is_some());
    context.reading(
        &expressions[0],
        "x",
        MarkdownMathIssueKind::DelimiterContext,
    );
    assert_eq!(context.state.borrow().nodes, MAX_MARKDOWN_MATH_NODES);
    let (_, summary) = context.finish().unwrap();
    assert!(summary.budget_exhausted);
    assert_eq!(summary.encoded_formulas, 0);
    assert_eq!(summary.fallback_formulas, 2);
}

#[test]
fn zero_text_formula_can_use_an_exactly_filled_byte_allowance() {
    let expression = atom("");
    let context = MathContext::export(true, MarkdownMath::Latex);
    set_remaining_budget(&context, MAX_MARKDOWN_MATH_NODES, 0);
    assert!(context.latex(&expression, "").is_some());
    assert_eq!(context.state.borrow().nodes, 1);
    assert_eq!(
        context.state.borrow().input_bytes,
        MAX_MARKDOWN_MATH_INPUT_BYTES
    );
    let (_, summary) = context.finish().unwrap();
    assert!(!summary.budget_exhausted);
    assert_eq!(summary.encoded_formulas, 1);
}

#[test]
fn mathml_variants_each_reserve_their_encoded_bytes_once() {
    for order in [[false, true], [true, false]] {
        let context = MathContext::export(true, MarkdownMath::Mathml);
        let expression = atom("x");
        let mut expected = 0;
        for display in order {
            let markup = context.encoded(&expression, "x", display).unwrap();
            expected += markup.len() + 16;
            assert_eq!(context.state.borrow().output_bytes, expected);
            assert_eq!(context.encoded(&expression, "x", display).unwrap(), markup);
            assert_eq!(context.state.borrow().output_bytes, expected);
        }
        assert_eq!(context.state.borrow().nodes, 1);
        assert_eq!(context.state.borrow().input_bytes, 1);
        assert_eq!(context.finish().unwrap().1.encoded_formulas, 1);
    }
}

#[test]
fn mathml_text_expansion_limit_is_local_and_checked_exports_report_it() {
    let expression = atom(&"*".repeat(crate::math::MAX_MATH_OUTPUT_BYTES / 4));
    let value = expression.readable_text();
    let tail = atom("tail");
    let context = MathContext::export(true, MarkdownMath::Mathml);
    assert!(context.encoded(&expression, &value, false).is_none());
    assert!(!context.state.borrow().exhausted);
    assert!(context.encoded(&tail, "tail", false).is_some());
    let (issues, summary) = context.finish().unwrap();
    assert_eq!(summary.formulas, 2);
    assert_eq!(summary.encoded_formulas, 1);
    assert_eq!(summary.fallback_formulas, 1);
    assert_eq!(
        issues[0].kind,
        MarkdownMathIssueKind::EncodingLimit(crate::math::MathLimit::OutputBytes)
    );
    for policy in [MathPolicy::Strict, MathPolicy::BestEffort] {
        let context = MathContext::new(policy, MarkdownMath::Mathml);
        assert!(context.encoded(&expression, &value, false).is_none());
        assert!(matches!(
            context.finish(),
            Err(MarkdownMathError::Encoding {
                formula: 0,
                error: crate::math::MathError::LimitExceeded {
                    limit: crate::math::MathLimit::OutputBytes
                }
            })
        ));
    }
}
