//! Windows path identity, containment and reparse-point policy.

use super::*;
use windows_sys::Win32::Foundation::ERROR_PRIVILEGE_NOT_HELD;

#[cfg(windows)]
#[test]
fn windows_relative_source_paths_resolve_beside_a_relative_root() {
    let root = std::path::PathBuf::from("target").join(format!(
        "libmandoc-rs-relative-windows-root-{}",
        process::id()
    ));
    let section = root.join("man1");
    fs::create_dir_all(&section).expect("create relative Windows root");
    fs::write(
        section.join("target.1"),
        ".TH RELATIVE-WINDOWS-ROOT 1\n.SH NAME\nrelative-root \\- included\n",
    )
    .expect("write relative Windows include target");
    let alias = section.join("alias.1");
    fs::write(&alias, ".so target.1\n").expect("write relative Windows alias");

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(root.clone()),
        compression: Compression::Plain,
    })
    .parse_file(&alias)
    .expect("resolve beside a relative source below a relative root");
    fs::remove_dir_all(root).expect("remove relative Windows root");

    assert_eq!(
        report.document.metadata.title.as_deref(),
        Some("RELATIVE-WINDOWS-ROOT")
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains(".so request failed"))
    );
}

#[cfg(windows)]
#[test]
fn windows_source_paths_resolve_beside_a_differently_cased_root() {
    let root =
        std::env::temp_dir().join(format!("libmandoc-rs-cased-windows-root-{}", process::id()));
    let section = root.join("man1");
    fs::create_dir_all(&section).expect("create cased Windows root");
    fs::write(
        section.join("target.1"),
        ".TH CASED-WINDOWS-ROOT 1\n.SH NAME\ncased-root \\- included\n",
    )
    .expect("write cased include target");
    let alias = section.join("alias.1");
    fs::write(&alias, ".so ./target.1\n").expect("write cased alias");
    let differently_cased_root = std::path::PathBuf::from(
        root.to_string_lossy()
            .chars()
            .map(|character| {
                if character.is_ascii_lowercase() {
                    character.to_ascii_uppercase()
                } else {
                    character.to_ascii_lowercase()
                }
            })
            .collect::<String>(),
    );

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(differently_cased_root),
        compression: Compression::Plain,
    })
    .parse_file(&alias)
    .expect("resolve beside a source through a differently cased root");
    fs::remove_dir_all(root).expect("remove cased Windows root");

    assert_eq!(
        report.document.metadata.title.as_deref(),
        Some("CASED-WINDOWS-ROOT"),
        "diagnostics: {:#?}",
        report.diagnostics
    );
}

#[cfg(windows)]
#[test]
fn explicit_include_root_rejects_windows_reparse_targets() {
    use std::os::windows::fs::symlink_file;

    let base = std::env::temp_dir().join(format!(
        "libmandoc-rs-windows-linked-include-target-{}",
        process::id()
    ));
    let includes = base.join("includes");
    fs::create_dir_all(&includes).expect("create explicit include root");
    let target = includes.join("real.1");
    fs::write(
        &target,
        ".TH REPARSE-TARGET 1\n.SH NAME\nreparse-target \\- must not be included\n",
    )
    .expect("write in-root target");
    if let Err(error) = symlink_file(&target, includes.join("target.1")) {
        let privilege_not_held = error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok())
            == Some(ERROR_PRIVILEGE_NOT_HELD);
        if error.kind() == std::io::ErrorKind::PermissionDenied || privilege_not_held {
            fs::remove_dir_all(base).expect("remove skipped reparse fixture");
            return;
        }
        panic!("create Windows file link: {error}");
    }
    let alias = base.join("alias.1");
    fs::write(&alias, ".so target.1\n").expect("write alias source");

    let result = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(includes),
        compression: Compression::Auto,
    })
    .parse_file(&alias);
    fs::remove_dir_all(base).expect("remove temporary manual tree");

    match result {
        Ok(report) => {
            assert_ne!(
                report.document.metadata.title.as_deref(),
                Some("REPARSE-TARGET")
            );
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains(".so request failed"))
            );
        }
        Err(error) => assert_eq!(error.kind, crate::ParseErrorKind::Parse),
    }
}

#[cfg(windows)]
#[test]
fn explicit_include_root_rejects_windows_path_namespaces() {
    let root = std::env::temp_dir().join(format!(
        "libmandoc-rs-windows-path-namespace-{}",
        process::id()
    ));
    fs::create_dir_all(&root).expect("create explicit include root");
    for target in [
        "C:/outside.1",
        "target.1:stream",
        r"\\server\share\outside.1",
    ] {
        let report = Parser::new(ParseOptions {
            includes: IncludePolicy::Root(root.clone()),
            compression: Compression::Plain,
        })
        .parse_bytes("alias.1", format!(".so {target}\n").as_bytes())
        .expect("return a finite document for a denied include");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(".so request failed")),
            "denied Windows namespace must remain observable: {target}"
        );
    }
    fs::remove_dir_all(root).expect("remove explicit include root");
}

#[cfg(windows)]
#[test]
fn windows_explicit_root_supports_unicode_paths_and_concurrent_sessions() {
    const WORKERS: usize = 8;

    let base =
        std::env::temp_dir().join(format!("libmandoc-rs-windows-root-日本-{}", process::id()));
    let roots = (0..WORKERS)
        .map(|worker| {
            let root = base.join(format!("文档-{worker}"));
            let section = root.join("章节");
            fs::create_dir_all(&section).expect("create Unicode include root");
            fs::write(
                section.join("target.1"),
                format!(".TH WINDOWS-ROOT-{worker} 1\n.SH NAME\nroot-{worker} \\- isolated\n"),
            )
            .expect("write isolated include target");
            (root, section.join("alias.1"))
        })
        .collect::<Vec<_>>();
    let start = Arc::new(Barrier::new(WORKERS));
    let workers = roots
        .into_iter()
        .enumerate()
        .map(|(worker, (root, alias))| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..100 {
                    let report = Parser::new(ParseOptions {
                        includes: IncludePolicy::Root(root.clone()),
                        compression: Compression::Plain,
                    })
                    .parse_bytes(&alias, b".so target.1\n")
                    .expect("resolve isolated Windows root");
                    assert_eq!(
                        report.document.metadata.title.as_deref(),
                        Some(format!("WINDOWS-ROOT-{worker}").as_str())
                    );
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().expect("root resolver worker must not panic");
    }
    fs::remove_dir_all(base).expect("remove concurrent Windows roots");
}
