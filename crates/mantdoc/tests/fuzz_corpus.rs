//! Deterministic replay for the checked-in native fuzz seeds.
//!
//! `cargo-fuzz` remains the mutation engine, but every discovered seed must
//! also be replayable by ordinary stable-Rust tests. This keeps the useful
//! regression boundary available in release-candidate CI and exercises the
//! parser's independent-session guarantee under a small bounded worker pool.

use std::{fmt::Write as _, fs, path::PathBuf, thread};

use mantdoc::{FatalErrorKind, Parser, ParserConfig, Source, SourceName};

const MAX_FUZZ_INPUT_BYTES: usize = 128 * 1024;
type NodeFingerprint = (mantdoc::NodeKind, Option<String>, Option<String>);
type ReportFingerprint = (Vec<NodeFingerprint>, Vec<(String, mantdoc::Severity)>);

#[test]
fn native_fuzz_seed_corpus_replays_in_parallel_with_bounded_spans() {
    let inputs = corpus_inputs("mantdoc_scanner");
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 4)
        .min(inputs.len());
    let chunk_size = inputs.len().div_ceil(workers);

    thread::scope(|scope| {
        for input_chunk in inputs.chunks(chunk_size) {
            scope.spawn(move || {
                let parser = Parser::default();
                let name =
                    SourceName::new("fuzz-corpus.roff").expect("fixed corpus source name is valid");
                for input in input_chunk {
                    assert!(input.len() <= MAX_FUZZ_INPUT_BYTES);
                    let report = parser
                        .parse(Source::new(&name, input))
                        .expect("fuzz seed stays inside the root-source limit");
                    assert_report_is_bounded(&report, input);
                }
            });
        }
    });
}

#[test]
fn concurrent_parser_sessions_remain_reentrant_under_repeated_manual_parses() {
    const WORKERS: usize = 8;
    const ITERATIONS_PER_WORKER: usize = 64;

    let source = realistic_manual();
    let name = SourceName::new("concurrent-session.1").expect("fixed source name is valid");
    let expected = report_fingerprint(
        &Parser::default()
            .parse(Source::new(&name, source.as_bytes()))
            .expect("generated manual parses"),
    );

    thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                let parser = Parser::default();
                let name =
                    SourceName::new("concurrent-session.1").expect("fixed source name is valid");
                for _ in 0..ITERATIONS_PER_WORKER {
                    let report = parser
                        .parse(Source::new(&name, source.as_bytes()))
                        .expect("generated manual stays within parser limits");
                    assert_eq!(report_fingerprint(&report), expected);
                }
            });
        }
    });
}

#[test]
fn one_mebibyte_valid_manual_stays_within_default_resource_budgets() {
    const PARAGRAPHS: usize = 12_000;

    let mut source = String::with_capacity(1024 * 1024 + 64 * 1024);
    source.push_str(".TH LARGE-MANUAL 1\n.SH NAME\nlarge-manual \\- stress fixture\n");
    for index in 0..PARAGRAPHS {
        writeln!(
            source,
            ".PP\nparagraph {index:05} keeps a stable amount of prose, \\fBbold\\fR text, and an escaped \\(em dash."
        )
        .expect("writing into String is infallible");
    }
    assert!(
        source.len() >= 1024 * 1024,
        "fixture must remain a mebibyte-scale input"
    );

    let name = SourceName::new("large-manual.1").expect("fixed source name is valid");
    let report = Parser::default()
        .parse(Source::new(&name, source.as_bytes()))
        .expect("mebibyte-scale valid manual stays below default root limit");
    assert!(!report.statistics.truncated, "{:#?}", report.diagnostics);
    assert!(report.document.node_count() >= PARAGRAPHS);
    assert_report_is_bounded(&report, source.as_bytes());
}

#[test]
fn root_byte_limit_accepts_the_boundary_and_rejects_one_extra_byte() {
    let name = SourceName::new("root-limit.1").expect("fixed source name is valid");
    let mut config = ParserConfig::default();
    config.limits.max_root_source_bytes = 32;
    config.limits.max_total_source_bytes = 32;
    let parser = Parser::new(config);

    assert!(parser.parse(Source::new(&name, &[b'x'; 32])).is_ok());
    let error = parser
        .parse(Source::new(&name, &[b'x'; 33]))
        .expect_err("one byte above the root limit is rejected before AST allocation");
    assert_eq!(error.kind, FatalErrorKind::SourceLimit);
}

#[cfg(feature = "render")]
#[test]
fn renderer_fuzz_seed_corpus_finishes_with_bounded_output() {
    use mantdoc::{Compression, RenderFormat, Renderer, SourceName};

    let name = SourceName::new("render-fuzz-corpus.1").expect("fixed corpus source name is valid");
    for input in corpus_inputs("roff_pipeline") {
        assert!(input.len() <= MAX_FUZZ_INPUT_BYTES);
        for format in [RenderFormat::Ascii, RenderFormat::Utf8, RenderFormat::Html] {
            let report = Renderer::new(format)
                .with_max_output_bytes(MAX_FUZZ_INPUT_BYTES)
                .render_bytes(&name, &input, Compression::Plain)
                .expect("configured renderer accepts bounded native fuzz seed");
            assert!(report.output.len() <= MAX_FUZZ_INPUT_BYTES);
        }
    }
}

fn corpus_inputs(target: &str) -> Vec<Vec<u8>> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus")
        .join(target);
    let mut paths = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        .map(|entry| entry.expect("read corpus directory entry").path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "native fuzz corpus {target} must retain at least one regression seed"
    );
    paths
        .into_iter()
        .map(|path| {
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        })
        .collect()
}

fn assert_report_is_bounded(report: &mantdoc::ParseReport, input: &[u8]) {
    assert!(report.document.node_count() <= report.statistics.emitted_nodes);
    assert!(report.statistics.emitted_nodes <= Parser::default().config().limits.max_nodes);
    for node in report.document.preorder() {
        assert_span_is_bounded(node.location(), &report.document, input, "node");
        let _ = node.children().count();
        let _ = node.ancestors().count();
        let _ = node.macro_name();
        let _ = node.text();
    }
    for finding in &report.diagnostics {
        assert_span_is_bounded(
            finding.primary.as_ref(),
            &report.document,
            input,
            "diagnostic",
        );
        for related in &finding.related {
            assert_span_is_bounded(
                Some(&related.span),
                &report.document,
                input,
                "related diagnostic",
            );
        }
    }
}

fn realistic_manual() -> String {
    let mut source = String::from(
        ".Dd August 27, 2026\n.Dt CONCURRENT-SESSION 1\n.Os\n.Sh NAME\n.Nm concurrent-session\n.Nd parser session stress fixture\n.Sh DESCRIPTION\n",
    );
    for index in 0..64 {
        writeln!(
            source,
            ".Pp\nThis paragraph {index} contains .Nm and .Xr printf 3 style, punctuation, and \\fBbold\\fR text."
        )
        .expect("writing into String is infallible");
    }
    source
}

fn report_fingerprint(report: &mantdoc::ParseReport) -> ReportFingerprint {
    let nodes = report
        .document
        .preorder()
        .map(|node| {
            (
                node.kind(),
                node.macro_name().map(ToOwned::to_owned),
                node.text().map(ToOwned::to_owned),
            )
        })
        .collect();
    let diagnostics = report
        .diagnostics
        .iter()
        .map(|finding| (finding.code.to_string(), finding.severity))
        .collect();
    (nodes, diagnostics)
}

fn assert_span_is_bounded(
    span: Option<&mantdoc::SourceSpan>,
    document: &mantdoc::Document,
    input: &[u8],
    label: &str,
) {
    let Some(span) = span else {
        return;
    };
    assert!(span.start <= span.end, "{label} span must be monotonic");
    assert!(
        usize::try_from(span.end).expect("u32 source offsets fit usize") <= input.len(),
        "{label} span exceeds {} input bytes",
        input.len()
    );
    assert!(document.source_position(span).is_some());
}
