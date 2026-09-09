//! Process boundary for strict source reads and independent link discovery.
mod support;

use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const SOURCE: &str = "# [Index](absent.md#Mixed.Target)\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- [`run`](absent.md#Mixed.Target): Source body, not remote content.\n\n[Second](second.md) and [web](https://example.test).\n";

fn run(root: &Path, args: &[&str], format: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mant"));
    support::configure_registered_documents(&mut command, root);
    command
        .args(["links"])
        .args(args)
        .args(["--format", format, "--color", "never"])
        .output()
        .unwrap()
}

fn success(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn outline_reference_paging_and_strict_reads_share_one_source_snapshot() {
    let root = std::env::temp_dir().join(format!("mant-navigation-process-{}", std::process::id()));
    let documents = support::registered_documents_dir(&root);
    fs::create_dir_all(&documents).unwrap();
    fs::write(documents.join("links.md"), SOURCE).unwrap();
    let summary = success(&run(
        &root,
        &["--outline", "--outline-entries=none"],
        "json",
    ));
    assert_eq!(
        summary["references"]["occurrences"],
        json!({"kind":"exact", "value":3})
    );
    assert_eq!(
        summary["references"]["targets"],
        json!({"kind":"exact", "value":2})
    );
    assert_eq!(summary["references"]["records"], json!([]));
    let all = success(&run(
        &root,
        &[
            "--outline",
            "--outline-references=all",
            "--reference-limit=1",
        ],
        "json",
    ));
    let record = &all["references"]["records"][0];
    assert_eq!(record["target"]["fragment"], "Mixed.Target");
    assert_eq!(record["resolution"]["kind"], "logical-address");
    assert_eq!(record["resolution"]["fragment"]["kind"], "unchecked");
    assert_eq!(record["sourceRead"], json!({"kind":"path", "path":"root"}));
    assert_eq!(all["references"]["page"]["nextOffset"], 1);
    let later = success(&run(
        &root,
        &[
            "--outline",
            "--outline-references=all",
            "--reference-offset=1",
            "--reference-limit=1",
        ],
        "json",
    ));
    assert_eq!(later["references"]["records"][0]["label"], "run");
    assert_ne!(
        record["origin"],
        later["references"]["records"][0]["origin"]
    );
    let read = success(&run(&root, &["--node=1/e1"], "json"));
    assert!(read.to_string().contains("Source body, not remote content"));
    for selector in ["run", "--run", "https://example.test", "#Mixed.Target"] {
        let output = run(&root, &[&format!("--node={selector}")], "json");
        assert!(!output.status.success(), "accepted {selector}");
        assert!(output.stdout.is_empty());
    }
    let text = run(&root, &["--outline", "--outline-references=all"], "text");
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("absent#Mixed.Target"), "{text}");
    assert!(text.contains("readSource=path:root"), "{text}");
    assert!(text.contains("document not loaded"), "{text}");
    let none = run(&root, &["--outline", "--outline-references=none"], "text");
    assert!(
        !String::from_utf8(none.stdout)
            .unwrap()
            .contains("References:")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_reference_policy_is_rejected_before_discovery() {
    for arguments in [
        vec!["--outline", "--reference-types=document,document"],
        vec!["--outline", "--reference-limit=1001"],
        vec!["--outline", "--reference-limit=0"],
        vec!["--outline", "--reference-types=unknown"],
        vec!["--reference-offset=1"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mant"))
            .arg("nonexistent-navigation-fixture")
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("was not found"));
    }
}
