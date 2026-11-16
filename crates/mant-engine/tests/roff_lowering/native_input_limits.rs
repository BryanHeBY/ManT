//! Successful native input guards retain safe text and honest completeness.

use libmandoc_rs::{DiagnosticCode, DiagnosticLevel, Parser};
use mant_ir::{DiagnosticImpact, ResolvedContent};
use mant_protocol::{ExplanationQuery, QueryBundle, SearchQuery};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    limited: bool,
}

fn assert_native_limit(parser: &Parser, case: &Case) {
    let report = parser
        .parse_bytes("limits.1", case.source.as_bytes())
        .unwrap();
    let findings = report
        .diagnostics
        .iter()
        .filter(|finding| finding.code() == Some(DiagnosticCode::InputProcessingLimit))
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), usize::from(case.limited), "{}", case.id);
    for finding in findings {
        assert_eq!(finding.level, DiagnosticLevel::Error);
        assert_eq!(finding.location.unwrap().line, 9);
    }
}

#[test]
fn native_input_limits_survive_json_and_all_query_summaries() {
    // Both exact sources ran all five pristine profiles before these asserts.
    // Pristine mdoc_macro.c::in_line parses every Fl; the wrapper's pinned
    // mdoc_macro_call safety guard instead keeps the rejected rest as literal
    // dword TEXT. That safe suffix is not proof of complete macro execution.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("native_input_limits/cases.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 2);
    let cases: Vec<Case> = serde_json::from_value(fixture["cases"].clone()).unwrap();
    let parser = Parser::default();
    for case in cases {
        assert_native_limit(&parser, &case);
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = serde_json::to_string(&QueryBundle::from(&original)).unwrap();
        let wire: QueryBundle = serde_json::from_str(&json).unwrap();
        let response = wire.document.as_ref().unwrap();
        assert_eq!(
            mant_ir::content_complete(&response.diagnostics),
            !case.limited
        );
        assert_eq!(
            mant_ir::semantics_complete(&response.diagnostics),
            !case.limited
        );
        let content: ResolvedContent = wire.into();
        assert_eq!(content, original);
        let document = content.document.as_ref().unwrap();
        let lowered = document
            .diagnostics
            .iter()
            .filter(|finding| finding.code.as_deref() == Some("manual.input-processing-limit"))
            .collect::<Vec<_>>();
        assert_eq!(lowered.len(), usize::from(case.limited));
        for finding in lowered {
            assert_eq!(finding.impact, DiagnosticImpact::ContentCoverage);
            assert_eq!(finding.level, mant_ir::DiagnosticLevel::Error);
            assert_eq!(finding.source.as_ref().unwrap().line, 9);
        }
        let text = mant_render::render_query_text(&content);
        assert!(text.contains("BodyWord"), "{}: safe body", case.id);
        if case.limited {
            assert!(text.contains("-flag0"), "safe accepted prefix");
            assert!(text.contains("flag254"), "safe literal suffix");
        }
        let outline = mant_query::build_outline(&content).unwrap();
        assert_eq!(outline.content_complete, !case.limited);
        assert_eq!(outline.semantics_complete, !case.limited);
        let search = mant_query::search_query(
            &content,
            &SearchQuery {
                pattern: "BodyWord".into(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(search.total, 1);
        assert_eq!(search.content_complete, !case.limited);
        assert_eq!(
            mant_ir::semantics_complete(&search.diagnostics),
            !case.limited
        );
        let explanation = mant_query::explain_query(
            &content,
            &ExplanationQuery {
                entry: "BodyWord".into(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(explanation.content_complete, !case.limited);
        assert_eq!(explanation.semantics_complete, !case.limited);
        let excerpt = mant_query::select_excerpt(
            &content,
            &[mant_protocol::ContentSelector::Path { path: "1".into() }],
        )
        .unwrap();
        assert_eq!(excerpt.content_complete, !case.limited);
        assert_eq!(excerpt.semantics_complete, !case.limited);
    }
}

#[cfg(unix)]
#[test]
fn native_diagnostic_source_labels_leave_loader_and_query_coverage_intact() {
    use std::{ffi::OsString, fs, os::unix::ffi::OsStringExt as _, path::PathBuf};

    use mant_loader::{DocumentLoader, LoadPolicy, LoadSpec};
    use mant_protocol::InputFormat;

    const PREAMBLE: &str =
        ".TH PROBE 1 2026-11-17\n.SH NAME\nprobe \\- diagnostic capture\n.SH DESCRIPTION\nBEFORE\n";
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let scratch = Scratch(
        std::env::temp_dir().join(format!("mant-native-diagnostics-{}", std::process::id())),
    );
    fs::create_dir(&scratch.0).unwrap();
    let loader = DocumentLoader::from_system();
    let search = SearchQuery {
        pattern: "AFTER".into(),
        syntax: mant_protocol::SearchSyntax::Literal,
        case: mant_protocol::SearchCase::Sensitive,
        scope: mant_protocol::SearchScope::Visible,
        word: false,
        context_lines: 0,
        limit: 10,
        offset: 0,
    };
    let saturated = ".ab\n".repeat(4097);
    let late_limit = format!("{saturated}.de A\n.A\n..\n.A\n");
    for (body, limited) in [
        (".ZZ input stack limit exceeded, infinite loop?\n", false),
        (".de A\n.A\n..\n.A\n", true),
        (saturated.as_str(), false),
        (late_limit.as_str(), true),
    ] {
        // The pinned CVS emits MANDOCERR_MACRO for the first input and
        // MANDOCERR_ROFFLOOP for the second, retaining BEFORE and AFTER.
        let source = format!("{PREAMBLE}{body}AFTER\n");
        let baseline = scratch.0.join("probe.1");
        fs::write(&baseline, &source).unwrap();
        let expected = mant_loader::parse_manual_source_with_report(&baseline).unwrap();
        for name in [
            OsString::from("probe: ERROR: page.1"),
            OsString::from("probe: UNSUPP: page.1"),
            OsString::from_vec(b"probe-\xff.1".to_vec()),
        ] {
            let path = scratch.0.join(name);
            fs::write(&path, &source).unwrap();
            let (document, report) = mant_loader::parse_manual_source_with_report(&path).unwrap();
            assert_eq!(report, expected.1, "{path:?}");
            assert_eq!(document.diagnostics, expected.0.diagnostics, "{path:?}");
            assert_eq!(mant_ir::content_complete(&document.diagnostics), !limited);
            let Some(path) = path.to_str() else {
                // This is an actual opaque Unix path. The Path API above
                // has no obligation to invent UTF-8 for the CLI.
                continue;
            };
            let content = loader
                .load(
                    LoadSpec::File {
                        path,
                        format: InputFormat::Roff,
                    },
                    LoadPolicy::Combined,
                )
                .unwrap();
            let encoded = serde_json::to_string(&QueryBundle::from(&content)).unwrap();
            let decoded: QueryBundle = serde_json::from_str(&encoded).unwrap();
            let restored: ResolvedContent = decoded.into();
            let searched = mant_query::search_query(&restored, &search).unwrap();
            assert_eq!(searched.total, 1);
            assert_eq!(searched.content_complete, !limited);
            let explained = mant_query::explain_query(
                &restored,
                &ExplanationQuery {
                    entry: "AFTER".into(),
                    options: mant_protocol::ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(explained.content_complete, !limited);
            assert_eq!(explained.semantics_complete, !limited);
        }
    }
}
