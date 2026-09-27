//! Native and IR equation text must agree on operand coverage and grouping.
use super::*;

fn equation_from_native(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::EquationBox> {
    node.equation
        .as_ref()
        .or_else(|| node.children.iter().find_map(equation_from_native))
}

fn equation_node_mut(node: &mut libmandoc_rs::Node) -> Option<&mut libmandoc_rs::Node> {
    if node.equation.is_some() {
        return Some(node);
    }
    node.children.iter_mut().find_map(equation_node_mut)
}

fn assert_projection(source_expression: &str, expected: &str) {
    let source = format!(".TH REVIEW 7\n.SH DESCRIPTION\n.EQ\n{source_expression}\n.EN\n");
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("review.7", source.as_bytes())
        .expect("native equation parse");
    let native_equation = equation_from_native(&native.document.root).expect("native equation");
    assert_eq!(
        native_equation.readable_text(),
        expected,
        "native: {source_expression}"
    );

    let document = parse_manual_bytes(std::path::Path::new("review.7"), source.as_bytes())
        .expect("IR equation parse");
    let Block::Equation {
        value,
        expression: Some(expression),
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("IR equation: {source_expression}");
    };
    assert_eq!(value, expected, "IR value: {source_expression}");
    assert_eq!(
        expression.readable_text(),
        expected,
        "IR tree: {source_expression}"
    );
}

#[test]
fn matrix_projection_preserves_non_column_operands() {
    // Each exact source was checked with the fixed -Thtml/-Tutf8/-Tlint oracle
    // before these assertions. CVS eqn_html.c::eqn_box traverses columns only
    // for a non-singleton EQN_LIST; eqn.c::eqn_box_alloc also permits arbitrary
    // operands under EQN_MATRIX. Retain all operands if the tree is not a full
    // List -> Pile -> List column/row structure.
    for (expression, expected) in [
        ("matrix LOSTOPERAND", "matrix(LOSTOPERAND)"),
        ("matrix { a + b }", "matrix(a + b)"),
        ("matrix { a over b }", "matrix(a / b)"),
        ("matrix { lcol { a } x }", "matrix(a x)"),
        (
            "matrix { lcol { a above b } rcol { c above d } }",
            "matrix(a, c; b, d)",
        ),
    ] {
        assert_projection(expression, expected);
    }
}

#[test]
fn positioned_operands_keep_grouping_through_transparent_lists() {
    // Each exact source was checked with the fixed -Thtml/-Tutf8/-Tlint oracle
    // before these assertions. CVS eqn.c::eqn_box_makebinary reparents a full
    // EQN_LIST as an operand; eqn_html.c::eqn_box then nests MFRAC/MSUP tags.
    // The readable projection must expose the same operand boundaries.
    for (expression, expected) in [
        ("{ a over b } over c", "(a / b) / c"),
        ("a over { b over c }", "a / (b / c)"),
        ("{ a + b } sup 2", "(a + b) ^ 2"),
        ("a sup { b + c }", "a ^ (b + c)"),
        ("a over b over c", "(a / b) / c"),
        ("a sup b sup c", "a ^ (b ^ c)"),
        ("a over { b + c }", "a / (b + c)"),
        ("left ( a + b right ) sup 2", "(a + b) ^ 2"),
        ("a sup left ( b + c right )", "a ^ (b + c)"),
        ("a sub b sup c", "a _ b ^ c"),
        ("sqrt { a + b } sup 2", "sqrt(a + b) ^ 2"),
    ] {
        assert_projection(expression, expected);
    }
}

#[test]
fn deep_equation_without_source_span_still_marks_semantic_summary() {
    // This exact 24-sqrt source was checked with the fixed -Thtml/-Tlint
    // oracle. CVS eqn.c::eqn_box_alloc retains each sqrt/list level; remove
    // only the owned source coordinate to exercise an FFI location omission.
    let expression = format!("{}x{}", "sqrt { ".repeat(24), " }".repeat(24));
    let source = format!(".TH DEEP 7\n.SH DESCRIPTION\n.EQ\n{expression}\n.EN\n");
    let mut report = libmandoc_rs::Parser::default()
        .parse_bytes("deep.7", source.as_bytes())
        .expect("native equation parse");
    let node = equation_node_mut(&mut report.document.root).expect("equation node");
    node.line = 0;
    node.column = 0;
    let document = lower_mandoc_document(std::path::Path::new("deep.7"), &report);
    assert!(document.diagnostics.iter().any(|finding| {
        finding.code.as_deref() == Some("manual.equation-structure-depth-summarized")
            && finding.impact == mant_ir::DiagnosticImpact::SemanticCoverage
            && finding.source.is_none()
    }));
    assert!(mant_ir::content_complete(&document.diagnostics));
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
}
