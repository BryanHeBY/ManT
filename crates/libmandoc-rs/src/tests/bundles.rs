//! Caller-owned virtual source trees and session isolation.

use super::*;

#[test]
fn source_bundle_normalizes_current_directory_and_resolves_same_directory_includes() {
    let mut bundle = SourceBundle::new();
    bundle
        .insert("man1/alias.1", b".so man1/redirect.1\n".to_vec())
        .expect("insert root source");
    bundle
        .insert("man1/redirect.1", b".so ./target.1\n".to_vec())
        .expect("insert redirect source");
    bundle
        .insert(
            "man1/target.1",
            b".TH BUNDLE-TARGET 1\n.SH NAME\nbundle-target \\- virtual source\n".to_vec(),
        )
        .expect("insert target source");

    let report = Parser::default()
        .parse_bundle("man1/alias.1", &bundle)
        .expect("parse virtual source tree");
    assert_eq!(
        report.document.metadata.title.as_deref(),
        Some("BUNDLE-TARGET")
    );
}

#[test]
fn source_bundle_missing_include_is_diagnostic_not_a_host_lookup() {
    let missing = format!("mant-bundle-missing-{}.1", process::id());
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "man1/root.1",
            format!(".TH BUNDLE-ROOT 1\n.SH NAME\nbundle-root \\- isolated\n.so {missing}\n")
                .into_bytes(),
        )
        .expect("insert isolated root");

    let report = Parser::default()
        .parse_bundle("man1/root.1", &bundle)
        .expect("missing include degrades to a diagnostic");
    assert_eq!(
        report.document.metadata.title.as_deref(),
        Some("BUNDLE-ROOT")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains(&missing)),
        "missing bundle source must be reported: {:?}",
        report.diagnostics
    );
}

#[test]
fn concurrent_source_bundles_keep_virtual_trees_isolated() {
    const WORKERS: usize = 8;
    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                let title = format!("BUNDLE-{worker}");
                let mut bundle = SourceBundle::new();
                bundle
                    .insert("man1/alias.1", b".so target.1\n".to_vec())
                    .expect("insert alias");
                bundle
                    .insert(
                        "man1/target.1",
                        format!(".TH {title} 1\n.SH NAME\nbundle-{worker} \\- isolated\n")
                            .into_bytes(),
                    )
                    .expect("insert worker target");
                start.wait();
                for _ in 0..16 {
                    let report = Parser::default()
                        .parse_bundle("man1/alias.1", &bundle)
                        .expect("parse concurrent bundle");
                    assert_eq!(
                        report.document.metadata.title.as_deref(),
                        Some(title.as_str())
                    );
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("bundle worker must not panic");
    }
}
