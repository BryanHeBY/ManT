//! Stress probes run under the mixed Rust/C `ThreadSanitizer` build.

use std::{
    env,
    sync::{Arc, Barrier},
};

#[cfg(unix)]
use std::{fs, process};

#[cfg(unix)]
use libmandoc_rs::{IncludePolicy, ParseOptions};
use libmandoc_rs::{Parser, SourceBundle};
#[cfg(feature = "render")]
use libmandoc_rs::{RenderFormat, Renderer};

const WORKERS: usize = 8;
const DEFAULT_ROUNDS: usize = 64;

fn rounds() -> usize {
    env::var("LIBMANDOC_RS_TSAN_ROUNDS").map_or(DEFAULT_ROUNDS, |value| {
        let rounds = value
            .parse::<usize>()
            .expect("LIBMANDOC_RS_TSAN_ROUNDS must be a positive integer");
        assert!(
            (1..=10_000).contains(&rounds),
            "LIBMANDOC_RS_TSAN_ROUNDS must be between 1 and 10000"
        );
        rounds
    })
}

#[test]
#[ignore = "run crates/libmandoc-rs/scripts/check-thread-safety"]
fn concurrent_memory_sessions_isolate_parser_and_diagnostic_state() {
    let parser = Arc::new(Parser::default());
    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let parser = Arc::clone(&parser);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for round in 0..rounds() {
                    let identity = format!("tsan-{worker}-{round}");
                    let title = format!("TSAN-{worker}-{round}");
                    let date = if round % 2 == 0 {
                        "Jul 20, 2020"
                    } else {
                        "$Mdocdate$"
                    };
                    let source = format!(
                        ".Dd {date}\n.Dt {title} 1\n.Os\n\
                         .Sh NAME\n.Nm {identity}\n.Nd concurrent \\(em parser state\n\
                         .Sh SYNOPSIS\n.Fn {identity} value\n\
                         .Sh LIBRARY\n.Lb {identity}\n\
                         .Sh DESCRIPTION\n.ce 1\n{identity}\n\
                         .EQ\nx sup 2\n.EN\n\
                         .TS\nl l.\n{identity}\tvalue\n.TE\n\
                         .Sh SEE ALSO\n.Xr pthread_create 3\n"
                    );

                    let report = parser
                        .parse_bytes(format!("{identity}.1"), source.as_bytes())
                        .expect("concurrent in-memory parse must succeed");
                    assert_eq!(
                        report.document.metadata.title.as_deref(),
                        Some(title.as_str())
                    );
                    assert_eq!(
                        report.document.metadata.name.as_deref(),
                        Some(identity.as_str())
                    );
                    assert!(
                        report
                            .diagnostics
                            .iter()
                            .any(|diagnostic| diagnostic.message.contains(&identity)),
                        "worker diagnostic must remain attached to its parser session: {:?}",
                        report.diagnostics
                    );
                }
            })
        })
        .collect();

    for worker in workers {
        worker.join().expect("memory parser worker must not panic");
    }
}

#[test]
#[ignore = "run crates/libmandoc-rs/scripts/check-thread-safety"]
fn concurrent_virtual_source_trees_isolate_bundle_state() {
    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                let title = format!("TSAN-BUNDLE-{worker}");
                let mut bundle = SourceBundle::new();
                bundle
                    .insert("man1/alias.1", b".so target.1\n".to_vec())
                    .expect("insert bundle alias");
                bundle
                    .insert(
                        "man1/target.1",
                        format!(".TH {title} 1\n.SH NAME\ntsan-bundle-{worker} \\- isolated\n")
                            .into_bytes(),
                    )
                    .expect("insert bundle target");
                start.wait();
                for _ in 0..rounds() {
                    let report = Parser::default()
                        .parse_bundle("man1/alias.1", &bundle)
                        .expect("concurrent bundle parse must succeed");
                    assert_eq!(
                        report.document.metadata.title.as_deref(),
                        Some(title.as_str())
                    );
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("bundle parser worker must not panic");
    }
}

#[cfg(feature = "render")]
#[test]
#[ignore = "run crates/libmandoc-rs/scripts/check-thread-safety"]
fn concurrent_renderers_isolate_formatter_and_output_state() {
    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                let identity = format!("tsan-render-{worker}");
                let source =
                    format!(".TH TSAN-RENDER-{worker} 1\n.SH NAME\n{identity} \\- isolated\n");
                let format = match worker % 3 {
                    0 => RenderFormat::Ascii,
                    1 => RenderFormat::Utf8,
                    _ => RenderFormat::Html,
                };
                start.wait();
                for _ in 0..rounds() {
                    let report = Renderer::new(format)
                        .render_bytes(format!("{identity}.1"), source.as_bytes())
                        .expect("concurrent render must succeed");
                    assert!(report.output.contains(&identity));
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("renderer worker must not panic");
    }
}

#[cfg(unix)]
#[test]
#[ignore = "run crates/libmandoc-rs/scripts/check-thread-safety"]
fn concurrent_source_tree_sessions_isolate_include_roots() {
    let root = env::temp_dir().join(format!("libmandoc-rs-tsan-source-trees-{}", process::id()));
    let aliases: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let tree = root.join(format!("tree-{worker}")).join("man1");
            fs::create_dir_all(&tree).expect("create isolated manual tree");
            fs::write(
                tree.join("target.1"),
                format!(
                    ".Dd $Mdocdate$\n.Dt TSAN-INCLUDE-{worker} 1\n.Os\n\
                     .Sh NAME\n.Nm tsan-include-{worker}\n.Nd isolated include tree\n"
                ),
            )
            .expect("write included manual source");
            let alias = tree.join("alias.1");
            fs::write(&alias, ".so target.1\n").expect("write manual redirect");
            alias
        })
        .collect();

    let parser = Arc::new(Parser::new(ParseOptions {
        includes: IncludePolicy::SourceTree,
        ..ParseOptions::default()
    }));
    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = aliases
        .into_iter()
        .enumerate()
        .map(|(worker, alias)| {
            let parser = Arc::clone(&parser);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..rounds() {
                    let report = parser
                        .parse_file(&alias)
                        .expect("concurrent source-tree parse must succeed");
                    assert_eq!(
                        report.document.metadata.title.as_deref(),
                        Some(format!("TSAN-INCLUDE-{worker}").as_str())
                    );
                }
            })
        })
        .collect();

    for worker in workers {
        worker.join().expect("include parser worker must not panic");
    }
    fs::remove_dir_all(root).expect("remove isolated manual trees");
}
