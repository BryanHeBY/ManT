//! Resolve row ownership before mathematical encoders inspect the document IR.
use super::*;
use mant_ir::{EquationExpression, EquationFont, EquationKind};

fn lowered_expression(source_expression: &str) -> EquationExpression {
    let source = format!(".TH PROBE 3\n.SH DESCRIPTION\n.EQ\n{source_expression}\n.EN\n");
    let document = parse_manual_bytes(std::path::Path::new("probe.3"), source.as_bytes())
        .expect("lower equation");
    let Block::Equation {
        value,
        expression: Some(expression),
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("structured equation: {source_expression}");
    };
    assert_eq!(value, &expression.readable_text(), "{source_expression}");
    expression.clone()
}

#[test]
fn recovered_piles_without_rows_are_transparent_sequences() {
    // Every exact input was run with the fixed CVS -Thtml reference first.
    // eqn_html.c::eqn_box emits an mrow, without an mtable, when a Pile has
    // no first row List. eqn.c's singleton font List does not establish a row.
    for (source, expected) in [("pile", ""), ("pile x", "x"), ("pile roman x", "x")] {
        let expression = lowered_expression(source);
        assert_eq!(expression.children[0].kind, EquationKind::Sequence);
        assert_eq!(expression.readable_text(), expected, "{source}");
    }
    let roman = lowered_expression("pile roman x");
    assert_eq!(roman.children[0].children[0].font, EquationFont::Roman);
}

#[test]
fn empty_and_singleton_authored_rows_remain_vertical_piles() {
    // Fixed CVS -Thtml distinguishes `pile`'s transparent empty mrow from
    // `pile {}`'s one empty mtr/mtd. A singleton row remains a real table row.
    // eqn.c also creates that row List for `above`, without an opening brace.
    for source in ["pile {}", "pile { a }", "pile above y"] {
        let expression = lowered_expression(source);
        let pile = &expression.children[0];
        assert_eq!(pile.kind, EquationKind::Pile, "{source}");
        assert_eq!(pile.children.len(), 1, "{source}");
        assert_eq!(pile.children[0].kind, EquationKind::Sequence, "{source}");
    }
    let empty = lowered_expression("pile {}");
    assert_eq!(empty.children[0].children[0].children, []);
}

#[test]
fn recovered_pile_prefixes_do_not_acquire_invented_rows() {
    // These exact inputs were checked with fixed CVS -Thtml first. eqn.c
    // accepts `above` after an unbraced initial atom/font wrapper. HTML then
    // emits that inline prefix followed by otherwise orphaned mtr elements.
    // Retain their owned rows in a valid Pile after the prefix, preserving
    // every operand without fabricating a row around the unbraced content.
    for source in ["pile x above y above z", "pile roman x above y above z"] {
        let expression = lowered_expression(source);
        let recovered = &expression.children[0];
        assert_eq!(recovered.kind, EquationKind::Sequence);
        assert_eq!(recovered.children.len(), 2);
        assert_eq!(recovered.children[0].readable_text(), "x");
        let rows = &recovered.children[1];
        assert_eq!(rows.kind, EquationKind::Pile);
        assert_eq!(rows.children.len(), 2);
        assert_eq!(rows.children[0].readable_text(), "y");
        assert_eq!(rows.children[1].readable_text(), "z");
    }
    let grouped = lowered_expression("pile x above { y z }");
    let row = &grouped.children[0].children[1].children[0];
    assert_eq!(row.kind, EquationKind::Sequence);
    assert_eq!(row.children[0].kind, EquationKind::Group);
    assert_eq!(row.children[0].children.len(), 2);

    let empty = lowered_expression("pile x above {}");
    let row = &empty.children[0].children[1].children[0];
    assert_eq!(row.kind, EquationKind::Sequence);
    assert_eq!(row.children[0].kind, EquationKind::Group);
    assert_eq!(row.children[0].children, []);
}

#[test]
fn transparent_recovered_piles_keep_authored_operand_boundaries() {
    // Fixed CVS -Thtml retains authored braces and each msup/msqrt operand.
    // eqn_term.c additionally prints parentheses around neighboring Piles.
    // Those inferred terminal delimiters do not create mathematical scope
    // when the recovered Pile itself has no rows.
    for (source, expected) in [
        ("a pile x", "a x"),
        ("pile x a", "x a"),
        ("f { pile x }", "f(x)"),
        ("pile x sup i", "x ^ i"),
        ("a sup pile x", "a ^ x"),
        ("sqrt pile x", "sqrt(x)"),
    ] {
        let expression = lowered_expression(source);
        assert_eq!(expression.readable_text(), expected, "{source}");
    }
    let authored = lowered_expression("f { pile x }");
    assert_eq!(authored.children[1].kind, EquationKind::Group);
}

#[test]
fn matrix_axes_keep_font_wrapped_and_empty_rows() {
    // Fixed CVS eqn_html.c::eqn_box bypasses ordinary Pile rendering while
    // traversing matrix columns. `lcol roman x` owns a real one-cell axis,
    // although the same children in an ordinary `pile roman x` form an mrow.
    // Both exact inputs were run with the fixed -Thtml reference first.
    let expression = lowered_expression("matrix { lcol roman x rcol { y } }");
    let matrix = expression.children[0].matrix_view().expect("matrix axes");
    assert_eq!(matrix.columns.len(), 2);
    let first = &matrix.columns[0];
    assert_eq!(first.kind, EquationKind::Pile);
    assert_eq!(first.children.len(), 1);
    assert_eq!(first.children[0].kind, EquationKind::Sequence);
    assert_eq!(first.children[0].font, EquationFont::Roman);
    assert_eq!(first.children[0].readable_text(), "x");
    assert_eq!(expression.readable_text(), "matrix(x, y)");

    let expression = lowered_expression("matrix { lcol {} rcol { y above z } }");
    let matrix = expression.children[0].matrix_view().expect("unequal axes");
    assert_eq!(matrix.columns[0].children.len(), 1);
    assert_eq!(matrix.columns[0].children[0].children, []);
    assert_eq!(matrix.columns[1].children.len(), 2);
    // The source-neutral matrix retains the second column's final row even
    // though the pinned HTML path stops at the shorter first column's height.
    assert_eq!(expression.readable_text(), "matrix(, y; , z)");
}
