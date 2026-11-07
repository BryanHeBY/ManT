#![cfg(feature = "render")]

use libmandoc_rs::{DiagnosticLevel, RenderErrorKind, RenderFormat, Renderer, SourceBundle};
use std::{path::Path, process::Command, thread};

const FIXTURES: &str = "tests/fixtures/markdown-renderer";
const MDOC: &[u8] = include_bytes!("fixtures/markdown-renderer/reference.mdoc");
const EXPECTED: &str = include_str!("fixtures/markdown-renderer/reference.expected");
const OMISSIONS: &[u8] = include_bytes!("fixtures/markdown-renderer/omissions.mdoc");
const EXPECTED_OMISSIONS: &str = include_str!("fixtures/markdown-renderer/omissions.expected");
const MAN: &[u8] = include_bytes!("fixtures/markdown-renderer/traditional.man");

#[test]
fn markdown_matches_pristine_cvs_for_bytes_files_and_resolved_bundles() {
    // Captured from the fixed CVS binary with -Tmarkdown. The fixture
    // exercises indentation, hard breaks, named and numeric entities,
    // percent-escaped URLs, ASCII literal glyphs and list numbering.
    let renderer = Renderer::new(RenderFormat::Markdown)
        .with_width(1)
        .with_html_fragment(true)
        .with_max_output_bytes(EXPECTED.len());
    let bytes = renderer.render_bytes("probe.1", MDOC).unwrap();
    assert_eq!(bytes.output, EXPECTED);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURES)
        .join("reference.mdoc");
    assert_eq!(renderer.render_file(path).unwrap().output, EXPECTED);

    let mut bundle = SourceBundle::new();
    bundle
        .insert("alias.1", b".so target.1\n".to_vec())
        .unwrap();
    bundle.insert("target.1", MDOC.to_vec()).unwrap();
    assert_eq!(
        renderer.render_bundle("alias.1", &bundle).unwrap().output,
        EXPECTED
    );

    let limited = renderer.with_max_output_bytes(EXPECTED.len() - 1);
    assert_eq!(
        limited.render_bytes("probe.1", MDOC).unwrap_err().kind,
        RenderErrorKind::OutputLimit
    );
    assert_eq!(
        Renderer::new(RenderFormat::Markdown)
            .render_bytes("probe.1", MDOC)
            .unwrap()
            .output,
        EXPECTED
    );
}

#[test]
fn markdown_reports_render_time_table_and_equation_omissions() {
    // CVS md_node emits a placeholder and an UNSUPP finding for each
    // skipped native payload. Surrounding prose survives in the exact output.
    let report = Renderer::new(RenderFormat::Markdown)
        .render_bytes("omissions.1", OMISSIONS)
        .unwrap();
    assert_eq!(report.output, EXPECTED_OMISSIONS);
    let omissions: Vec<_> = report
        .diagnostics
        .iter()
        .filter_map(|diagnostic| {
            diagnostic.message.starts_with("skipping ").then_some((
                diagnostic.level,
                diagnostic.message.as_str(),
                diagnostic.location.map(|loc| (loc.line, loc.column)),
            ))
        })
        .collect();
    assert_eq!(
        omissions,
        [
            (
                DiagnosticLevel::Unsupported,
                "skipping tbl in -Tmarkdown mode",
                Some((11, 1))
            ),
            (
                DiagnosticLevel::Unsupported,
                "skipping eqn in -Tmarkdown mode",
                Some((13, 2))
            ),
        ]
    );
}

#[test]
fn markdown_rejects_the_resolved_man_dialect() {
    // main.c dispatches on the parsed macro set, including .so aliases.
    let renderer = Renderer::new(RenderFormat::Markdown);
    let error = renderer.render_bytes("traditional.1", MAN).unwrap_err();
    assert_eq!(error.kind, RenderErrorKind::Unsupported);
    assert!(error.message.contains("requires mdoc(7)"));
    let mut bundle = SourceBundle::new();
    bundle
        .insert("alias.1", b".so target.1\n".to_vec())
        .unwrap();
    bundle.insert("target.1", MAN.to_vec()).unwrap();
    assert_eq!(
        renderer.render_bundle("alias.1", &bundle).unwrap_err().kind,
        RenderErrorKind::Unsupported
    );
}

#[test]
fn markdown_calls_isolate_state_and_output_from_one_another() {
    thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                let renderer = Renderer::new(RenderFormat::Markdown);
                for _ in 0..12 {
                    assert_eq!(
                        renderer.render_bytes("probe.1", MDOC).unwrap().output,
                        EXPECTED
                    );
                    assert_eq!(
                        renderer
                            .render_bytes("omissions.1", OMISSIONS)
                            .unwrap()
                            .output,
                        EXPECTED_OMISSIONS
                    );
                }
            });
        }
    });
}

#[test]
fn markdown_child_render() {
    if std::env::var_os("LIBMANDOC_RS_MARKDOWN_STDIO_CHILD").is_some() {
        let report = Renderer::new(RenderFormat::Markdown)
            .render_bytes("probe.1", MDOC)
            .unwrap();
        assert_eq!(report.output, EXPECTED);
    }
}

#[test]
fn markdown_does_not_write_to_process_stdout() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "markdown_child_render", "--nocapture"])
        .env("LIBMANDOC_RS_MARKDOWN_STDIO_CHILD", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("native Markdown probe"),
        "{output:?}"
    );
    assert!(output.stderr.is_empty(), "{output:?}");
}
