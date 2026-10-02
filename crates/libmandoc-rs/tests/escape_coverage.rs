//! Actual escape-guard facts are session-local and independent of diagnostic text.
//! Exact depth/closure sources were frozen in the pristine oracle before the
//! assertions; `roff_escape_impl`'s guard keeps its 256-level limit and consumes
//! only the rejected suffix. No CVS mandocerr value crosses this boundary.

use libmandoc_rs::{DiagnosticCode, Parser};

const HEADER: &str =
    ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
const TAIL: &str = ".Sh NEXT\n.No END\n";

fn source(depth: usize, closed: bool) -> String {
    let close = if closed {
        format!("{}Z", "'".repeat(depth))
    } else {
        String::new()
    };
    format!(
        "{HEADER}.No \"A{}Q{close}\"\n.No AFTER\n{TAIL}",
        "\\X'".repeat(depth)
    )
}

fn guarded(report: &[libmandoc_rs::Diagnostic]) -> usize {
    report
        .iter()
        .filter(|finding| finding.code() == Some(DiagnosticCode::EscapeDepthLimit))
        .count()
}

#[test]
fn native_escape_coverage_is_retained_after_parser_release_and_reset_between_calls() {
    let parser = Parser::default();
    for depth in [1, 2, 8, 64, 255, 256, 257, 300] {
        for closed in [false, true] {
            let mut report = parser
                .parse_bytes("escape.1", source(depth, closed).as_bytes())
                .unwrap();
            assert_eq!(
                guarded(&report.diagnostics),
                usize::from(depth > 256),
                "{depth}/{closed}"
            );
            for finding in &mut report.diagnostics {
                finding.message = "translated diagnostic detail".to_owned();
            }
            assert_eq!(guarded(&report.diagnostics), usize::from(depth > 256));
            let clean = parser
                .parse_bytes("clean.1", source(1, true).as_bytes())
                .unwrap();
            assert_eq!(
                guarded(&clean.diagnostics),
                0,
                "guard state leaked after {depth}"
            );
        }
    }
}

#[test]
fn independent_concurrent_sessions_do_not_inherit_another_calls_guard() {
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        for index in 0..4 {
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                for turn in 0..16 {
                    let depth = if (index + turn) % 2 == 0 { 257 } else { 256 };
                    let report = Parser::default()
                        .parse_bytes("parallel.1", source(depth, true).as_bytes())
                        .unwrap();
                    assert_eq!(guarded(&report.diagnostics), usize::from(depth > 256));
                }
            });
        }
    });
}

#[cfg(feature = "render")]
#[test]
fn renderer_only_and_parser_results_observe_the_same_private_omission_fact() {
    use libmandoc_rs::{RenderFormat, Renderer};
    for format in [RenderFormat::Ascii, RenderFormat::Utf8, RenderFormat::Html] {
        let renderer = Renderer::new(format);
        for depth in [256, 257, 300, 1] {
            let report = renderer
                .render_bytes("render.1", source(depth, true).as_bytes())
                .unwrap();
            assert_eq!(
                guarded(&report.diagnostics),
                usize::from(depth > 256),
                "{format:?}/{depth}"
            );
            assert!(report.output.contains("AFTER"));
        }
    }
}

#[cfg(feature = "render")]
#[test]
fn a_failed_render_releases_the_private_escape_receipt_before_the_next_call() {
    use libmandoc_rs::{RenderErrorKind, RenderFormat, Renderer};
    // Both exact words use the already frozen D01 pristine source. The shim's
    // output-limit cleanup must clear its active receipt even when no partial
    // RenderReport is returned (mant_mandoc_shim.c::parse_input).
    for format in [RenderFormat::Ascii, RenderFormat::Utf8, RenderFormat::Html] {
        let error = Renderer::new(format)
            .with_max_output_bytes(16)
            .render_bytes("failed.1", source(257, true).as_bytes())
            .unwrap_err();
        assert_eq!(error.kind, RenderErrorKind::OutputLimit);
        let clean = Renderer::new(format)
            .render_bytes("clean.1", source(1, true).as_bytes())
            .unwrap();
        assert_eq!(guarded(&clean.diagnostics), 0);
        let parsed = Parser::default()
            .parse_bytes("clean.1", source(1, true).as_bytes())
            .unwrap();
        assert_eq!(guarded(&parsed.diagnostics), 0);
    }
}
