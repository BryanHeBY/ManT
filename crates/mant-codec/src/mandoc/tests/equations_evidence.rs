//! Native equation evidence is normalized once, without replaying source.

use super::{Block, parse_manual_bytes};
use mant_ir::{DiagnosticImpact, EquationAtomRole, EquationExpression, EquationKind};

fn expressions(document: &mant_ir::Document) -> Vec<&EquationExpression> {
    document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter_map(|block| match block {
            Block::Equation {
                expression: Some(expression),
                ..
            } => Some(expression),
            _ => None,
        })
        .collect()
}

fn atoms<'a>(expression: &'a EquationExpression, output: &mut Vec<&'a EquationExpression>) {
    if expression.kind == EquationKind::Text {
        output.push(expression);
    }
    for child in &expression.children {
        atoms(child, output);
    }
}

fn native_equation(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::EquationBox> {
    node.equation
        .as_ref()
        .or_else(|| node.children.iter().find_map(native_equation))
}

fn native_equation_mut(node: &mut libmandoc_rs::Node) -> Option<&mut libmandoc_rs::EquationBox> {
    if node.equation.is_some() {
        node.equation.as_mut()
    } else {
        node.children.iter_mut().find_map(native_equation_mut)
    }
}

#[test]
fn atom_roles_preserve_complete_function_and_literal_evidence() {
    use EquationAtomRole::{Function, Identifier, Literal, Number, Operator};

    // Exact source was checked with fixed CVS -Ttree and -Thtml before the
    // assertion. eqn_next expands definitions before FUNC/QUOTED admission;
    // eqn_parse's quoted "sin" remains an italic mathematical atom.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\nsin \"sin\" roman \"sin\" define f /sin/ f define q /\"sin\"/ q x2 12 + alpha\n.EN\n";
    let document = parse_manual_bytes(std::path::Path::new("roles.7"), source).unwrap();
    let mut found = Vec::new();
    atoms(expressions(&document)[0], &mut found);
    assert_eq!(
        found
            .iter()
            .map(|atom| (atom.text.as_deref().unwrap(), atom.atom_role))
            .collect::<Vec<_>>(),
        [
            ("sin", Function),
            ("sin", Literal),
            ("sin", Literal),
            ("sin", Function),
            ("sin", Literal),
            ("x", Identifier),
            ("2", Number),
            ("12", Number),
            ("+", Operator),
            ("α", Identifier),
        ]
    );
    assert_eq!(mant_ir::validate_document(&document), []);
    let encoded = serde_json::to_string(&document).unwrap();
    let decoded: mant_ir::Document = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, document);
}

#[test]
fn allocation_default_is_not_size_evidence_but_explicit_zero_is() {
    // Exact fixed CVS -Ttree distinguishes the default x atom (size sentinel),
    // size0's wrapper0, and gsize0's atom0. eqn_box_new alone calloc's root0;
    // neither eqn_html.c nor eqn_term.c reads the stored size values.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\nx\n.EN\n.EQ\nsize 0 y\n.EN\n.EQ\ngsize 0 z\n.EN\n";
    let document = parse_manual_bytes(std::path::Path::new("sizes.7"), source).unwrap();
    let findings = document
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("manual.equation-size-unsupported"))
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].source.unwrap().line, 6);
    assert_eq!(findings[1].source.unwrap().line, 9);
    assert!(
        findings
            .iter()
            .all(|finding| finding.impact == DiagnosticImpact::SemanticCoverage)
    );
    assert!(mant_ir::content_complete(&document.diagnostics));
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
    let encoded = serde_json::to_value(&document).unwrap();
    assert!(!encoded.to_string().contains("\"size\""));
}

#[test]
fn incomplete_native_operators_become_opaque_without_losing_text() {
    // Exact source was run with pinned -Ttree and -Tutf8 first. eqn.c permits
    // over/sub with only their base and sqrt without an operand. A valid
    // document must retain that owned readable payload, not malformed arity.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\na over\n.EN\n.EQ\nsqrt\n.EN\n.EQ\na sub\n.EN\n";
    let document = parse_manual_bytes(std::path::Path::new("incomplete.7"), source).unwrap();
    let formulas = expressions(&document);
    assert_eq!(formulas.len(), 3);
    for formula in formulas {
        assert_eq!(formula.children[0].kind, EquationKind::Opaque);
        assert!(formula.children[0].text.is_some());
        assert_eq!(formula.children[0].children, []);
    }
    assert_eq!(mant_ir::validate_document(&document), []);
    assert_eq!(
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_deref() == Some("manual.equation-structure-opaque")
            })
            .count(),
        3
    );
    assert!(mant_ir::content_complete(&document.diagnostics));
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
}

#[test]
fn extra_owned_operands_remain_in_the_opaque_payload() {
    // Exact a-over-b source was checked with pinned -Ttree first. This test
    // then edits the public owned value, not a native execution expectation.
    // Every supplied operand must survive producer recovery of invalid arity.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\na over b\n.EN\n";
    let report = libmandoc_rs::Parser::default()
        .parse_bytes("extra.7", source)
        .unwrap();
    let mut operation = native_equation(&report.document.root).unwrap().children[0].clone();
    operation.children.push(operation.children[0].clone());
    let expected = operation.readable_text();
    let recovered = crate::mandoc::equations::expression_from_ast(&operation);
    assert_eq!(recovered.kind, EquationKind::Opaque);
    assert_eq!(recovered.text.as_deref(), Some(expected.as_str()));
    assert!(recovered.validate_shallow().is_ok());
}

#[test]
fn coverage_details_are_bounded_without_hiding_semantic_loss() {
    // The exact generated 260-formula source was run with the pinned -Ttree
    // reference before this assertion; each size0 wrapper is retained there.
    let source = format!(
        ".TH EQN 7\n.SH BODY\n{}",
        ".EQ\nsize 0 x\n.EN\n".repeat(260)
    );
    let document = parse_manual_bytes(std::path::Path::new("many.7"), source.as_bytes()).unwrap();
    assert_eq!(
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_deref() == Some("manual.equation-size-unsupported")
            })
            .count(),
        256
    );
    assert_eq!(
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_deref() == Some("manual.equation-diagnostics-summarized")
            })
            .count(),
        1
    );
    assert!(mant_ir::content_complete(&document.diagnostics));
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
}

#[test]
fn coverage_scan_respects_hidden_owned_subtrees_and_nondefault_root_evidence() {
    // Public owned-value mutations, not new source execution expectations.
    // The exact source was already checked with fixed CVS -Ttree above;
    // man_html.c/mdoc_html.c skip NODE_NOPRT before visiting child formulas.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\na over b\n.EN\n";
    let mut report = libmandoc_rs::Parser::default()
        .parse_bytes("scope.7", source)
        .unwrap();
    assert_eq!(
        crate::mandoc::equations::diagnostics(&report.document.root),
        []
    );
    native_equation_mut(&mut report.document.root).unwrap().size = 14;
    assert_eq!(
        crate::mandoc::equations::diagnostics(&report.document.root).len(),
        1
    );
    report.document.root.flags.no_print = true;
    assert_eq!(
        crate::mandoc::equations::diagnostics(&report.document.root),
        []
    );
}

#[test]
fn matrix_wrapper_fences_survive_the_native_to_ir_reading_boundary() {
    // Exact pinned tree/HTML/UTF-8 source: eqn.c retains wrapper fences,
    // although eqn_term.c:135-148/eqn_html.c:49-94 skip the wrapper. The
    // shared reading contract retains owned content rather than that loss.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\nmatrix left ( lcol { a above b } right )\n.EN\n";
    let document = parse_manual_bytes(std::path::Path::new("matrix.7"), source).unwrap();
    let expression = expressions(&document)[0];
    assert_eq!(expression.readable_text(), "matrix((a b))");
    assert!(expression.children[0].matrix_view().is_none());
    assert_eq!(mant_ir::validate_document(&document), []);
}

#[test]
fn synthetic_matrix_header_content_keeps_native_and_ir_reading_in_agreement() {
    // Exact baseline was checked with pinned tree/HTML/UTF-8 before these
    // assertions. Header mutations exercise public owned values, not new
    // source parsing behavior or guessed native renderer output.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\nmatrix { lcol { a above b } }\n.EN\n";
    let report = libmandoc_rs::Parser::default()
        .parse_bytes("matrix.7", source)
        .unwrap();
    let original = &native_equation(&report.document.root).unwrap().children[0];
    for column in [false, true] {
        for (field, glyph) in [(0, "["), (1, "]"), (2, "@"), (3, "_")] {
            let mut native = original.clone();
            let header = if column {
                &mut native.children[0].children[0]
            } else {
                &mut native.children[0]
            };
            match field {
                0 => header.left = Some(glyph.into()),
                1 => header.right = Some(glyph.into()),
                2 => header.top = Some(glyph.into()),
                _ => header.bottom = Some(glyph.into()),
            }
            let expression = crate::mandoc::equations::expression_from_ast(&native);
            let expected = native.readable_text();
            assert_eq!(expression.readable_text(), expected, "{column}/{field}");
            assert!(expression.readable_text().contains(glyph));
            assert!(expression.matrix_view().is_none());
            assert!(expression.validate_shallow().is_ok());
        }
    }
}

#[test]
fn native_accent_wrapper_keeps_pile_row_scope_through_lowering() {
    // Exact pinned tree/HTML/UTF-8 source: eqn.c:716-770 wraps the complete
    // Pile in a decorated unary List; eqn_term.c:135-148 skips singleton row
    // wrappers regardless of that outer accent. This is a real native source
    // control for the independently authored decorated-column cases above.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\npile { a above b } bar\n.EN\n";
    let report = libmandoc_rs::Parser::default()
        .parse_bytes("accent.7", source)
        .unwrap();
    let native = native_equation(&report.document.root).unwrap();
    let document = parse_manual_bytes(std::path::Path::new("accent.7"), source).unwrap();
    let expression = expressions(&document)[0];
    assert_eq!(expression.readable_text(), "(a b)‾");
    assert_eq!(
        expression.readable_text(),
        crate::mandoc::visible_text(&native.readable_text())
    );
    assert_eq!(mant_ir::validate_document(&document), []);
}
