//! Regression test for bare same-directory `.so` redirect stubs.
//!
//! A stub such as fedora's `man1/lastb.1` containing `.so last.1` names its
//! target relative to its own `man#` directory rather than the manual
//! hierarchy root. The include resolver strips the `man#` component to honour
//! the more common `.so man1/foo.1` spelling, so the bare form only resolves
//! when the unstripped stub directory is also tried. `man(1)` follows both.

use std::{fs, process};

use libmandoc_rs::{Compression, IncludePolicy, Node, ParseOptions, Parser};

fn has_macro(node: &Node, name: &str) -> bool {
    node.macro_name.as_deref() == Some(name)
        || node.children.iter().any(|child| has_macro(child, name))
}

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
