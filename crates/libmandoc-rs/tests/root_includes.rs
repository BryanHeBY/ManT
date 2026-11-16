#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use libmandoc_rs::{Compression, IncludePolicy, Node, ParseOptions, ParseReport, Parser};

const SIBLING: &str = ".TH PROBE 1\n.SH DESCRIPTION\nbefore\n.so target.1\nafter\n";

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = PathBuf::from(format!("mant-{label}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn parse_both(root: &Path, source_path: &Path, source: &str) -> [ParseReport; 2] {
    let parser = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(root.to_owned()),
        compression: Compression::Plain,
    });
    [
        parser.parse_file(source_path).unwrap(),
        parser.parse_bytes(source_path, source.as_bytes()).unwrap(),
    ]
}

fn visible<'a>(node: &'a Node, text: &mut Vec<&'a str>) {
    if let Some(value) = &node.text {
        text.push(value);
    }
    for child in &node.children {
        visible(child, text);
    }
}

#[test]
fn explicit_roots_accept_equivalent_caller_paths() {
    let dir = Scratch::new("root-spellings");
    let root = dir.0.join("tree");
    let man = root.join("man1");
    fs::create_dir_all(&man).unwrap();
    fs::write(man.join("target.1"), "TARGETWORD\n").unwrap();
    let path = man.join("probe.1");
    let cwd = std::env::current_dir().unwrap();
    let absolute_root = cwd.join(&root);
    let absolute_path = cwd.join(&path);
    let redundant_path = root.join("man1/.//probe.1");

    // read.c dispatches both .so spellings through mparse_open(). The
    // pinned reader resolves the bare sibling from man1 and the qualified
    // request from the tree root, with the outer document on both sides.
    let qualified = SIBLING.replace(".so target.1", ".so man1/target.1");
    for source in [SIBLING, &qualified] {
        fs::write(&path, source).unwrap();
        for root in [
            root.clone(),
            absolute_root.clone(),
            PathBuf::from(format!("{}/", root.display())),
            absolute_root.join("."),
        ] {
            for path in [&path, &absolute_path, &redundant_path] {
                for report in parse_both(&root, path, source) {
                    let mut text = Vec::new();
                    visible(&report.document.root, &mut text);
                    assert_eq!(
                        text,
                        ["DESCRIPTION", "before", "TARGETWORD", "after"],
                        "{root:?} {path:?} {source}"
                    );
                    assert!(
                        report
                            .diagnostics
                            .iter()
                            .all(|finding| { !finding.message.contains(".so request failed") })
                    );
                }
            }
        }
    }
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}

#[test]
fn explicit_roots_only_follow_a_link_at_the_approved_boundary() {
    let dir = Scratch::new("root-containment");
    let cwd = std::env::current_dir().unwrap();
    let root = dir.0.join("tree");
    let sibling = dir.0.join("tree-other/man1");
    fs::create_dir(&root).unwrap();
    fs::create_dir_all(&sibling).unwrap();
    fs::write(sibling.join("target.1"), "TARGETWORD\n.so more.1\n").unwrap();
    fs::write(sibling.join("more.1"), "INNERWORD\n").unwrap();
    fs::write(sibling.join("probe.1"), SIBLING).unwrap();
    symlink(cwd.join(&sibling), root.join("linked")).unwrap();
    let approved = dir.0.join("approved");
    symlink(cwd.join(sibling.parent().unwrap()), &approved).unwrap();

    for report in parse_both(&approved, &cwd.join(approved.join("man1/probe.1")), SIBLING) {
        let mut text = Vec::new();
        visible(&report.document.root, &mut text);
        assert_eq!(
            text,
            ["DESCRIPTION", "before", "TARGETWORD", "INNERWORD", "after"]
        );
    }
    for path in [sibling.join("probe.1"), root.join("linked/probe.1")] {
        for report in parse_both(&cwd.join(&root), &path, SIBLING) {
            let mut text = Vec::new();
            visible(&report.document.root, &mut text);
            assert_eq!(text.get(1), Some(&"before"), "{text:?}");
            assert_eq!(text.last(), Some(&"after"), "{text:?}");
            assert!(!text.contains(&"TARGETWORD"));
            assert!(report.diagnostics.iter().any(|finding| {
                finding.message.contains(".so request failed")
                    && finding.message.contains("target.1")
            }));
        }
    }
}
