//! Regression test for bare same-directory `.so` redirect stubs.
//!
//! A stub such as fedora's `man1/lastb.1` containing `.so last.1` names its
//! target relative to its own `man#` directory rather than the manual
//! hierarchy root. The include resolver strips the `man#` component to honour
//! the more common `.so man1/foo.1` spelling, so the bare form only resolves
//! when the unstripped stub directory is also tried. `man(1)` follows both.

use std::{fs, process};

use libmandoc_rs::{Compression, IncludePolicy, ParseOptions, Parser};

#[cfg(unix)]
use libmandoc_rs::Node;

#[cfg(unix)]
fn has_macro(node: &Node, name: &str) -> bool {
    node.macro_name.as_deref() == Some(name)
        || node.children.iter().any(|child| has_macro(child, name))
}

#[cfg(unix)]
fn visible_text(node: &Node, output: &mut String) {
    if let Some(value) = &node.text {
        output.push_str(value);
        output.push(' ');
    }
    for child in &node.children {
        visible_text(child, output);
    }
}

#[cfg(unix)]
#[test]
fn deny_policy_rejects_embedded_includes_without_a_cwd_fallback() {
    let identifier = format!("libmandoc-rs-denied-so-{}", process::id());
    let cwd = std::env::current_dir().expect("read test working directory");
    let secret_dir = cwd.join(&identifier);
    fs::create_dir_all(&secret_dir).expect("create denied include directory");
    fs::write(secret_dir.join("secret.1"), "PRIVATE-INCLUDE-MARKER\n")
        .expect("write denied include target");

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes(
        "embedded.1",
        format!(".TH EMBEDDED 1\n.SH BODY\nbefore\n.so {identifier}/secret.1\n.\\\" trailing\n")
            .as_bytes(),
    )
    .expect("retain the surrounding manual after denying an include");
    fs::remove_dir_all(&secret_dir).expect("remove denied include directory");

    let mut text = String::new();
    visible_text(&report.document.root, &mut text);
    assert!(text.contains("before"));
    assert!(!text.contains("PRIVATE-INCLUDE-MARKER"));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("secret.1")),
        "the denied include must remain observable: {:?}",
        report.diagnostics
    );
}

#[cfg(unix)]
#[test]
fn deny_policy_still_reads_the_caller_selected_top_level_file() {
    let root = std::env::temp_dir().join(format!("libmandoc-rs-deny-top-{}", process::id()));
    fs::create_dir_all(&root).expect("create top-level source directory");
    let source = root.join("top.1");
    fs::write(&source, ".TH TOP 1\n.SH NAME\ntop \\- retained\n").expect("write top-level source");

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Auto,
    })
    .parse_file(&source)
    .expect("read the explicitly selected top-level source");
    fs::remove_dir_all(&root).expect("remove top-level source directory");

    assert_eq!(report.document.metadata.title.as_deref(), Some("TOP"));
}

#[cfg(unix)]
#[test]
fn deny_policy_does_not_reopen_the_top_level_file_as_an_include() {
    let identifier = format!("libmandoc-rs-deny-reopen-{}", process::id());
    fs::create_dir_all(&identifier).expect("create relative source directory");
    let source = std::path::Path::new(&identifier).join("source.1");
    fs::write(
        &source,
        format!(".TH REOPEN 1\n.SH BODY\nretained\n.so {identifier}/source.1\n.\\\" trailing\n"),
    )
    .expect("write self-including top-level source");

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Auto,
    })
    .parse_file(&source)
    .expect("read the top-level source once and deny its include");
    fs::remove_dir_all(&identifier).expect("remove relative source directory");

    assert_eq!(report.document.metadata.title.as_deref(), Some("REOPEN"));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("source.1"))
    );
}

#[cfg(unix)]
#[test]
fn resolves_bare_same_directory_so_target_inside_a_man_section() {
    let root = std::env::temp_dir().join(format!("libmandoc-rs-bare-so-{}", process::id()));
    let man1 = root.join("man1");
    fs::create_dir_all(&man1).expect("create temporary manual tree");
    fs::write(
        man1.join("target.1"),
        ".TH TARGET 1\n.SH NAME\ntarget \\- redirect destination\n",
    )
    .expect("write included source");
    let alias = man1.join("alias.1");
    fs::write(&alias, ".so target.1\n").expect("write alias source");

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::SourceTree,
        compression: Compression::Auto,
    })
    .parse_file(&alias)
    .expect("resolve bare same-directory include");
    fs::remove_dir_all(&root).expect("remove temporary manual tree");

    assert_eq!(
        report.document.metadata.title.as_deref(),
        Some("TARGET"),
        "the redirect target's metadata must replace the stub's",
    );
    assert!(
        has_macro(&report.document.root, "SH"),
        "the redirect target's sections must be inlined",
    );
}

#[test]
fn explicit_root_keeps_bare_redirects_inside_the_source_section() {
    let root =
        std::env::temp_dir().join(format!("libmandoc-rs-explicit-bare-so-{}", process::id()));
    let man1 = root.join("man1");
    fs::create_dir_all(&man1).expect("create temporary manual tree");
    fs::write(
        man1.join("target.1"),
        ".TH EXPLICIT-TARGET 1\n.SH NAME\ntarget \\- redirect destination\n",
    )
    .expect("write included source");
    let alias = man1.join("alias.1");
    fs::write(&alias, ".so target.1\n").expect("write alias source");

    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(root.clone()),
        compression: Compression::Plain,
    })
    .parse_bytes(&alias, b".so target.1\n")
    .expect("resolve bare include without a cwd fallback");
    fs::remove_dir_all(&root).expect("remove temporary manual tree");

    assert_eq!(
        report.document.metadata.title.as_deref(),
        Some("EXPLICIT-TARGET")
    );
}

#[test]
fn explicit_root_rejects_parent_directory_redirects() {
    let base =
        std::env::temp_dir().join(format!("libmandoc-rs-parent-escape-so-{}", process::id()));
    let root = base.join("approved");
    let man1 = root.join("man1");
    fs::create_dir_all(&man1).expect("create temporary manual tree");
    fs::write(
        base.join("outside.1"),
        ".TH OUTSIDE 1\n.SH NAME\noutside \\- must not be included\n",
    )
    .expect("write outside source");
    let alias = man1.join("alias.1");

    let result = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(root),
        compression: Compression::Plain,
    })
    .parse_bytes(&alias, b".so ../../outside.1\n");
    fs::remove_dir_all(&base).expect("remove temporary manual tree");

    if let Ok(report) = result {
        assert_ne!(
            report.document.metadata.title.as_deref(),
            Some("OUTSIDE"),
            "an explicit root must not permit lexical parent traversal"
        );
    }
}

#[test]
fn explicit_root_rejects_an_empty_directory() {
    let error = Parser::new(ParseOptions {
        includes: IncludePolicy::Root("".into()),
        compression: Compression::Plain,
    })
    .parse_bytes("alias.1", b".so target.1\n")
    .expect_err("an empty include root must not mean the filesystem root");

    assert_eq!(error.message, "manual include root is empty");
}
