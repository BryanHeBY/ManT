//! Source-neutral contracts: these expressions are not roff parser fixtures.

use mant_ir::{
    EquationAtomRole as Role, EquationExpression as Expr, EquationFont as Font,
    EquationKind as Kind, EquationOperandScope as Scope, EquationOperator as Operator,
};

use super::*;

fn atom(text: &str, role: Role) -> Expr {
    Expr {
        kind: Kind::Text,
        font: Font::None,
        operator: Operator::None,
        atom_role: role,
        operand_scope: Scope::Atomic,
        text: Some(text.to_owned()),
        left: None,
        right: None,
        top: None,
        bottom: None,
        children: Vec::new(),
    }
}

fn container(kind: Kind, children: Vec<Expr>) -> Expr {
    let mut expression = atom("", Role::Unknown);
    expression.kind = kind;
    expression.text = None;
    expression.children = children;
    expression
}

fn operation(operator: Operator, children: Vec<Expr>) -> Expr {
    let mut expression = container(Kind::Operation, children);
    expression.operator = operator;
    expression
}

fn render(expression: &Expr, format: MathFormat) -> MathArtifact {
    encode_equation(
        expression,
        MathOptions {
            format,
            ..Default::default()
        },
    )
    .unwrap()
}

fn best(expression: &Expr, format: MathFormat) -> MathArtifact {
    encode_equation(
        expression,
        MathOptions {
            format,
            policy: MathPolicy::BestEffort,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn nested_fraction_root_and_script_keep_their_operands() {
    let expression = operation(
        Operator::Fraction,
        vec![
            operation(
                Operator::Subscript,
                vec![atom("x", Role::Identifier), atom("i", Role::Identifier)],
            ),
            operation(Operator::SquareRoot, vec![atom("2", Role::Number)]),
        ],
    );
    let xml = render(&expression, MathFormat::Mathml);
    assert_eq!(xml.losses, []);
    assert!(
        xml.text
            .contains("<mfrac><msub><mi>x</mi><mi>i</mi></msub><msqrt><mn>2</mn></msqrt></mfrac>")
    );
    assert_eq!(
        render(&expression, MathFormat::Latex).text,
        "\\frac{{{x}}_{{i}}}{\\sqrt{{2}}}"
    );
}

#[test]
fn limits_and_scripts_are_not_interchangeable() {
    let operands = vec![
        atom("∑", Role::Operator),
        atom("0", Role::Number),
        atom("n", Role::Identifier),
    ];
    let limits = operation(Operator::LowerUpperLimits, operands.clone());
    let scripts = operation(Operator::SubscriptSuperscript, operands);
    assert!(
        render(&limits, MathFormat::Mathml)
            .text
            .contains("<munderover accentunder=\"false\" accent=\"false\">")
    );
    assert!(
        render(&scripts, MathFormat::Mathml)
            .text
            .contains("<msubsup>")
    );
    assert!(
        render(&limits, MathFormat::Latex)
            .text
            .contains("\\underset")
    );
    assert!(
        !render(&scripts, MathFormat::Latex)
            .text
            .contains("\\underset")
    );
}

#[test]
fn invisible_groups_and_visible_one_sided_fences_remain_distinct() {
    let group = container(
        Kind::Group,
        vec![atom("a", Role::Identifier), atom("b", Role::Identifier)],
    );
    let xml = render(&group, MathFormat::Mathml).text;
    assert!(xml.contains("<mrow><mi>a</mi><mi>b</mi></mrow>"));
    assert!(!xml.contains("<mo"));
    let mut fenced = group;
    fenced.right = Some("]".to_owned());
    assert!(
        render(&fenced, MathFormat::Mathml)
            .text
            .contains("<mo fence=\"true\" stretchy=\"true\">]</mo>")
    );
    let latex = render(&fenced, MathFormat::Latex).text;
    assert!(latex.starts_with("\\left. "));
    assert!(latex.ends_with("\\right] "));
}

#[test]
fn quoted_literals_are_single_math_atoms_and_source_cannot_inject_markup() {
    let mut expression = atom("\\input{evil}$%_<script>&", Role::Literal);
    expression.font = Font::Italic;
    let xml = render(&expression, MathFormat::Mathml).text;
    assert!(
        xml.contains("<mi mathvariant=\"normal\" style=\"font-style:italic;font-weight:normal\">")
    );
    assert!(!xml.contains("<mtext>"));
    assert!(xml.contains("&lt;script&gt;&amp;"));
    assert!(!xml.contains("<script>"));
    let latex = render(&expression, MathFormat::Latex).text;
    assert!(latex.contains("\\textit{"));
    assert!(latex.contains("\\textbackslash{}input\\{evil\\}"));
    assert!(latex.contains("\\textdollar{}"));
    assert!(!latex.contains('$'));
    assert!(!latex.contains("\\input{"));
}

#[test]
fn function_role_and_inherited_font_are_retained() {
    let function = atom("sin", Role::Function);
    let function_xml = render(&function, MathFormat::Mathml).text;
    assert!(function_xml.contains("mathvariant=\"normal\"") && function_xml.contains("sin</mi>"));
    assert!(
        render(&function, MathFormat::Latex)
            .text
            .contains("\\mathop{")
    );
    let mut styled = container(Kind::Sequence, vec![atom("x", Role::Identifier)]);
    styled.font = Font::Bold;
    assert!(
        render(&styled, MathFormat::Mathml)
            .text
            .contains("font-weight:bold")
    );
    assert!(
        render(&styled, MathFormat::Latex)
            .text
            .contains("\\mathbf{x}")
    );
}

#[test]
fn known_accents_are_structural_and_custom_accents_report_latex_loss() {
    let mut expression = atom("x", Role::Identifier);
    expression.top = Some("→".to_owned());
    assert!(
        render(&expression, MathFormat::Mathml)
            .text
            .contains("<mover accent=\"true\">")
    );
    assert_eq!(
        render(&expression, MathFormat::Latex).text,
        "\\overrightarrow{{x}}"
    );
    expression.top = Some("custom".to_owned());
    assert!(matches!(
        encode_equation(
            &expression,
            MathOptions {
                format: MathFormat::Latex,
                ..Default::default()
            }
        ),
        Err(MathError::Loss(MathLoss {
            kind: MathLossKind::UnsupportedDecoration,
            ..
        }))
    ));
    let fallback = best(&expression, MathFormat::Latex);
    assert_eq!(fallback.losses[0].kind, MathLossKind::UnsupportedDecoration);
    assert!(fallback.text.contains("custom"));
}

fn matrix(columns: Vec<Vec<Expr>>) -> Expr {
    container(
        Kind::Matrix,
        vec![container(
            Kind::Group,
            columns
                .into_iter()
                .map(|rows| {
                    container(
                        Kind::Pile,
                        rows.into_iter()
                            .map(|cell| container(Kind::Sequence, vec![cell]))
                            .collect(),
                    )
                })
                .collect(),
        )],
    )
}

#[test]
fn matrix_is_emitted_in_row_order_and_missing_cells_never_drop_tail_content() {
    let expression = matrix(vec![
        vec![atom("a", Role::Identifier), atom("b", Role::Identifier)],
        vec![atom("c", Role::Identifier), atom("d", Role::Identifier)],
    ]);
    let xml = render(&expression, MathFormat::Mathml).text;
    assert!(xml.contains(
        "<mtr><mtd><mrow><mi>a</mi></mrow></mtd><mtd><mrow><mi>c</mi></mrow></mtd></mtr>"
    ));
    let tex = render(&expression, MathFormat::Latex).text;
    assert!(tex.find("{a}").unwrap() < tex.find("{c}").unwrap());
    assert!(tex.find("{c}").unwrap() < tex.find("{b}").unwrap());
    let ragged = matrix(vec![
        vec![atom("a", Role::Identifier)],
        vec![atom("b", Role::Identifier), atom("TAIL", Role::Identifier)],
    ]);
    for format in [MathFormat::Mathml, MathFormat::Latex] {
        let artifact = best(&ragged, format);
        assert!(artifact.text.contains("TAIL"));
        assert!(
            artifact
                .losses
                .iter()
                .any(|loss| loss.kind == MathLossKind::IrregularMatrix)
        );
    }
}

#[test]
fn opaque_structure_has_explicit_loss_and_never_becomes_trusted_tex() {
    let mut expression = atom("\\input{private}", Role::Unknown);
    expression.kind = Kind::Opaque;
    expression.operand_scope = Scope::Compound;
    for format in [MathFormat::Mathml, MathFormat::Latex] {
        assert!(matches!(
            encode_equation(
                &expression,
                MathOptions {
                    format,
                    ..Default::default()
                }
            ),
            Err(MathError::Loss(MathLoss {
                kind: MathLossKind::OpaqueStructure,
                ..
            }))
        ));
        let artifact = best(&expression, format);
        assert_eq!(artifact.losses[0].kind, MathLossKind::OpaqueStructure);
        if format == MathFormat::Latex {
            assert!(!artifact.text.contains("\\input{"));
        }
    }
}

#[test]
fn losses_have_original_child_paths_and_repeated_unknown_glyphs_are_deduplicated() {
    let expression = container(
        Kind::Sequence,
        vec![
            atom("x", Role::Identifier),
            atom("未知未知", Role::Identifier),
        ],
    );
    let artifact = best(&expression, MathFormat::Latex);
    assert_eq!(
        artifact.losses,
        [MathLoss {
            path: vec![1],
            kind: MathLossKind::UnsupportedSymbol
        }]
    );
    assert!(artifact.text.contains('未') && artifact.text.contains('知'));
}

#[test]
fn malformed_operations_are_hard_errors_even_in_best_effort() {
    let expression = operation(Operator::Fraction, vec![atom("a", Role::Identifier)]);
    assert!(matches!(
        encode_equation(
            &expression,
            MathOptions {
                policy: MathPolicy::BestEffort,
                ..Default::default()
            }
        ),
        Err(MathError::InvalidExpression { .. })
    ));
}

#[test]
fn independent_input_and_output_budgets_never_return_partial_artifacts() {
    let mut deep = atom("x", Role::Identifier);
    for _ in 0..MAX_MATH_DEPTH + 2 {
        deep = container(Kind::Sequence, vec![deep]);
    }
    assert_eq!(
        encode_equation(&deep, MathOptions::default()),
        Err(limit(MathLimit::Depth))
    );
    let wide = container(
        Kind::Sequence,
        vec![atom("x", Role::Identifier); MAX_MATH_NODES],
    );
    assert_eq!(
        encode_equation(&wide, MathOptions::default()),
        Err(limit(MathLimit::Nodes))
    );
    let huge = atom(&"x".repeat(MAX_MATH_INPUT_BYTES + 1), Role::Literal);
    assert_eq!(
        encode_equation(&huge, MathOptions::default()),
        Err(limit(MathLimit::InputBytes))
    );
    let escaping = atom(&"&".repeat(MAX_MATH_INPUT_BYTES), Role::Literal);
    assert_eq!(
        encode_equation(&escaping, MathOptions::default()),
        Err(limit(MathLimit::OutputBytes))
    );
}

#[test]
fn rectangular_expansion_is_bounded_even_with_few_actual_cells() {
    let mut columns = vec![vec![atom("x", Role::Identifier)]; 128];
    columns[0] = vec![atom("x", Role::Identifier); 129];
    let expression = matrix(columns);
    assert_eq!(
        encode_equation(
            &expression,
            MathOptions {
                policy: MathPolicy::BestEffort,
                ..Default::default()
            }
        ),
        Err(limit(MathLimit::MatrixSlots))
    );
}

#[test]
fn matrix_container_presentation_is_never_silently_discarded() {
    for column_presentation in [false, true] {
        let mut expression = matrix(vec![vec![atom("x", Role::Identifier)]]);
        let node = if column_presentation {
            &mut expression.children[0].children[0]
        } else {
            &mut expression.children[0]
        };
        node.left = Some("(".to_owned());
        node.right = Some(")".to_owned());
        for format in [MathFormat::Mathml, MathFormat::Latex] {
            assert!(matches!(
                encode_equation(
                    &expression,
                    MathOptions {
                        format,
                        ..Default::default()
                    }
                ),
                Err(MathError::Loss(MathLoss {
                    kind: MathLossKind::UnsupportedMatrix,
                    ..
                }))
            ));
            let artifact = best(&expression, format);
            assert!(
                artifact
                    .losses
                    .iter()
                    .any(|loss| loss.kind == MathLossKind::UnsupportedMatrix)
            );
            assert!(artifact.text.contains('(') && artifact.text.contains(')'));
        }
    }
}

#[test]
fn matrix_views_are_not_scanned_before_the_input_node_budget() {
    let expression = matrix(vec![vec![atom("x", Role::Identifier); MAX_MATH_NODES]]);
    assert_eq!(
        encode_equation(&expression, MathOptions::default()),
        Err(limit(MathLimit::Nodes))
    );
}

#[test]
fn display_is_explicit_and_latex_never_supplies_markdown_delimiters() {
    let expression = atom("x", Role::Identifier);
    let xml = encode_equation(
        &expression,
        MathOptions {
            display: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(xml.text.contains("display=\"block\""));
    let tex = encode_equation(
        &expression,
        MathOptions {
            format: MathFormat::Latex,
            display: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(tex.text, "\\displaystyle {x}");
    assert!(!tex.text.contains('$'));
}

#[test]
fn explicit_roman_greek_is_not_falsely_promised_by_a_math_alphabet() {
    let mut expression = atom("α", Role::Identifier);
    expression.font = Font::Roman;
    assert!(matches!(
        encode_equation(
            &expression,
            MathOptions {
                format: MathFormat::Latex,
                ..Default::default()
            }
        ),
        Err(MathError::Loss(MathLoss {
            kind: MathLossKind::UnsupportedFont,
            ..
        }))
    ));
    let artifact = best(&expression, MathFormat::Latex);
    assert!(
        artifact
            .losses
            .iter()
            .any(|loss| loss.kind == MathLossKind::UnsupportedFont)
    );
    let xml = render(&expression, MathFormat::Mathml).text;
    assert!(xml.contains("mathvariant=\"normal\"") && xml.contains("font-style:normal"));
}

#[test]
fn matrix_reading_fallback_retains_its_effective_font() {
    let mut expression = container(Kind::Matrix, vec![atom("x", Role::Identifier)]);
    expression.font = Font::Italic;
    let artifact = best(&expression, MathFormat::Mathml);
    assert!(
        artifact
            .losses
            .iter()
            .any(|loss| loss.kind == MathLossKind::UnsupportedMatrix)
    );
    assert!(
        artifact
            .text
            .contains("<mtext style=\"font-style:italic;font-weight:normal\">")
    );
}

#[test]
fn mathml_rejects_illegal_xml_scalars_and_reports_surface_whitespace() {
    for value in ["\u{0}", "\u{fffe}", "\u{ffff}"] {
        let expression = atom(value, Role::Literal);
        assert!(matches!(
            encode_equation(&expression, MathOptions::default()),
            Err(MathError::Loss(MathLoss {
                kind: MathLossKind::UnsupportedCharacter,
                ..
            }))
        ));
        let artifact = best(&expression, MathFormat::Mathml);
        assert!(!artifact.text.contains(value));
        assert!(artifact.text.contains("[U+"));
    }
    for decoration in [false, true] {
        let mut expression = atom("x", Role::Identifier);
        if decoration {
            expression.top = Some("\t".to_owned());
        } else {
            expression.left = Some(" ".to_owned());
        }
        assert!(matches!(
            encode_equation(&expression, MathOptions::default()),
            Err(MathError::Loss(MathLoss {
                kind: MathLossKind::LiteralWhitespace,
                ..
            }))
        ));
    }
}

#[test]
fn depth_limit_counts_the_root_in_the_same_way_as_ir_validation() {
    let mut expression = atom("x", Role::Identifier);
    for _ in 1..MAX_MATH_DEPTH {
        expression = container(Kind::Sequence, vec![expression]);
    }
    assert!(encode_equation(&expression, MathOptions::default()).is_ok());
    let deeper = container(Kind::Sequence, vec![expression]);
    assert_eq!(
        encode_equation(&deeper, MathOptions::default()),
        Err(limit(MathLimit::Depth))
    );
}

#[test]
fn no_atom_role_can_inject_html_commands_or_markdown_dollars_into_latex() {
    for role in [
        Role::Unknown,
        Role::Identifier,
        Role::Number,
        Role::Operator,
        Role::Function,
        Role::Literal,
    ] {
        let expression = atom("<script>\\input{bad}$a+b</script>", role);
        let artifact = best(&expression, MathFormat::Latex);
        assert!(
            !artifact.text.contains(['<', '>', '$']),
            "{role:?}: {}",
            artifact.text
        );
        assert!(!artifact.text.contains("\\input{"));
        assert!(artifact.text.contains("\\textless{}"));
        assert!(artifact.text.contains("\\textgreater{}"));
        assert!(artifact.text.contains("\\textbackslash{}input\\{bad\\}"));
    }
    for value in ["<", ">"] {
        let artifact = render(&atom(value, Role::Operator), MathFormat::Latex);
        assert!(!artifact.text.contains(['<', '>']));
        assert!(artifact.text.contains("\\mathrel{"));
    }
}

#[test]
fn custom_identifiers_numbers_and_operator_labels_are_not_reparsed_as_operations() {
    for (value, role) in [("a+b", Role::Identifier), ("1e-5", Role::Number)] {
        let artifact = render(&atom(value, role), MathFormat::Latex);
        assert_eq!(artifact.text, format!("\\mathord{{\\textrm{{{value}}}}}"));
        assert_eq!(artifact.losses, []);
    }
    for value in ["mod", "++"] {
        let expression = atom(value, Role::Operator);
        assert!(matches!(
            encode_equation(
                &expression,
                MathOptions {
                    format: MathFormat::Latex,
                    ..Default::default()
                }
            ),
            Err(MathError::Loss(MathLoss {
                kind: MathLossKind::UnsupportedAtom,
                ..
            }))
        ));
        let artifact = best(&expression, MathFormat::Latex);
        assert_eq!(artifact.text, format!("\\mathord{{\\textrm{{{value}}}}}"));
        assert_eq!(
            artifact.losses,
            [MathLoss {
                path: vec![],
                kind: MathLossKind::UnsupportedAtom
            }]
        );
    }
}

#[test]
fn ascii_angle_fences_are_not_silently_rewritten_to_different_glyphs() {
    let mut expression = operation(
        Operator::Fraction,
        vec![atom("a", Role::Identifier), atom("b", Role::Identifier)],
    );
    expression.left = Some("<".to_owned());
    expression.right = Some(">".to_owned());
    assert!(matches!(
        encode_equation(
            &expression,
            MathOptions {
                format: MathFormat::Latex,
                ..Default::default()
            }
        ),
        Err(MathError::Loss(MathLoss {
            kind: MathLossKind::UnsupportedFence,
            ..
        }))
    ));
    let artifact = best(&expression, MathFormat::Latex);
    assert!(artifact.text.contains("\\mathopen{") && artifact.text.contains("\\mathclose{"));
    assert!(artifact.text.contains("\\frac{"));
    assert!(artifact.text.contains("\\textless{}") && artifact.text.contains("\\textgreater{}"));
    assert!(!artifact.text.contains("\\langle") && !artifact.text.contains("\\rangle"));
    expression.left = Some("⟨".to_owned());
    expression.right = Some("⟩".to_owned());
    let artifact = render(&expression, MathFormat::Latex);
    assert!(artifact.text.contains("\\left\\langle") && artifact.text.contains("\\right\\rangle"));
}

#[test]
fn invalid_matrix_fallback_does_not_duplicate_its_own_visible_surfaces() {
    let mut expression = container(Kind::Matrix, vec![atom("x", Role::Identifier)]);
    expression.left = Some("[".to_owned());
    expression.right = Some("]".to_owned());
    expression.top = Some("@".to_owned());
    expression.bottom = Some("_".to_owned());
    let xml = best(&expression, MathFormat::Mathml);
    assert_eq!(xml.text.matches('[').count(), 1);
    assert_eq!(xml.text.matches(']').count(), 1);
    assert_eq!(xml.text.matches('@').count(), 1);
    assert_eq!(xml.text.matches('_').count(), 1);
    assert!(!xml.text.contains("<mover") && !xml.text.contains("<mo "));
    let tex = best(&expression, MathFormat::Latex);
    assert_eq!(tex.text.matches('[').count(), 1);
    assert_eq!(tex.text.matches(']').count(), 1);
    assert_eq!(tex.text.matches('@').count(), 1);
    assert_eq!(tex.text.matches("\\_").count(), 1);
    assert!(!tex.text.contains("\\left") && !tex.text.contains("\\over"));
}
