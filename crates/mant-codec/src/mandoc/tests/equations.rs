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

fn assert_decoration_projection(source_expression: &str, expected: &str) {
    let source = format!(".TH REVIEW 7\n.SH DESCRIPTION\n.EQ\n{source_expression}\n.EN\n");
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("review.7", source.as_bytes())
        .expect("native equation parse");
    let native_equation = equation_from_native(&native.document.root).expect("native equation");
    assert_eq!(
        super::super::visible_text(&native_equation.readable_text()),
        expected,
        "decoded native: {source_expression}"
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
        ("matrix { lcol { a } x }", "matrix((a) x)"),
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
fn sequence_groups_and_decorated_operands_keep_their_scope() {
    // Every exact source was checked first with the fixed -Tutf8/-Thtml/-Tlint
    // oracle. CVS eqn_term.c::eqn_box groups explicit EQN_LIST siblings and
    // suppresses spacing after a name/list. CVS eqn.c::eqn_box_makebinary
    // wraps diacritics in unary EQN_LIST boxes; eqn_term.c groups a decorated
    // box again when it becomes a script operand.
    for (expression, expected) in [
        ("f { a + b }", "f(a + b)"),
        ("{ a + b } { c + d }", "(a + b)(c + d)"),
        ("f { a }", "f(a)"),
        ("{ a } { b }", "(a)(b)"),
        ("f left ( a + b right )", "f(a + b)"),
        ("a sup { b }", "a ^ (b)"),
        ("a over { b }", "a / (b)"),
        ("a + { b + c }", "a + (b + c)"),
        ("{ a + b } + c", "(a + b) + c"),
        ("x sup y { a + b }", "x ^ y (a + b)"),
        ("f { pile { a above b } }", "f (a b)"),
        ("f { matrix { ccol { a above b } } }", "f (matrix(a; b))"),
    ] {
        assert_projection(expression, expected);
    }
    for (expression, expected) in [
        ("{ x + y } hat sup 2", "((x + y)^) ^ 2"),
        ("x hat sup 2", "(x^) ^ 2"),
        ("x under sub i", "(x_) _ i"),
        ("{ a + b } hat", "(a + b)^"),
        ("{ a + b } under sub i", "((a + b)_) _ i"),
        ("left ( a + b right ) hat sup 2", "((a + b)^) ^ 2"),
    ] {
        assert_decoration_projection(expression, expected);
    }
}

#[test]
fn pile_rows_use_row_scope_without_erasing_explicit_groups() {
    // These exact sources were checked with fixed CVS -Tutf8/-Thtml/-Tlint
    // before assertions. eqn_term.c::eqn_box skips the row List for a
    // singleton row, groups multi-operand rows, and groups a Pile with
    // sequence neighbors. eqn_html.c::eqn_box retains row boundaries.
    for (expression, expected) in [
        ("pile { a above b }", "a b"),
        ("pile { a + b above c + d }", "(a + b)(c + d)"),
        ("pile { { a + b } above { c + d } }", "(a + b) (c + d)"),
        ("pile { pile { a above b } above c }", "a b c"),
        ("pile { a above pile { b above c } }", "a b c"),
        ("a pile { b above c }", "a (b c)"),
        ("pile { a above b } c", "(a b) c"),
        ("pile { a above c + d }", "a(c + d)"),
        ("pile { a + b above c }", "(a + b) c"),
    ] {
        assert_projection(expression, expected);
    }
    // The same row/pile shapes are valid matrix operands; CVS HTML places
    // them in cells and the source-neutral projection labels matrix rows.
    for (expression, expected) in [
        ("matrix { ccol { a above b } }", "matrix(a; b)"),
        (
            "matrix { ccol { a + b above c + d } }",
            "matrix(a + b; c + d)",
        ),
        (
            "matrix { ccol { pile { a above b } above c } }",
            "matrix(a b; c)",
        ),
    ] {
        assert_projection(expression, expected);
    }
}

#[test]
fn invisible_and_one_sided_fences_keep_operand_boundaries() {
    // Each exact source was checked with fixed CVS -Tutf8/-Thtml/-Tlint
    // before these assertions. CVS eqn.c stores an empty left/right fence,
    // and eqn_html.c::eqn_box still emits mfenced/mrow around the operands.
    // The readable projection supplies its missing visible delimiter.
    for (expression, expected) in [
        ("a sup left \"\" b + c right \"\"", "a ^ (b + c)"),
        ("a over left \"\" b + c right \"\"", "a / (b + c)"),
        ("left \"\" a + b right \"\" sup 2", "(a + b) ^ 2"),
        ("a sup left \"\" b + c right )", "a ^ (b + c)"),
        ("a sup left ( b + c right \"\"", "a ^ (b + c)"),
        ("a sup left \"\" b + c right ]", "a ^ [b + c]"),
        ("a sup left [ b + c right \"\"", "a ^ [b + c]"),
        ("a sup left \"\" b + c right |", "a ^ |b + c|"),
        ("a sup left | b + c right \"\"", "a ^ |b + c|"),
        ("left \"\" a + b right \"\"", "(a + b)"),
        ("f left \"\" a + b right \"\"", "f(a + b)"),
        (
            "left \"\" a + b right \"\" left \"\" c + d right \"\"",
            "(a + b)(c + d)",
        ),
        ("left \"\" a + b right \"\" + c", "(a + b) + c"),
        ("f left [ a + b right \"\"", "f[a + b]"),
        (
            "left \"\" a + b right ) left \"\" c + d right ]",
            "(a + b)[c + d]",
        ),
        ("a sup left ( b + c right .", "a ^ (b + c)."),
        ("a sup left [ b + c right )", "a ^ ([b + c])"),
        ("a sup left . b + c right )", "a ^ (.b + c)"),
    ] {
        assert_projection(expression, expected);
    }
}

#[test]
fn deep_equation_without_source_span_still_marks_semantic_summary() {
    // This exact 70-sqrt source was checked with the fixed -Thtml/-Tlint
    // oracle. CVS eqn.c::eqn_box_alloc retains each sqrt/list level; remove
    // only the owned source coordinate to exercise an FFI location omission.
    let expression = format!("{}x{}", "sqrt { ".repeat(70), " }".repeat(70));
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
