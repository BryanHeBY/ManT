//! Independently authored IR contracts; no native source execution is inferred.
use super::*;

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
fn roles_are_explicit_independent_and_canonical() {
    for (role, spelling) in [
        (EquationAtomRole::Unknown, "unknown"),
        (EquationAtomRole::Identifier, "identifier"),
        (EquationAtomRole::Number, "number"),
        (EquationAtomRole::Operator, "operator"),
        (EquationAtomRole::Function, "function"),
        (EquationAtomRole::Literal, "literal"),
    ] {
        let expression = EquationExpression {
            atom_role: role,
            font: EquationFont::Italic,
            ..atom("sin")
        };
        let encoded = serde_json::to_value(&expression).unwrap();
        if role == EquationAtomRole::Unknown {
            assert!(encoded.get("atomRole").is_none());
        } else {
            assert_eq!(encoded["atomRole"], spelling);
        }
        let restored: EquationExpression = serde_json::from_value(encoded).unwrap();
        assert_eq!(restored, expression);
        assert_eq!(restored.readable_text(), "sin");
        assert_eq!(restored.font, EquationFont::Italic);
    }
}

#[test]
fn opaque_payload_is_not_an_inferred_math_atom() {
    let expression = EquationExpression {
        kind: EquationKind::Opaque,
        operand_scope: EquationOperandScope::Compound,
        ..atom("a / (b + c)")
    };
    assert!(expression.validate_shallow().is_ok());
    assert!(expression.needs_operand_group());
    assert_eq!(expression.readable_text(), "a / (b + c)");
    let encoded = serde_json::to_value(&expression).unwrap();
    assert_eq!(encoded["kind"], "opaque");
    assert_eq!(encoded["operandScope"], "compound");
    assert_eq!(
        serde_json::from_value::<EquationExpression>(encoded).unwrap(),
        expression
    );
    let mut invalid = expression;
    invalid.atom_role = EquationAtomRole::Function;
    assert!(invalid.validate_shallow().is_err());
}

#[test]
fn operation_views_keep_exact_original_operand_owners() {
    for (operator, count) in [
        (EquationOperator::Fraction, 2),
        (EquationOperator::SquareRoot, 1),
        (EquationOperator::Superscript, 2),
        (EquationOperator::Subscript, 2),
        (EquationOperator::SubscriptSuperscript, 3),
        (EquationOperator::UpperLimit, 2),
        (EquationOperator::LowerLimit, 2),
        (EquationOperator::LowerUpperLimits, 3),
    ] {
        let mut expression = container(
            EquationKind::Operation,
            (0..count).map(|_| atom("x")).collect(),
        );
        expression.operator = operator;
        expression.children[0] = container(EquationKind::Group, vec![atom("x")]);
        expression.children[0].font = EquationFont::Bold;
        let first = match expression.operation_view().unwrap() {
            EquationOperationRef::Fraction { numerator, .. } => numerator,
            EquationOperationRef::SquareRoot { radicand } => radicand,
            EquationOperationRef::Superscript { base, .. }
            | EquationOperationRef::Subscript { base, .. }
            | EquationOperationRef::SubscriptSuperscript { base, .. }
            | EquationOperationRef::UpperLimit { base, .. }
            | EquationOperationRef::LowerLimit { base, .. }
            | EquationOperationRef::LowerUpperLimits { base, .. } => base,
        };
        assert!(std::ptr::eq(first, &raw const expression.children[0]));
        assert_eq!(first.kind, EquationKind::Group);
        assert_eq!(first.font, EquationFont::Bold);
        expression.children.push(atom("extra"));
        assert!(expression.operation_view().is_none());
        assert!(expression.validate_shallow().is_err());
        assert!(
            serde_json::from_value::<EquationExpression>(serde_json::to_value(expression).unwrap())
                .is_err()
        );
    }
}

#[test]
fn node_combinations_are_enforced_locally_in_serde() {
    let valid = serde_json::to_value(atom("x")).unwrap();
    let mut cases = Vec::new();
    let mut invalid = valid.clone();
    invalid["text"] = serde_json::Value::Null;
    cases.push(invalid);
    let mut invalid = valid.clone();
    invalid["children"] = serde_json::json!([valid.clone()]);
    cases.push(invalid);
    let mut invalid = valid.clone();
    invalid["operator"] = "fraction".into();
    cases.push(invalid);
    let mut invalid = valid.clone();
    invalid["operandScope"] = "compound".into();
    cases.push(invalid);
    let mut invalid = valid.clone();
    invalid["kind"] = "sequence".into();
    cases.push(invalid);
    let mut invalid = valid.clone();
    invalid["size"] = 12.into();
    cases.push(invalid);
    for invalid in cases {
        assert!(
            serde_json::from_value::<EquationExpression>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
    for kind in [
        EquationKind::Sequence,
        EquationKind::Group,
        EquationKind::Pile,
        EquationKind::Matrix,
    ] {
        let empty = container(kind, Vec::new());
        assert!(empty.validate_shallow().is_ok());
        assert!(
            serde_json::from_value::<EquationExpression>(serde_json::to_value(empty).unwrap())
                .is_ok()
        );
    }
    assert!(
        serde_json::from_value::<EquationExpression>(serde_json::to_value(atom("")).unwrap())
            .is_ok()
    );
}

#[test]
fn aggregate_limits_are_inclusive_and_do_not_generate_output() {
    let expression = container(EquationKind::Sequence, vec![atom("α"), atom("x")]);
    let limits = EquationValidationLimits {
        max_nodes: 3,
        max_depth: 2,
        max_text_bytes: 3,
    };
    assert_eq!(expression.validate(limits), Ok(()));
    assert_eq!(
        expression.validate(EquationValidationLimits {
            max_nodes: 2,
            ..limits
        }),
        Err(EquationValidationError::NodeLimit)
    );
    assert_eq!(
        expression.validate(EquationValidationLimits {
            max_depth: 1,
            ..limits
        }),
        Err(EquationValidationError::DepthLimit)
    );
    assert_eq!(
        expression.validate(EquationValidationLimits {
            max_text_bytes: 2,
            ..limits
        }),
        Err(EquationValidationError::TextByteLimit)
    );
    let mut invalid_later = expression;
    invalid_later.children[1].text = None;
    assert_eq!(
        invalid_later.validate(EquationValidationLimits {
            max_nodes: 2,
            ..limits
        }),
        Err(EquationValidationError::NodeLimit)
    );
    assert!(matches!(
        invalid_later.validate(limits),
        Err(EquationValidationError::InvalidNode(_))
    ));
    let mut decorated = atom("x");
    decorated.top = Some("→".into());
    decorated.left = Some("(".into());
    assert_eq!(
        decorated.validate(EquationValidationLimits {
            max_text_bytes: 4,
            ..Default::default()
        }),
        Err(EquationValidationError::TextByteLimit)
    );
}

#[test]
fn matrix_view_keeps_ragged_rows_and_all_presentation_wrappers() {
    let mut first = container(
        EquationKind::Pile,
        vec![container(EquationKind::Sequence, vec![atom("a")])],
    );
    first.font = EquationFont::Bold;
    let mut second = container(
        EquationKind::Pile,
        vec![
            container(EquationKind::Group, vec![atom("b")]),
            container(EquationKind::Sequence, vec![atom("c")]),
        ],
    );
    second.children[0].left = Some("[".into());
    let mut group = container(EquationKind::Group, vec![first, second]);
    group.font = EquationFont::Italic;
    let matrix = container(EquationKind::Matrix, vec![group]);
    let view = matrix.matrix_view().unwrap();
    assert!(std::ptr::eq(
        view.columns_group,
        &raw const matrix.children[0]
    ));
    assert_eq!(view.columns_group.font, EquationFont::Italic);
    assert_eq!(view.columns[0].font, EquationFont::Bold);
    assert_eq!(view.columns[0].children.len(), 1);
    assert_eq!(view.columns[1].children.len(), 2);
    assert_eq!(view.columns[1].children[0].left.as_deref(), Some("["));
    assert_eq!(view.columns[1].children[1].readable_text(), "c");
    let alternative = container(EquationKind::Matrix, vec![atom("all operands remain")]);
    assert!(
        alternative
            .validate(EquationValidationLimits::default())
            .is_ok()
    );
    assert!(alternative.matrix_view().is_none());
    assert_eq!(alternative.readable_text(), "matrix(all operands remain)");
}

#[test]
fn nontransparent_matrix_axes_preserve_visible_content_in_fallback() {
    // Independently authored IR, not an expectation for a native renderer.
    // Axis symbols are content owners, never matrix-cell padding or borders.
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
        for (field, symbol) in [("left", "["), ("right", "]"), ("top", "→"), ("bottom", "_")] {
            let mut expression = matrix.clone();
            let group = &mut expression.children[0];
            let axis = if column_axis {
                &mut group.children[0]
            } else {
                group
            };
            match field {
                "left" => axis.left = Some(symbol.into()),
                "right" => axis.right = Some(symbol.into()),
                "top" => axis.top = Some(symbol.into()),
                "bottom" => axis.bottom = Some(symbol.into()),
                _ => unreachable!(),
            }
            assert!(expression.matrix_view().is_none());
            let text = expression.readable_text();
            assert!(
                text.contains(symbol),
                "{field} column={column_axis}: {text}"
            );
            assert!(text.contains('a') && text.contains('b'), "{text}");
            let restored: EquationExpression =
                serde_json::from_value(serde_json::to_value(&expression).unwrap()).unwrap();
            assert_eq!(restored, expression);
            assert_eq!(restored.readable_text(), text);
        }
    }
    let mut explicit_empty = matrix;
    explicit_empty.children[0].left = Some(String::new());
    assert!(explicit_empty.matrix_view().is_none());
}

#[test]
fn a_pile_decoration_does_not_change_the_first_rows_scope() {
    // Independently authored IR: the row wrapper is already normalized.
    // Exact control `pile { { a } above b + c }` was checked with the fixed
    // CVS -Ttree/-Thtml/-Tutf8 oracle. eqn_term.c::eqn_box skips the singleton
    // first-row wrapper and joins the following explicit group without space.
    // A decoration belongs to the complete pile, not only its first row.
    let mut pile = container(
        EquationKind::Pile,
        vec![
            container(
                EquationKind::Sequence,
                vec![container(EquationKind::Group, vec![atom("a")])],
            ),
            container(EquationKind::Group, vec![atom("b"), atom("+"), atom("c")]),
        ],
    );
    let baseline = pile.readable_text();
    // Preserve the established adjacent-group spelling; the regression is
    // about scope boundaries, not inserting a new visible row separator.
    assert_eq!(baseline, "(a)(b + c)");
    pile.top = Some("@".into());
    assert_eq!(pile.readable_text(), format!("{baseline}@"));
    pile.top = None;
    pile.bottom = Some("_".into());
    assert_eq!(pile.readable_text(), format!("{baseline}_"));
    assert!(pile.validate(EquationValidationLimits::default()).is_ok());
}

#[test]
fn decoration_views_never_reinterpret_source_escapes() {
    for (symbol, expected) in [
        ("˙", EquationDecorationKind::Dot),
        ("¨", EquationDecorationKind::DoubleDot),
        ("^", EquationDecorationKind::Hat),
        ("~", EquationDecorationKind::Tilde),
        ("‾", EquationDecorationKind::Overbar),
        ("→", EquationDecorationKind::Vector),
        ("↔", EquationDecorationKind::BidirectionalVector),
        (r"\[->]", EquationDecorationKind::Custom),
        ("", EquationDecorationKind::Custom),
    ] {
        let mut expression = atom("x");
        expression.top = Some(symbol.into());
        let view = expression.top_decoration().unwrap();
        assert_eq!(view.kind, expected);
        assert_eq!(view.symbol, symbol);
    }
    let mut expression = atom("x");
    expression.bottom = Some("_".into());
    assert_eq!(
        expression.bottom_decoration().unwrap().kind,
        EquationDecorationKind::Underline
    );
}

#[test]
fn standalone_block_and_inline_decoding_check_cached_projection() {
    let expression = serde_json::to_value(atom("x")).unwrap();
    let block =
        serde_json::json!({"type":"equation", "value":"different", "expression":expression});
    assert!(serde_json::from_value::<crate::Block>(block).is_err());
    let inline =
        serde_json::json!({"type":"equation", "value":"different", "expression":expression});
    assert!(serde_json::from_value::<crate::Inline>(inline.clone()).is_err());
    assert!(
        serde_json::from_value::<crate::Inline>(
            serde_json::json!({"type":"strong", "children":[inline]})
        )
        .is_err()
    );
    let inline = serde_json::json!({"type":"equation", "value":"x", "expression":expression});
    assert!(serde_json::from_value::<crate::Inline>(inline).is_ok());
}

#[test]
fn schema_carries_closed_fields_and_local_shape_conditions() {
    let schema = serde_json::to_value(schemars::schema_for!(EquationExpression)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert!(schema["properties"].get("size").is_none());
    assert!(schema["properties"].get("atomRole").is_some());
    assert_eq!(schema["allOf"].as_array().unwrap().len(), 7);
    assert_eq!(
        schema["allOf"][0]["then"]["required"],
        serde_json::json!(["text"])
    );
    assert_eq!(
        schema["allOf"][4]["then"]["properties"]["children"]["maxItems"],
        1
    );
}

#[test]
fn validation_reports_the_exact_checked_prefix_on_success_and_limit_failure() {
    let expression = container(EquationKind::Sequence, vec![atom("x"), atom("yz")]);
    let complete = EquationValidationLimits {
        max_nodes: 3,
        max_depth: 2,
        max_text_bytes: 3,
    };
    for (limits, result, expected_work) in [
        (complete, Ok(()), (3, 3, 2)),
        (
            EquationValidationLimits {
                max_nodes: 2,
                ..complete
            },
            Err(EquationValidationError::NodeLimit),
            (2, 1, 2),
        ),
        (
            EquationValidationLimits {
                max_depth: 1,
                ..complete
            },
            Err(EquationValidationError::DepthLimit),
            (2, 0, 2),
        ),
        (
            EquationValidationLimits {
                max_text_bytes: 2,
                ..complete
            },
            Err(EquationValidationError::TextByteLimit),
            (3, 3, 2),
        ),
        (
            EquationValidationLimits {
                max_nodes: 0,
                ..complete
            },
            Err(EquationValidationError::NodeLimit),
            (0, 0, 0),
        ),
        (
            EquationValidationLimits {
                max_depth: 0,
                ..complete
            },
            Err(EquationValidationError::DepthLimit),
            (1, 0, 1),
        ),
    ] {
        let report = expression.validate_with_report(limits);
        assert_eq!(report.result, result);
        assert_eq!(expression.validate(limits), report.result);
        assert_eq!(
            (report.visited_nodes, report.text_bytes, report.max_depth),
            expected_work
        );
    }
}

#[test]
fn invalid_equations_report_only_the_fields_reached_before_failure() {
    let mut invalid = container(EquationKind::Sequence, Vec::new());
    invalid.text = Some("unvisited bytes".into());
    let limits = EquationValidationLimits::default();
    let root = invalid.validate_with_report(limits);
    assert!(matches!(
        root.result,
        Err(EquationValidationError::InvalidNode(_))
    ));
    assert_eq!(
        (root.visited_nodes, root.text_bytes, root.max_depth),
        (1, 0, 1)
    );
    let expression = container(EquationKind::Sequence, vec![atom("x"), invalid]);
    let late = expression.validate_with_report(limits);
    assert!(matches!(
        late.result,
        Err(EquationValidationError::InvalidNode(_))
    ));
    assert_eq!(
        (late.visited_nodes, late.text_bytes, late.max_depth),
        (3, 1, 2)
    );
}

#[test]
fn validation_byte_reports_include_the_violating_utf8_field() {
    let mut expression = atom("x");
    expression.left = Some("(".into());
    expression.right = Some(")".into());
    expression.top = Some("→".into());
    expression.bottom = Some("_".into());
    let limits = EquationValidationLimits::default();
    let complete = expression.validate_with_report(limits);
    assert_eq!(complete.result, Ok(()));
    assert_eq!(
        (
            complete.visited_nodes,
            complete.text_bytes,
            complete.max_depth
        ),
        (1, 7, 1)
    );
    let partial = expression.validate_with_report(EquationValidationLimits {
        max_text_bytes: 4,
        ..limits
    });
    assert_eq!(partial.result, Err(EquationValidationError::TextByteLimit));
    assert_eq!(
        (partial.visited_nodes, partial.text_bytes, partial.max_depth),
        (1, 6, 1)
    );
    for empty in [atom(""), container(EquationKind::Sequence, Vec::new())] {
        let report = empty.validate_with_report(EquationValidationLimits {
            max_text_bytes: 0,
            ..limits
        });
        assert_eq!(report.result, Ok(()));
        assert_eq!(
            (report.visited_nodes, report.text_bytes, report.max_depth),
            (1, 0, 1)
        );
    }
}
