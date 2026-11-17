use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use libmandoc_rs::{Diagnostic, DiagnosticCode, DiagnosticLevel, Node, Parser, SourceLocation};

#[cfg(feature = "render")]
use libmandoc_rs::{RenderFormat, Renderer};

const LOOP: &str = ".de A\n.A\n..\n.A\n";

fn manifest() -> &'static serde_json::Value {
    static MANIFEST: OnceLock<serde_json::Value> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("diagnostic_capture/cases.json")).unwrap()
    })
}

fn labels() -> Vec<&'static str> {
    manifest()["utf8Labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|label| label.as_str().unwrap())
        .collect()
}

#[cfg(unix)]
fn opaque_label() -> OsString {
    use std::os::unix::ffi::OsStringExt as _;

    OsString::from_vec(b"probe-\xff.1".to_vec())
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "mant-diagnostic-capture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cases() -> impl Iterator<Item = (&'static str, Option<Diagnostic>)> {
    manifest()["cases"].as_array().unwrap().iter().map(|case| {
        let finding = &case["finding"];
        let diagnostic = (!finding.is_null()).then(|| {
            let code = match finding["kind"].as_str() {
                None => None,
                Some("inputProcessingLimit") => Some(DiagnosticCode::InputProcessingLimit),
                other => panic!("unsupported diagnostic kind in fixture: {other:?}"),
            };
            let level = match finding["level"].as_str().unwrap() {
                "style" => DiagnosticLevel::Style,
                "warning" => DiagnosticLevel::Warning,
                "error" => DiagnosticLevel::Error,
                "unsupported" => DiagnosticLevel::Unsupported,
                other => panic!("unsupported diagnostic level in fixture: {other}"),
            };
            Diagnostic {
                code,
                level,
                message: finding["message"].as_str().unwrap().to_owned(),
                location: Some(SourceLocation {
                    line: finding["line"].as_u64().unwrap().try_into().unwrap(),
                    column: finding["column"].as_u64().unwrap().try_into().unwrap(),
                }),
            }
        });
        (case["body"].as_str().unwrap(), diagnostic)
    })
}

fn source(body: &str) -> String {
    let fixture = manifest();
    format!(
        "{}{body}{}",
        fixture["preamble"].as_str().unwrap(),
        fixture["trailer"].as_str().unwrap()
    )
}

fn native_source_has(node: &Node, value: &str) -> bool {
    node.text.as_deref() == Some(value)
        || node
            .children
            .iter()
            .any(|node| native_source_has(node, value))
}

fn assert_bytes(path: &Path, body: &str, finding: Option<&Diagnostic>) {
    let source = source(body);
    let parser = Parser::default();
    let canonical = parser.parse_bytes("probe.1", source.as_bytes()).unwrap();
    let report = parser.parse_bytes(path, source.as_bytes()).unwrap();
    assert_eq!(report, canonical, "{} / {body:?}", path.display());
    assert_eq!(
        report.diagnostics,
        finding.map_or(&[][..], std::slice::from_ref)
    );
    for word in ["BEFORE", "AFTER"] {
        assert!(native_source_has(&report.document.root, word));
    }
    #[cfg(feature = "render")]
    {
        let renderer = Renderer::new(RenderFormat::Utf8);
        let canonical = renderer.render_bytes("probe.1", source.as_bytes()).unwrap();
        let output = renderer.render_bytes(path, source.as_bytes()).unwrap();
        assert_eq!(output, canonical, "{} / {body:?}", path.display());
        assert_eq!(output.diagnostics, report.diagnostics);
        for word in ["BEFORE", "AFTER"] {
            assert!(output.output.contains(word));
        }
    }
}

#[test]
fn diagnostic_facts_are_independent_of_the_source_label_and_display_markers() {
    // CVS mandoc_msg() reports its severity, line and column before formatting
    // the source. Only read.c's MANDOCERR_ROFFLOOP means input was omitted.
    let labels = labels();
    for (body, finding) in cases() {
        for label in &labels {
            assert_bytes(Path::new(label), body, finding.as_ref());
        }
        #[cfg(unix)]
        assert_bytes(Path::new(&opaque_label()), body, finding.as_ref());
    }
}

#[test]
fn file_and_byte_entries_share_diagnostic_capture() {
    let scratch = Scratch::new();
    let labels = labels();
    let portable_labels = if cfg!(windows) { &labels[..2] } else { &labels };
    let names = portable_labels.iter().map(OsString::from);
    // APFS rejects opaque byte names before the native parser opens the
    // file. Exercise their filesystem path on Linux and the bytes API on Unix.
    #[cfg(target_os = "linux")]
    let names = names.chain([opaque_label()]);
    let parser = Parser::default();
    for name in names {
        let path = scratch.0.join(name);
        for (body, finding) in cases() {
            let input = source(body);
            fs::write(&path, input.as_bytes()).unwrap();
            assert_bytes(&path, body, finding.as_ref());
            let file = parser.parse_file(&path).unwrap();
            let bytes = parser.parse_bytes(&path, input.as_bytes()).unwrap();
            assert_eq!(file, bytes, "{path:?} / {body:?}");
            #[cfg(feature = "render")]
            {
                let renderer = Renderer::new(RenderFormat::Utf8);
                let output = renderer.render_file(&path).unwrap();
                assert_eq!(
                    output,
                    renderer.render_bytes(&path, input.as_bytes()).unwrap(),
                    "{path:?} / {body:?}"
                );
                assert_eq!(output.diagnostics, file.diagnostics);
            }
        }
    }
}

#[test]
fn isolated_findings_bound_display_without_changing_location_or_code() {
    let input = source(&format!(".ZZ {}\n", "x".repeat(5000)));
    let report = Parser::default()
        .parse_bytes("probe.1", input.as_bytes())
        .unwrap();
    let [diagnostic] = report.diagnostics.as_slice() else {
        panic!("expected one unknown-macro finding");
    };
    assert_eq!(diagnostic.code(), None);
    assert_eq!(diagnostic.level, DiagnosticLevel::Error);
    assert_eq!(
        diagnostic.location,
        Some(SourceLocation { line: 6, column: 2 })
    );
    assert_eq!(diagnostic.message.len(), 4096);
    assert!(
        diagnostic
            .message
            .starts_with("skipping unknown macro: .ZZ ")
    );
    assert!(diagnostic.message.ends_with(" [truncated]"));
    assert!(native_source_has(&report.document.root, "AFTER"));
    #[cfg(feature = "render")]
    {
        let rendered = Renderer::new(RenderFormat::Utf8)
            .render_bytes("probe.1", input.as_bytes())
            .unwrap();
        assert_eq!(rendered.diagnostics, report.diagnostics);
        assert!(rendered.output.contains("AFTER"));
    }
}

#[test]
fn saturated_detail_still_reports_late_input_loss_and_preserves_content() {
    use std::fmt::Write as _;

    // CVS roff_unsupp() emits each .ab finding during input execution.
    // Unique unknown macro names exercise the independent message byte budget.
    let parser = Parser::default();
    let exact = source(&".ab\n".repeat(4096));
    assert_eq!(
        parser
            .parse_bytes("probe.1", exact.as_bytes())
            .unwrap()
            .diagnostics
            .len(),
        4096
    );
    let mut long_messages = String::new();
    let detail = "x".repeat(5000);
    for index in 0..1025 {
        writeln!(long_messages, ".Z{index} {detail}").unwrap();
    }
    for (body, budget, retained, limit_line) in [
        (".ab\n".repeat(4097), "record count budget", 4096, 4106),
        (long_messages, "message byte budget", 1024, 1034),
    ] {
        for limited in [false, true] {
            let case = source(&format!("{body}{}", if limited { LOOP } else { "" }));
            let report = parser.parse_bytes("probe.1", case.as_bytes()).unwrap();
            assert!(native_source_has(&report.document.root, "BEFORE"));
            assert!(native_source_has(&report.document.root, "AFTER"));
            assert_eq!(
                report.diagnostics.len(),
                retained + 1 + usize::from(limited),
                "{budget}, {limited}: {:?}",
                &report.diagnostics[report.diagnostics.len().saturating_sub(2)..]
            );
            assert!(
                report.diagnostics[..retained]
                    .iter()
                    .all(|finding| finding.code().is_none())
            );
            if limited {
                let input_limit = &report.diagnostics[retained];
                assert_eq!(
                    input_limit.code(),
                    Some(DiagnosticCode::InputProcessingLimit)
                );
                assert_eq!(input_limit.level, DiagnosticLevel::Error);
                assert_eq!(
                    input_limit.location,
                    Some(SourceLocation {
                        line: limit_line,
                        column: 4
                    })
                );
            }
            let summary = report.diagnostics.last().unwrap();
            assert_eq!(summary.level, DiagnosticLevel::Warning);
            assert_eq!(summary.code(), None);
            assert_eq!(summary.location, None);
            assert!(summary.message.contains(budget), "{summary:?}");
            #[cfg(feature = "render")]
            {
                let output = Renderer::new(RenderFormat::Utf8)
                    .render_bytes("probe.1", case.as_bytes())
                    .unwrap();
                assert_eq!(output.diagnostics, report.diagnostics);
                assert!(output.output.contains("BEFORE"));
                assert!(output.output.contains("AFTER"));
            }
            assert_bytes(Path::new("probe.1"), "", None);
        }
    }
}
