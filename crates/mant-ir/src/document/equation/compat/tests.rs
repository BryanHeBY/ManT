//! Source-neutral projection and exact wire-cache migration regressions.
use super::*;
use crate::{
    Block, EquationAtomRole, EquationFont, EquationKind, EquationOperandScope, EquationOperator,
    Inline,
};

fn atom(text: &str) -> EquationExpression {
    EquationExpression {
        kind: EquationKind::Text,
        font: EquationFont::None,
        operator: EquationOperator::None,
        atom_role: EquationAtomRole::Unknown,
        operand_scope: EquationOperandScope::Atomic,
        text: Some(text.into()),
        left: None,
        right: None,
        top: None,
        bottom: None,
        children: Vec::new(),
    }
}

fn container(kind: EquationKind, children: Vec<EquationExpression>) -> EquationExpression {
    EquationExpression {
        kind,
        text: None,
        children,
        ..atom("")
    }
}

#[test]
fn readable_lower_decorations_preserve_exact_authored_symbols() {
    // Independently authored IR must not interpret source-language escapes.
    for (symbol, expected) in [
        ("_", "x_"),
        ("\u{0332}", "x_"),
        ("", "x"),
        ("↓", "x _ (↓)"),
        ("custom marker", "x _ (custom marker)"),
        (r"\[ul]", r"x _ (\[ul])"),
    ] {
        let mut expression = atom("x");
        expression.bottom = Some(symbol.into());
        assert_eq!(expression.readable_text(), expected);
        let restored: EquationExpression =
            serde_json::from_value(serde_json::to_value(&expression).unwrap()).unwrap();
        assert_eq!(restored, expression);
        assert_eq!(restored.bottom.as_deref(), Some(symbol));
        assert_eq!(restored.readable_text(), expected);
    }
}

#[test]
fn normalized_native_under_keeps_its_established_reading() {
    // Fixed-CVS `.TH PROBE 3`, `.SH DESCRIPTION`, `.EQ`, `x under`, `.EN`
    // (one per line) produces terminal `x_` and MathML munder(x, _).
    // eqn.c::eqn_parse sets \[ul]; eqn_term.c::eqn_box emits `_`.
    // The producer has already normalized that source symbol in this IR.
    let mut expression = container(EquationKind::Sequence, vec![atom("x")]);
    expression.bottom = Some("_".into());
    assert_eq!(expression.readable_text(), "x_");
    assert!(expression.validate_projection("x_").is_ok());
}

#[test]
fn custom_lower_symbols_survive_nontransparent_matrix_axes_once() {
    let matrix = container(
        EquationKind::Matrix,
        vec![container(
            EquationKind::Group,
            vec![container(
                EquationKind::Pile,
                vec![
                    container(EquationKind::Sequence, vec![atom("a")]),
                    container(EquationKind::Sequence, vec![atom("b")]),
                ],
            )],
        )],
    );
    for column_axis in [false, true] {
        let mut expression = matrix.clone();
        let axis = &mut expression.children[0];
        let axis = if column_axis {
            &mut axis.children[0]
        } else {
            axis
        };
        axis.bottom = Some("↓".into());
        assert!(expression.matrix_view().is_none());
        let text = expression.readable_text();
        assert_eq!(text.matches('↓').count(), 1, "{text}");
        assert!(text.contains('a') && text.contains('b'), "{text}");
        assert!(text.contains(" _ (↓)"), "{text}");
    }
}

#[test]
fn inline_and_block_wire_normalize_only_the_exact_old_bottom_cache() {
    for (symbol, canonical) in [("↓", "x _ (↓)"), ("", "x")] {
        let mut expression = atom("x");
        expression.bottom = Some(symbol.into());
        for value in ["x_", canonical] {
            let wire = serde_json::json!({
                "type": "equation", "value": value, "expression": expression,
            });
            let inline: Inline = serde_json::from_value(wire.clone()).unwrap();
            let block: Block = serde_json::from_value(wire).unwrap();
            for encoded in [
                serde_json::to_value(&inline).unwrap(),
                serde_json::to_value(&block).unwrap(),
            ] {
                assert_eq!(encoded["value"], canonical);
                assert_eq!(encoded["expression"]["bottom"], symbol);
            }
        }
        let stale = serde_json::json!({
            "type": "equation", "value": "x_ stale", "expression": expression,
        });
        assert!(serde_json::from_value::<Inline>(stale.clone()).is_err());
        assert!(serde_json::from_value::<Block>(stale).is_err());
    }
}

#[test]
fn nested_wire_migration_preserves_operand_scope() {
    let mut lower = atom("x");
    lower.bottom = Some("↓".into());
    let mut fraction = container(EquationKind::Operation, vec![lower, atom("y")]);
    fraction.operator = EquationOperator::Fraction;
    assert_eq!(fraction.readable_text(), "(x _ (↓)) / y");
    let wire = serde_json::json!({
        "type": "strong",
        "children": [{
            "type": "equation", "value": "(x_) / y", "expression": fraction,
        }],
    });
    let inline: Inline = serde_json::from_value(wire).unwrap();
    let encoded = serde_json::to_value(inline).unwrap();
    assert_eq!(encoded["children"][0]["value"], "(x _ (↓)) / y");
    assert!(fraction.validate_projection("(x_) / y").is_err());
    assert!(fraction.validate_projection("(x _ (↓)) / y").is_ok());
}

#[test]
fn missing_legacy_block_structure_keeps_its_readable_value() {
    let block: Block = serde_json::from_value(serde_json::json!({
        "type": "equation", "value": "x_",
    }))
    .unwrap();
    let Block::Equation {
        value, expression, ..
    } = block
    else {
        panic!("expected an equation block");
    };
    assert_eq!(value, "x_");
    assert!(expression.is_none());
}

#[test]
fn migration_checks_structure_and_all_work_limits_before_projection() {
    let limits = EquationValidationLimits::default();
    let mut lower = atom("x");
    lower.bottom = Some("↓".into());
    let mut invalid = lower.clone();
    invalid.text = None;
    let nodes = container(
        EquationKind::Sequence,
        vec![lower.clone(); limits.max_nodes],
    );
    let mut depth = lower;
    for _ in 0..limits.max_depth {
        depth = container(EquationKind::Sequence, vec![depth]);
    }
    let mut text = atom(&"x".repeat(limits.max_text_bytes + 1));
    text.bottom = Some("↓".into());
    for (expression, expected) in [
        (
            invalid,
            EquationValidationError::InvalidNode(
                "text and opaque equations require text without children or an operator",
            ),
        ),
        (nodes, EquationValidationError::NodeLimit),
        (depth, EquationValidationError::DepthLimit),
        (text, EquationValidationError::TextByteLimit),
    ] {
        let mut value = "x_".to_owned();
        assert_eq!(
            expression.normalize_readable_cache(&mut value),
            Err(expected)
        );
        assert_eq!(value, "x_");
    }
}

#[test]
fn inline_and_block_wire_apply_the_aggregate_projection_budget() {
    let mut expression = atom(&"x".repeat(EquationValidationLimits::default().max_text_bytes + 1));
    expression.bottom = Some("↓".into());
    let wire = serde_json::json!({
        "type": "equation", "value": "x_", "expression": expression,
    });
    for error in [
        serde_json::from_value::<Inline>(wire.clone())
            .unwrap_err()
            .to_string(),
        serde_json::from_value::<Block>(wire)
            .unwrap_err()
            .to_string(),
    ] {
        assert!(error.contains("equation validation text byte limit exceeded"));
    }
}
