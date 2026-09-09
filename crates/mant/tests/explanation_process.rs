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

#[test]
#[cfg(feature = "roff")]
fn cli_file_stdin_and_public_production_api_agree_on_executed_boundaries() {
    use std::{io::Write, process::Stdio};
    let root = fixture("executed-boundaries");
    let path = root.join("probe.1");
    for between in [
        ".PP\n",
        ".if 1 .PP\n",
        ".if 0 \\{\\\n.PP\n.\\}\n",
        ".BREAK\n",
        ".de UNUSED\n.PP\n..\n",
    ] {
        let source = format!(
            ".TH PROBE 1\n.de BREAK\n.PP\n..\n.SH OPTIONS\n.TP\n.B -a\n{between}.TP\n.B -b\nShared body.\n"
        );
        fs::write(&path, &source).unwrap();
        let file = success(
            &Command::new(env!("CARGO_BIN_EXE_mant"))
                .args([
                    "--input",
                    path.to_str().unwrap(),
                    "--input-format",
                    "roff",
                    "--explain=-a",
                    "--format",
                    "json",
                    "--compact",
                ])
                .output()
                .unwrap(),
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_mant"))
            .args([
                "--input",
                "-",
                "--input-format",
                "roff",
                "--explain=-a",
                "--format",
                "json",
                "--compact",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(source.as_bytes())
            .unwrap();
        let stdin = success(&child.wait_with_output().unwrap());
        let document = mant_loader::parse_manual_source(&path).unwrap();
        let api = mant_query::explain_query(
            &mant_ir::ResolvedContent {
                label: "probe".into(),
                address: None,
                document: Some(document),
                tldr: None,
            },
            &mant_protocol::ExplanationQuery {
                entry: "-a".into(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap();
        let api = serde_json::to_value(api).unwrap();
        for field in ["evidence", "supports", "counts", "truncation"] {
            assert_eq!(file[field], stdin[field], "{field} {between:?}");
            assert_eq!(file[field], api[field], "API {field} {between:?}");
        }
    }
    fs::remove_dir_all(root).unwrap();
}

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
    assert!(evidence[3].get("entry").is_none());
    assert_eq!(evidence[3]["bases"], json!([{"kind":"literal"}]));
    assert_eq!(evidence[0]["outline"]["node"]["id"], "brief");
    assert_eq!(evidence[1]["outline"]["node"]["id"], "class-help");
    assert_eq!(evidence[2]["outline"]["node"]["id"], "assist");
    assert!(
        evidence[2]["bases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|basis| basis["kind"] == "related" && basis["from"] == "brief")
    );
    let strict = run(&root, &["first", "--node=--help"]);
    assert_eq!(strict.status.code(), Some(2));
    for id in ["brief", "class-help", "assist"] {
        let read = success(&run(&root, &["first", &format!("--node=id:{id}")]));
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
    for doc in documents {
        assert!(doc.get("explanation").is_none());
        assert!(doc.get("evidence").is_none());
        assert_eq!(doc["returned"], 1);
    }
    for (record, (ordinal, document_index)) in result["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .zip([(3, 1), (4, 0)])
    {
        assert_eq!(record["documentIndex"], document_index);
        assert_eq!(record["evidence"]["ordinal"], ordinal);
        assert_eq!(record["evidence"]["contentOmitted"], true);
        assert!(record["evidence"].get("content").is_none());
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

#[test]
fn request_json_and_cli_share_classification_and_original_rendering() {
    use std::io::Write;
    use std::process::Stdio;
    let root = fixture("request");
    let direct = success(&run(&root, &["first", "--explain=--help"]));
    let mut command = Command::new(env!("CARGO_BIN_EXE_mant"));
    support::configure_registered_documents(&mut command, &root);
    let mut child = command
        .args(["--request-json", "--format=json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request = json!({"schema":"mant.request/v0.11","input":{"kind":"document","selector":"first"},"view":{"kind":"explain","entry":"--help"}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let response = success(&child.wait_with_output().unwrap());
    assert_eq!(response, direct);
    assert_eq!(response["order"], "class-then-source");
    assert_eq!(
        response["counts"],
        json!({"directEntry":{"total":2,"returned":2},"relatedEntry":{"total":1,"returned":1},"entryMention":{"total":0,"returned":0},"contextMention":{"total":1,"returned":1}})
    );
    for format in ["text", "markdown"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mant"));
        support::configure_registered_documents(&mut command, &root);
        let output = command
            .args([
                "first",
                "--explain=--help",
                "--format",
                format,
                "--color=never",
                "--display=direct",
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        let positions = [
            "Direct entries",
            "Explicitly related entries",
            "Mentions in ordinary content",
        ]
        .map(|heading| {
            text.find(&if format == "markdown" {
                format!("## {heading}\n")
            } else {
                format!("========== {heading} ==========")
            })
            .unwrap()
        });
        assert!(positions[0] < positions[1] && positions[1] < positions[2]);
        assert!(text.contains("Class-specific help") || text.contains("Class\\-specific help"));
        assert!(!text.contains('\u{1b}'));
    }
    fs::remove_dir_all(root).unwrap();
}
