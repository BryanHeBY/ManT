//! Approved filesystem roots and source-tree include policy.

use super::*;

#[cfg(unix)]
#[test]
fn zstd_sources_keep_their_original_include_root() {
    let root = std::env::temp_dir().join(format!(
        "mant-zstd-include-mandoc-session-{}",
        process::id()
    ));
    let man1 = root.join("man1");
    fs::create_dir_all(&man1).expect("create temporary manual tree");
    let target = man1.join("target.1");
    fs::write(
        &target,
        ".TH ZSTD-INCLUDE 1\n.SH NAME\nzstd-include \\- included manual\n",
    )
    .expect("write included manual");
    let alias = man1.join("alias.1.zst");
    let compressed =
        zstd::stream::encode_all(b".so man1/target.1\n".as_slice(), 1).expect("compress alias");
    fs::write(&alias, compressed).expect("write compressed alias");

    let document = parse_file(&alias, true).expect("resolve include from zstd source");
    fs::remove_dir_all(root).expect("remove temporary manual tree");

    assert_eq!(document.macro_set, MacroSet::Man);
    assert_eq!(document.metadata.title.as_deref(), Some("ZSTD-INCLUDE"));
    assert!(document.metadata.has_body);
}

#[cfg(unix)]
#[test]
fn concurrent_source_tree_includes_keep_each_root_isolated() {
    const WORKERS: usize = 8;

    let root = std::env::temp_dir().join(format!(
        "libmandoc-rs-thread-local-includes-{}",
        process::id()
    ));
    let aliases: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let tree = root.join(format!("tree-{worker}")).join("man1");
            fs::create_dir_all(&tree).expect("create isolated manual tree");
            fs::write(
                tree.join("target.1"),
                format!(
                    ".Dd August 19, 2026\n.Dt TLS-INCLUDE-{worker} 1\n.Os\n.Sh NAME\n.Nm tls-include-{worker}\n.Nd isolated include tree\n"
                ),
            )
            .expect("write included manual source");
            let alias = tree.join("alias.1");
            fs::write(&alias, ".so target.1\n").expect("write manual redirect");
            alias
        })
        .collect();

    let start = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = aliases
        .into_iter()
        .enumerate()
        .map(|(worker, alias)| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                let document =
                    parse_file(&alias, true).expect("concurrent source-tree include must succeed");
                assert_eq!(
                    document.metadata.title.as_deref(),
                    Some(format!("TLS-INCLUDE-{worker}").as_str())
                );
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("include worker must not panic");
    }
    fs::remove_dir_all(root).expect("remove isolated manual trees");
}

#[cfg(unix)]
#[test]
fn source_relative_includes_do_not_change_process_cwd() {
    let root =
        std::env::temp_dir().join(format!("libmandoc-rs-relative-include-{}", process::id()));
    fs::create_dir_all(&root).expect("create temporary manual tree");
    let target = root.join("minimal-mdoc.1");
    fs::write(
        &target,
        ".Dd July 19, 2026\n.Dt INCLUDE-FIXTURE 1\n.Os\n.Sh NAME\ninclude-fixture\n",
    )
    .expect("write included source");
    let alias = root.join("alias-mdoc.1");
    fs::write(&alias, ".so minimal-mdoc.1\n").expect("write alias source");
    let cwd = std::env::current_dir().expect("current directory before parse");

    let document = parse_file(&alias, true).expect("resolve source-relative include");
    fs::remove_dir_all(root).expect("remove temporary manual tree");

    assert_eq!(document.macro_set, MacroSet::Mdoc);
    assert_eq!(document.metadata.title.as_deref(), Some("INCLUDE-FIXTURE"));
    assert_eq!(
        std::env::current_dir().expect("current directory after parse"),
        cwd
    );
}

#[test]
fn parser_only_expands_includes_when_policy_allows_a_root() {
    let base = std::env::temp_dir().join(format!(
        "libmandoc-rs-explicit-include-root-{}",
        process::id()
    ));
    let includes = base.join("includes");
    fs::create_dir_all(&includes).expect("create explicit include root");
    fs::write(
        includes.join("target.1"),
        ".TH EXPLICIT-ROOT 1\n.SH NAME\nexplicit-root \\- include fixture\n",
    )
    .expect("write included source");
    let alias = base.join("alias.1");
    fs::write(&alias, ".so target.1\n").expect("write alias source");

    let denied = Parser::default()
        .parse_file(&alias)
        .expect("parse alias without include expansion");
    let expanded = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(includes),
        compression: Compression::Auto,
    })
    .parse_file(&alias)
    .expect("resolve alias against explicit root");
    fs::remove_dir_all(base).expect("remove temporary manual tree");

    assert_ne!(
        denied.document.metadata.title.as_deref(),
        Some("EXPLICIT-ROOT")
    );
    assert_eq!(
        expanded.document.metadata.title.as_deref(),
        Some("EXPLICIT-ROOT")
    );
}

#[test]
fn explicit_root_resolves_compressed_includes_beside_the_source() {
    use std::io::Write;

    use flate2::{Compression as GzipCompression, write::GzEncoder};

    let root = std::env::temp_dir().join(format!(
        "libmandoc-rs-compressed-relative-include-{}",
        process::id()
    ));
    let man1 = root.join("man1");
    fs::create_dir_all(&man1).expect("create explicit manual section");
    let mut target = GzEncoder::new(Vec::new(), GzipCompression::fast());
    target
        .write_all(b".SH INCLUDED\ncompressed relative content\n")
        .expect("compress included source");
    fs::write(
        man1.join("target.1.gz"),
        target.finish().expect("finish included source"),
    )
    .expect("write compressed included source");
    let mut explicit = GzEncoder::new(Vec::new(), GzipCompression::fast());
    explicit
        .write_all(b".SH EXPLICIT\nexplicit compressed content\n")
        .expect("compress explicitly named include");
    fs::write(
        man1.join("explicit.1.gz"),
        explicit.finish().expect("finish explicit include"),
    )
    .expect("write explicitly named compressed include");
    let source = man1.join("source.1.gz");
    let mut source_bytes = GzEncoder::new(Vec::new(), GzipCompression::fast());
    source_bytes
        .write_all(
            b".TH SOURCE 1\n.SH NAME\nsource \\- include fixture\n.so target.1\n.so explicit.1.gz\n",
        )
        .expect("compress source manual");
    fs::write(
        &source,
        source_bytes.finish().expect("finish source manual"),
    )
    .expect("write source manual");

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(root.clone()),
        compression: Compression::Auto,
    })
    .parse_file(&source)
    .expect("resolve compressed include beside source");
    fs::remove_dir_all(root).expect("remove temporary manual tree");

    let mut visible = Vec::new();
    collect_visible_text(&report.document.root, &mut visible);
    assert!(visible.contains(&"compressed relative content"));
    assert!(visible.contains(&"explicit compressed content"));
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| { !diagnostic.message.contains(".so request failed") })
    );
}

#[test]
fn explicit_include_root_does_not_fall_back_to_process_cwd() {
    let identifier = format!("libmandoc-rs-ambient-{}", process::id());
    let cwd_target = std::env::current_dir()
        .expect("read test cwd")
        .join(format!("{identifier}.1"));
    fs::write(
        &cwd_target,
        ".TH AMBIENT 1\n.SH NAME\nambient \\- must not be included\n",
    )
    .expect("write ambient source");

    let base = std::env::temp_dir().join(format!("{identifier}-root"));
    fs::create_dir_all(&base).expect("create empty include root");
    let alias = base.join("alias.1");
    fs::write(&alias, format!(".so {identifier}.1\n")).expect("write alias source");

    let result = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(base.clone()),
        compression: Compression::Auto,
    })
    .parse_file(&alias);
    fs::remove_file(cwd_target).expect("remove ambient source");
    fs::remove_dir_all(base).expect("remove temporary manual tree");

    match result {
        Ok(report) => assert_ne!(report.document.metadata.title.as_deref(), Some("AMBIENT")),
        Err(error) => assert_eq!(error.kind, crate::ParseErrorKind::Parse),
    }
}

#[cfg(unix)]
#[test]
fn explicit_include_root_rejects_linked_target_files() {
    use std::os::unix::fs::symlink;

    let base = std::env::temp_dir().join(format!(
        "libmandoc-rs-linked-include-target-{}",
        process::id()
    ));
    let includes = base.join("includes");
    fs::create_dir_all(&includes).expect("create explicit include root");
    let outside = base.join("outside.1");
    fs::write(
        &outside,
        ".TH OUTSIDE 1\n.SH NAME\noutside \\- must not be included\n",
    )
    .expect("write outside target");
    symlink(&outside, includes.join("target.1")).expect("link target outside root");
    let alias = base.join("alias.1");
    fs::write(&alias, ".so target.1\n").expect("write alias source");

    let result = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(includes),
        compression: Compression::Auto,
    })
    .parse_file(&alias);
    fs::remove_dir_all(base).expect("remove temporary manual tree");

    match result {
        Ok(report) => assert_ne!(report.document.metadata.title.as_deref(), Some("OUTSIDE")),
        Err(error) => assert_eq!(error.kind, crate::ParseErrorKind::Parse),
    }
}

#[cfg(unix)]
#[test]
fn explicit_include_root_rejects_linked_intermediate_directories() {
    use std::os::unix::fs::symlink;

    let base = std::env::temp_dir().join(format!(
        "libmandoc-rs-linked-include-directory-{}",
        process::id()
    ));
    let includes = base.join("includes");
    let outside = base.join("outside");
    fs::create_dir_all(&includes).expect("create explicit include root");
    fs::create_dir_all(&outside).expect("create outside directory");
    fs::write(
        outside.join("target.1"),
        ".TH OUTSIDE-DIR 1\n.SH NAME\noutside-dir \\- must not be included\n",
    )
    .expect("write outside target");
    fs::write(outside.join("alias.1"), ".so target.1\n").expect("write alias source");
    symlink(&outside, includes.join("linked")).expect("link directory outside root");
    let alias = includes.join("linked/alias.1");

    let result = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(includes),
        compression: Compression::Auto,
    })
    .parse_file(&alias);
    fs::remove_dir_all(base).expect("remove temporary manual tree");

    match result {
        Ok(report) => assert_ne!(
            report.document.metadata.title.as_deref(),
            Some("OUTSIDE-DIR")
        ),
        Err(error) => assert_eq!(error.kind, crate::ParseErrorKind::Parse),
    }
}
