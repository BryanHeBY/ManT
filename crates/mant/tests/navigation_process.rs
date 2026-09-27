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

fn link_for_record<'a>(references: &'a Value, record: &Value) -> &'a Value {
    let key = record["occurrence"].as_u64().expect("link occurrence key");
    let index = usize::try_from(key.checked_sub(1).expect("nonzero link key")).unwrap();
    let link = &references["contentProjection"]["contentStore"]["links"][index];
    assert_eq!(link["key"], record["occurrence"]);
    link
}

#[test]
#[cfg(feature = "annotated-preview")]
fn annotated_fixed_outline_projects_native_references_without_flow_store() {
    // Exact input ran pinned CVS -Tutf8 -O width=78 before this assertion.
    // term.c::term_word() emits both sourced labels in one section; the
    // marker's visible <> bytes stay outside each clickable occurrence.
    let path = std::env::temp_dir().join(format!("mant-fixed-references-{}.1", std::process::id()));
    fs::write(&path, b".TH T 1\n.SH SEE ALSO\na(1) \\%<> and b(2) \\%<>\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mant"))
        .args([
            "--annotated-preview",
            "--input",
            path.to_str().unwrap(),
            "--input-format",
            "roff",
            "--outline",
            "--outline-references=all",
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    let value = success(&output);
    let references = &value["references"];
    assert!(references.get("contentProjection").is_none());
    assert_eq!(references["records"], json!([]));
    assert_eq!(references["occurrences"], json!({"kind":"exact","value":2}));
    assert_eq!(references["fixedRecords"][0]["labelPreview"], "a(1)");
    assert_eq!(references["fixedRecords"][1]["labelPreview"], "b(2)");
    assert_eq!(references["fixedRecords"][0]["target"]["kind"], "manual");
    assert_eq!(
        references["fixedRecords"][0]["sourceRead"],
        json!({"kind":"id","id":"see-also"})
    );
    assert_eq!(references["fixedRecords"][0]["origin"]["link"], 1);
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
    assert_eq!(
        link_for_record(&all["references"], record)["target"]["fragment"],
        "Mixed.Target"
    );
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
    let later_record = &later["references"]["records"][0];
    assert_eq!(later_record["labelPreview"], "run");
    assert_eq!(later_record["labelPreviewTruncated"], false);
    assert_eq!(
        link_for_record(&later["references"], later_record)["target"]["fragment"],
        "Mixed.Target"
    );
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
