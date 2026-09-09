//! Native replay, recursion and ownership-transfer depth budgets.

use super::*;

#[test]
fn infinite_while_loop_is_bounded_with_a_diagnostic() {
    let report = Parser::default()
        .parse_bytes(
            "loop.1",
            b".TH LOOP 1\n.SH BODY\n.while 1 \\{\\\nloop\n.\\}\n.SH AFTER\nretained\n",
        )
        .expect("return the finite prefix of a looping manual");
    let mut visible = Vec::new();
    collect_visible_text(&report.document.root, &mut visible);

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("infinite loop")),
        "loop budget must remain observable: {:?}",
        report.diagnostics
    );
    assert!(
        visible.contains(&"retained"),
        "parsing must continue after the bounded loop"
    );
    assert!(
        visible.iter().filter(|value| **value == "loop").count() <= 10_000,
        "the loop body must not exceed the documented budget"
    );
}

#[test]
fn aggregate_while_replays_are_bounded_across_statements() {
    let mut source =
        String::from(".TH AGGREGATE 1\n.SH BODY\n.de M\n.while 1 \\{\\\nreplayed\n.\\}\n..\n");
    for _ in 0..3 {
        source.push_str(".M\n");
    }
    source.push_str(".SH AFTER\nretained aggregate tail\n");

    let report = Parser::default()
        .parse_bytes("aggregate.1", source.as_bytes())
        .expect("return the finite prefix across multiple loops");
    let mut visible = Vec::new();
    collect_visible_text(&report.document.root, &mut visible);

    let replayed = visible.iter().filter(|value| **value == "replayed").count();
    assert!(
        replayed <= 10_003,
        "three loop statements must share one replay budget: {replayed}"
    );
    assert!(visible.contains(&"retained aggregate tail"));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("infinite loop")),
        "aggregate exhaustion must remain observable: {:?}",
        report.diagnostics
    );
}

#[test]
fn recursive_user_macro_retains_content_after_the_cycle() {
    let report = Parser::default()
        .parse_bytes(
            "recursive.7",
            b".TH RECUR 7\n.SH NAME\nrecur \\- x\n.de R\n.  R\n..\n.R\n.SH DESC\ntail marker ZZTAIL\n",
        )
        .expect("return the complete document around recursive macro input");
    let mut visible = Vec::new();
    collect_visible_text(&report.document.root, &mut visible);

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("infinite loop")),
        "recursion limit must remain observable: {:?}",
        report.diagnostics
    );
    let visible = visible.join(" ");
    assert!(visible.contains("recur"), "{visible}");
    assert!(visible.contains("tail marker ZZTAIL"), "{visible}");
}

#[test]
fn deeply_nested_callable_mdoc_macros_are_bounded_in_the_native_parser() {
    let mut source = String::from(
        ".Dd August 24, 2026\n.Dt DEEP-MDOC 1\n.Os\n.Sh NAME\n.Nm deep-mdoc\n.Nd bounded callable macros\n.Sh BODY\n.Op ",
    );
    for _ in 0..50_000 {
        source.push_str("Op ");
    }
    source.push_str("nested tail marker\n.Sh AFTER\nretained document tail\n");

    let report = Parser::default()
        .parse_bytes("deep-mdoc.1", source.as_bytes())
        .expect("return a finite document for deeply nested callable macros");
    let mut visible = Vec::new();
    collect_visible_text(&report.document.root, &mut visible);
    let visible = visible.join(" ");

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("infinite loop")),
        "macro depth exhaustion must remain observable: {:?}",
        report.diagnostics
    );
    assert!(visible.contains("nested tail marker"), "{visible}");
    assert!(visible.contains("retained document tail"), "{visible}");
}

#[test]
fn deeply_nested_input_is_bounded_instead_of_overflowing_the_stack() {
    // Exceed the copy cap while remaining below the separate native
    // construction budget (each RS contributes both a block and a body).
    let depth = 180;
    let mut source = String::from(".TH DEEP 1\n.SH BODY\n");
    for _ in 0..depth {
        source.push_str(".RS\n");
    }
    source.push_str("deep\n");

    let report = Parser::default()
        .parse_bytes("deep.1", source.as_bytes())
        .expect("deeply nested source parses");

    // The owned tree stays well under the input nesting, proving the copy
    // stopped descending at the cap.
    assert!(
        measured_depth(&report.document.root) <= 300,
        "tree depth must be bounded by the copy cap"
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == Some(DiagnosticCode::SyntaxTreeDepthLimit)),
        "node truncation must remain observable: {:?}",
        report.diagnostics
    );
}

#[test]
fn deeply_nested_equation_is_bounded_instead_of_overflowing_the_stack() {
    // Braces nest eqn boxes, a recursive walk the node-copy cap never
    // enters: copy_equation descends box->first without limit, so a
    // pathologically nested equation overflows the stack while flattening
    // it. Each `sqrt` level emits text, so an unbounded render would grow
    // the string with the input depth; a bounded one plateaus at the cap.
    let depth = 5_000;
    let mut equation = String::new();
    for _ in 0..depth {
        equation.push_str("sqrt { ");
    }
    equation.push('x');
    for _ in 0..depth {
        equation.push_str(" }");
    }
    let source = format!(".TH DEEP 1\n.SH BODY\n.EQ\n{equation}\n.EN\n");

    let report = Parser::default()
        .parse_bytes("deep-eqn.1", source.as_bytes())
        .expect("deeply nested equation parses");

    let node = find_kind(&report.document.root, NodeKind::Equation).expect("equation node");
    let rendered = node.equation.as_deref().expect("equation text");
    // The render stopped at the cap: the flattened text is far shorter than
    // the ~30k chars all 5000 `sqrt` levels would emit, proving it did not
    // recurse through every box (and so could not overflow the stack).
    assert!(
        rendered.len() < 2_000,
        "equation text must be bounded by the copy cap, got {} bytes",
        rendered.len()
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == Some(DiagnosticCode::EquationTreeDepthLimit)),
        "equation truncation must remain observable: {:?}",
        report.diagnostics
    );
}
