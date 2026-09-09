//! Parser release, sequential resets, and independent TLS sessions.

use super::*;
use std::fmt::Write as _;

#[test]
fn owned_bundle_report_survives_released_inputs_and_later_failed_sessions() {
    let parser = Parser::default();
    let retained = {
        let mut bundle = SourceBundle::new();
        bundle
            .insert("man1/root.1", b".so owned.1\n".to_vec())
            .unwrap();
        bundle
            .insert(
                "man1/owned.1",
                b".TH OWNED 1\n.SH BODY\nretained original bytes\n".to_vec(),
            )
            .unwrap();
        parser.parse_bundle("man1/root.1", &bundle).unwrap()
    };
    let expected_diagnostics = retained.diagnostics.clone();

    // Exercise an actual native rejection rather than a synthetic allocation
    // failure. Releasing the failed native session must not affect old reports.
    let rejected = format!(
        ".TH REJECTED 1\n.SH BODY\n{}too deep\n",
        ".RS 0\n".repeat(1_000)
    );
    let error = parser
        .parse_bytes("rejected.1", rejected.as_bytes())
        .unwrap_err();
    assert!(error.message.contains("nesting limit"), "{error}");
    let next = parser
        .parse_bytes("next.1", b".TH NEXT 1\n.SH BODY\nnew session\n")
        .unwrap();
    assert_eq!(next.document.metadata.title.as_deref(), Some("NEXT"));

    // Neither the input bundle, native parser nor calling thread owns any
    // storage borrowed by the returned public report.
    std::thread::spawn(move || {
        assert_eq!(retained.document.metadata.title.as_deref(), Some("OWNED"));
        let mut visible = Vec::new();
        collect_visible_text(&retained.document.root, &mut visible);
        assert!(visible.contains(&"retained original bytes"));
        assert!(!visible.contains(&"new session"));
        assert_eq!(retained.diagnostics, expected_diagnostics);
    })
    .join()
    .expect("owned report remains readable on another thread");
}

#[test]
fn parser_session_returns_an_owned_man_tree() {
    let path = source_path("mandoc-session");
    fs::write(
        &path,
        ".TH MANT 1 \"2026-07-19\"\n.SH NAME\nmant \\- manual viewer\n",
    )
    .expect("write temporary manual source");

    let document = parse_file(&path, false).expect("parse temporary manual");
    fs::remove_file(path).expect("remove temporary manual source");

    assert_eq!(document.macro_set, MacroSet::Man);
    assert_eq!(document.metadata.title.as_deref(), Some("MANT"));
    assert_eq!(document.metadata.section.as_deref(), Some("1"));
    assert!(document.metadata.has_body);
    assert_eq!(document.root.kind, NodeKind::Root);
    assert!(!document.root.children.is_empty());
}

#[test]
fn parser_session_reports_file_errors_as_values() {
    let path = source_path("missing-mandoc-session");
    let error = parse_file(&path, false).expect_err("missing source must fail");

    assert_eq!(error.path, path);
    assert!(!error.message.is_empty());
}

#[test]
fn parser_replaces_repeated_input_traps_without_losing_following_content() {
    let mut source = String::from(".TH TRAPS 1\n.SH BODY\n");
    for index in 0..1_024 {
        writeln!(&mut source, ".it 100000 trap-{index}").expect("write test trap");
    }
    source.push_str(".SH TAIL\nretained tail marker\n");
    let report = Parser::default()
        .parse_bytes("traps.1", source.as_bytes())
        .expect("replacing input traps must retain a finite parse");
    let mut visible = Vec::new();
    collect_visible_text(&report.document.root, &mut visible);
    assert!(visible.join(" ").contains("retained tail marker"));
}

#[test]
fn parser_sessions_reset_unfinished_roff_requests() {
    let parser = Parser::default();
    for round in 0..32 {
        parser
            .parse_bytes(
                format!("unfinished-trap-{round}.1"),
                b".TH UNFINISHED-TRAP 1\n.it 2 br\n",
            )
            .expect("parse page ending with an armed input trap");
        parser
            .parse_bytes(
                format!("unfinished-center-{round}.1"),
                b".TH UNFINISHED-CENTER 1\n.ce 2\nonly-one-line\n",
            )
            .expect("parse page ending with an active centering request");
        let next = parser
            .parse_bytes(
                format!("clean-session-{round}.1"),
                b".TH CLEAN-SESSION 1\n.SH NAME\nclean-session \\- independent state\n",
            )
            .expect("subsequent parser session must remain independent");
        assert_eq!(
            next.document.metadata.title.as_deref(),
            Some("CLEAN-SESSION")
        );
    }
}

#[test]
fn concurrent_callers_keep_thread_local_parser_state_isolated() {
    const WORKERS: usize = 8;
    const ROUNDS: usize = 16;

    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for round in 0..ROUNDS {
                    let title = format!("TLS-{worker}-{round}");
                    let source = format!(
                        ".Dd August 19, 2026\n.Dt {title} 1\n.Os\n.Sh NAME\n.Nm tls-{worker}-{round}\n.Nd concurrent \\(em parser state\n.Sh SEE ALSO\n.Xr pthread_create 3\n"
                    );
                    let report = Parser::default()
                        .parse_bytes(format!("tls-{worker}-{round}.1"), source.as_bytes())
                        .expect("concurrent memory parse must succeed");
                    assert_eq!(report.document.metadata.title.as_deref(), Some(title.as_str()));
                    let name = format!("tls-{worker}-{round}");
                    assert_eq!(report.document.metadata.name.as_deref(), Some(name.as_str()));
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("parser worker must not panic");
    }
}
