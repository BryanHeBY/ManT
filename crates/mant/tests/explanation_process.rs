//! Independent public-contract oracle: evidence is not unique navigation.
mod support;

use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output},
};

const SOURCE: &str = r#"# Evidence

Use `--help` for assistance.

<!-- mant:entries role=option case=sensitive -->
- `-h`, `--help`: Brief usage. <!-- mant:entry {"id":"brief","aliasGroups":[["-h","--help"]]} -->
- `--help=CLASS`: Class-specific help. <!-- mant:entry {"id":"class-help"} -->
- `--assist`: Independent examples. <!-- mant:entry {"id":"assist","aliasOf":"brief"} -->
"#;

fn fixture(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("mant-explanation-{name}-{}", std::process::id()));
    let documents = support::registered_documents_dir(&root);
    fs::create_dir_all(&documents).unwrap();
    fs::write(documents.join("first.md"), SOURCE).unwrap();
    fs::write(documents.join("second.md"), SOURCE).unwrap();
    root
}

fn run(root: &std::path::Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mant"));
    support::configure_registered_documents(&mut command, root);
    command
        .args(args)
        .args(["--format", "json", "--compact"])
        .output()
        .unwrap()
}

fn success(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn multiple_owners_relations_and_strict_navigation_remain_independent() {
    let root = fixture("owners");
    let result = success(&run(&root, &["first", "--explain=--help"]));
    assert_eq!(result["schema"], "mant.explanation/v0.11");
    assert_eq!(result["total"], 4);
    let evidence = result["evidence"].as_array().unwrap();
    assert!(evidence[0].get("entry").is_none());
    assert_eq!(evidence[0]["bases"], json!([{"kind":"literal"}]));
    assert_eq!(evidence[1]["outline"]["node"]["id"], "brief");
    assert_eq!(evidence[2]["outline"]["node"]["id"], "class-help");
    assert_eq!(evidence[3]["outline"]["node"]["id"], "assist");
    assert!(
        evidence[3]["bases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|basis| basis["kind"] == "related" && basis["from"] == "brief")
    );
    let strict = run(&root, &["first", "--node=--help"]);
    assert_eq!(strict.status.code(), Some(2));
    for id in ["brief", "class-help", "assist"] {
        let read = success(&run(&root, &["first", &format!("--node={id}")]));
        assert_eq!(read["schema"], "mant.excerpt/v0.11");
        assert_eq!(read["selections"].as_array().unwrap().len(), 1);
    }
    let outline = success(&run(
        &root,
        &["first", "--outline", "--outline-entries=all"],
    ));
    assert_eq!(
        outline["nodes"][0]["children"][0]["aliasGroups"],
        json!([["-h", "--help"]])
    );
    assert_eq!(outline["nodes"][0]["children"][2]["aliasOf"], "brief");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn semantic_paging_and_copy_budget_are_global_across_readable_sources() {
    let root = fixture("paging");
    let response = success(&run(
        &root,
        &[
            "--document=first",
            "--document=second",
            "--explain=--help",
            "--offset=3",
            "--limit=2",
            "--explain-content-bytes=1",
        ],
    ));
    let result = &response["result"]["explanation"];
    assert_eq!(result["outcome"], "evidence");
    assert_eq!(result["total"], 8);
    assert_eq!(result["returned"], 2);
    assert_eq!(result["nextOffset"], 5);
    assert_eq!(result["truncation"]["content"], true);
    assert_eq!(result["failures"], json!([]));
    let documents = result["documents"].as_array().unwrap();
    assert_eq!(documents.len(), 2);
    for (doc, ordinal) in documents.iter().zip([3, 4]) {
        let items = doc["explanation"]["evidence"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["ordinal"], ordinal);
        assert_eq!(items[0]["contentOmitted"], true);
        assert!(items[0].get("content").is_none());
    }
    let empty_page = success(&run(&root, &["first", "--explain=--help", "--offset=100"]));
    assert_eq!(empty_page["outcome"], "evidence");
    assert_eq!(empty_page["returned"], 0);
    assert!(empty_page.get("nextOffset").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn no_evidence_partial_sources_and_invalid_requests_have_distinct_outcomes() {
    let root = fixture("coverage");
    let none = success(&run(&root, &["first", "--explain=not-documented"]));
    assert_eq!(none["outcome"], "no-evidence");
    assert_eq!(none["evidence"], json!([]));
    let partial = success(&run(
        &root,
        &[
            "--document=first",
            "--document=documents/absent",
            "--explain=--help",
        ],
    ));
    assert_eq!(partial["result"]["explanation"]["outcome"], "evidence");
    assert_eq!(partial["scope"]["unresolved"].as_array().unwrap().len(), 1);
    let failed = run(&root, &["--document=documents/absent", "--explain=--help"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stdout.is_empty());
    for invalid in [
        "--limit=0",
        "--limit=257",
        "--explain-content-bytes=0",
        "--explain-content-bytes=4194305",
    ] {
        for scope in ["first", "--document=first"] {
            let output = run(&root, &[scope, "--explain=--help", invalid]);
            assert_eq!(output.status.code(), Some(2), "{invalid}: {output:?}");
            assert!(output.stdout.is_empty());
        }
    }
    fs::remove_dir_all(root).unwrap();
}
