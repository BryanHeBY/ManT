//! Run native stack-safety regressions in children so an abort is observable.

use libmandoc_rs::Parser;

#[test]
fn owned_equation_transfer_projection_and_drop_fit_a_small_stack() {
    // The exact sources below were run through the fixed CVS -Ttree oracle.
    // eqn.c::eqn_box_alloc retains two boxes per sqrt/braced operand; its
    // first/next forest order must survive the independent owned depth cap.
    // Keep a potential stack abort in a child, outside the test runner.
    const CHILD: &str = "MANT_OWNED_EQUATION_STACK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "owned_equation_transfer_projection_and_drop_fit_a_small_stack",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            for depth in [32, 120, 127, 128, 260] {
                let source = format!(
                    ".TH DEEP 1\n.SH BODY\n.EQ\n{}x{}\n.EN\n",
                    "sqrt { ".repeat(depth),
                    " }".repeat(depth),
                );
                let report = Parser::default()
                    .parse_bytes("deep.1", source.as_bytes())
                    .expect("bounded owned equation transfer");
                let mut nodes = vec![&report.document.root];
                let equation = loop {
                    let node = nodes.pop().expect("equation node");
                    if let Some(equation) = &node.equation {
                        break equation;
                    }
                    nodes.extend(&node.children);
                };
                let mut boxes = vec![(equation, 1)];
                let mut maximum_depth = 0;
                let mut atom_count = 0;
                while let Some((box_node, box_depth)) = boxes.pop() {
                    maximum_depth = maximum_depth.max(box_depth);
                    atom_count += usize::from(box_node.text.as_deref() == Some("x"));
                    boxes.extend(box_node.children.iter().map(|child| (child, box_depth + 1)));
                }
                let truncated = report.diagnostics.iter().any(|diagnostic| {
                    diagnostic.code() == Some(libmandoc_rs::DiagnosticCode::EquationTreeDepthLimit)
                });
                assert_eq!(maximum_depth, (2 * depth + 2).min(256), "{depth}");
                assert_eq!(truncated, 2 * depth + 2 > 256, "{depth}");
                assert_eq!(atom_count, usize::from(!truncated), "{depth}");
                let readable = equation.readable_text();
                assert_eq!(readable.contains('x'), !truncated, "{depth}: {readable}");
                // Drop the entire owned prefix on the same restricted stack.
                drop(report);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_nesting_is_rejected_without_aborting_or_poisoning_sessions() {
    if std::env::var_os("MANT_NATIVE_DEPTH_CHILD").is_some() {
        run_native_depth_child();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "native_nesting_is_rejected_without_aborting_or_poisoning_sessions",
            "--nocapture",
        ])
        .env("MANT_NATIVE_DEPTH_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_native_depth_child() {
    let man = format!(
        ".TH DEEP 1\n.SH BODY\n{}x\n{}",
        ".RS 0\n".repeat(40_000),
        ".RE\n".repeat(40_000)
    );
    let mdoc = format!(
        ".Dd September 7, 2026\n.Dt DEEP 1\n.Os\n.Sh NAME\n.Nm deep\n.Nd test\n.Sh BODY\n{}x\n{}",
        ".Bd -ragged\n".repeat(10_000),
        ".Ed\n".repeat(10_000)
    );
    for source in [&man, &mdoc] {
        let error = Parser::default()
            .parse_bytes("deep.1", source.as_bytes())
            .unwrap_err();
        assert!(error.to_string().contains("nesting limit"), "{error}");
    }
    for (name, depth, expect_limit) in [
        ("escape-at-limit.1", 255, false),
        ("escape-at-exact-limit.1", 256, false),
        ("escape-over-limit.1", 257, true),
        ("closed-escape.1", 50_000, true),
    ] {
        let source = format!(
            ".TH ESCAPE 1\n.SH BODY\n{}X{}\n",
            "\\o'".repeat(depth),
            "'".repeat(depth)
        );
        let report = Parser::default()
            .parse_bytes(name, source.as_bytes())
            .expect("deep escape input must remain a finite parse");
        let limit_diagnostics = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("infinite loop"))
            .count();
        assert_eq!(
            limit_diagnostics,
            usize::from(expect_limit),
            "escape depth diagnostics must be singular and exact: {:?}",
            report.diagnostics
        );
    }
    let source = format!(".TH ESCAPE 1\n.SH BODY\n{}X\n", "\\o'".repeat(100_000));
    let report = Parser::default()
        .parse_bytes("open-escape.1", source.as_bytes())
        .expect("deep escape input must remain a finite parse");
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("infinite loop"))
            .count(),
        1,
        "escape depth exhaustion must remain observable: {:?}",
        report.diagnostics
    );
    #[cfg(feature = "render")]
    for format in [
        libmandoc_rs::RenderFormat::Ascii,
        libmandoc_rs::RenderFormat::Utf8,
        libmandoc_rs::RenderFormat::Html,
    ] {
        let equation = format!(
            ".TH EQN 1\n.SH BODY\n.EQ\n{}x{}\n.EN\n",
            "sqrt { ".repeat(5_000),
            " }".repeat(5_000)
        );
        let copy_truncated = format!(
            ".TH COPY 1\n.SH BODY\n{}x\n{}",
            ".RS 0\n".repeat(180),
            ".RE\n".repeat(180)
        );
        for source in [&man, &mdoc, &equation, &copy_truncated] {
            let error = libmandoc_rs::Renderer::new(format)
                .with_max_output_bytes(4096)
                .render_bytes("deep.1", source.as_bytes())
                .unwrap_err();
            assert!(error.to_string().contains("nesting limit"), "{error}");
        }
        assert!(
            libmandoc_rs::Renderer::new(format)
                .render_bytes("ok.1", b".TH OK 1\n.SH NAME\nok\n")
                .is_ok()
        );
    }
    assert!(
        Parser::default()
            .parse_bytes("ok.1", b".TH OK 1\n.SH NAME\nok\n")
            .is_ok()
    );
}
