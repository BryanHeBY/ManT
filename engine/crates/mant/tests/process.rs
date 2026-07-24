//! Black-box checks for the executable's stdout, stderr, and exit-code contract.

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn executable() -> &'static str {
    env!("CARGO_BIN_EXE_mant")
}

#[test]
fn help_groups_the_public_query_surface() {
    let output = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("mant <TOPIC|MARKDOWN|-> [OPTIONS]"));
    assert!(help.contains("mant README.md"));
    assert!(help.contains("cat guide.md | mant -"));
    assert!(help.contains("Document selection:"));
    assert!(help.contains("Search:"));
    assert!(help.contains("Integration:"));
    assert!(help.contains("-h, --help"));
    assert!(help.contains("--format <FORMAT>"));
    assert!(help.contains("--preserve-anchors"));
    assert!(help.contains("--update-tldr"));
    assert!(help.contains("--protocol-version"));
    assert!(help.contains("--schema <CONTRACT>"));
    assert!(help.contains("--mcp"));
    assert!(help.contains("--explain <ENTRY>"));
    assert!(help.contains("--search <PATTERN>"));
    assert!(!help.contains("--json"));
    assert!(!help.contains("update tldr"));
}

#[test]
fn short_help_alias_matches_long_help() {
    let short = Command::new(executable())
        .arg("-h")
        .output()
        .expect("run mant -h");
    let long = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run mant --help");

    assert!(short.status.success());
    assert!(short.stderr.is_empty());
    assert_eq!(short.stdout, long.stdout);
    assert!(long.status.success());
    assert!(long.stderr.is_empty());
}

#[test]
fn request_schema_is_discoverable_without_host_state() {
    let output = Command::new(executable())
        .args(["--schema", "request", "--compact"])
        .output()
        .expect("run mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("request schema");
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(value["additionalProperties"], false);
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 schema")
            .contains("mant.request/v3")
    );
}

#[test]
fn protocol_version_is_a_clean_json_document() {
    let output = Command::new(executable())
        .args(["--protocol-version", "--compact"])
        .output()
        .expect("run mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("protocol JSON");
    assert_eq!(value["protocol"], "mant.cli/v3");
    assert_eq!(value["requestSchema"], "mant.request/v3");
    assert_eq!(value["querySchema"], "mant.query/v3");
    assert_eq!(value["outlineSchema"], "mant.outline/v3");
    assert_eq!(value["excerptSchema"], "mant.excerpt/v3");
    assert_eq!(value["searchSchema"], "mant.search/v2");
}

#[test]
fn invalid_stdin_request_uses_status_two_without_runtime_noise() {
    let mut child = Command::new(executable())
        .args(["--request-json", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"schema":"mant.request/v3","input":{"kind":"manual","topic":"git"},"view":{"kind":"full"},"futureField":true}"#,
        )
        .expect("write request");
    let output = child.wait_with_output().expect("wait for mant");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.starts_with("mant: invalid query request JSON:"));
    assert!(!diagnostic.contains("panicked at"));
    assert!(!diagnostic.contains("stack backtrace"));
}

#[test]
fn direct_stdin_reads_markdown_without_extending_the_request_schema() {
    let mut child = Command::new(executable())
        .args(["-", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"# Piped\n\n## Options\n\n- `--help`: Show help.\n")
        .expect("write Markdown");
    let output = child.wait_with_output().expect("wait for mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("query JSON");
    assert_eq!(value["label"], "stdin");
    assert_eq!(value["document"]["source"]["format"], "markdown");
    assert!(value["document"]["source"].get("path").is_none());
    assert!(value.get("tldr").is_none());
    assert_eq!(
        value["document"]["sections"][1]["blocks"][0]["items"][0]["identity"]["names"][0],
        "--help"
    );
}

#[test]
fn direct_and_protocol_queries_read_local_markdown_files_by_path() {
    let path = markdown_fixture_path();
    fs::write(&path, "# Local\n\nBody.\n").expect("write Markdown fixture");

    let direct = Command::new(executable())
        .args([
            path.to_str().expect("UTF-8 path"),
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("query Markdown file");
    assert!(direct.status.success());
    assert!(direct.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&direct.stdout).expect("query JSON");
    assert_eq!(value["document"]["meta"]["title"], "Local");
    assert_eq!(
        value["document"]["source"]["path"],
        path.to_str().expect("UTF-8 path")
    );
    assert!(value.get("tldr").is_none());

    let mut child = Command::new(executable())
        .args(["--request-json", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start protocol query");
    let request = serde_json::json!({
        "schema": "mant.request/v3",
        "input": {
            "kind": "markdown-file",
            "path": path.to_str().expect("UTF-8 path"),
        },
        "view": { "kind": "full" },
    });
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(request.to_string().as_bytes())
        .expect("write request");
    let protocol = child.wait_with_output().expect("wait for protocol query");
    let _ = fs::remove_file(&path);

    assert!(protocol.status.success());
    assert!(protocol.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&protocol.stdout).expect("query JSON");
    assert_eq!(
        value["label"].as_str(),
        Some(
            path.file_name()
                .expect("filename")
                .to_str()
                .expect("UTF-8 filename")
        )
    );
    assert_eq!(value["document"]["source"]["format"], "markdown");
}

#[test]
fn markdown_root_content_is_discoverable_selectable_and_searchable() {
    let path = std::env::temp_dir().join(format!(
        "mant-markdown-root-process-{}.md",
        std::process::id()
    ));
    fs::write(
        &path,
        "Read the preface needle first.\n\n# Guide\n\nSection body.\n",
    )
    .expect("write Markdown fixture");
    let path = path.to_str().expect("UTF-8 path");

    let run_json = |arguments: &[&str]| {
        let output = Command::new(executable())
            .args(arguments)
            .args(["--format", "json", "--compact"])
            .output()
            .expect("query Markdown projection");
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("projection JSON")
    };

    let outline = run_json(&[path, "--outline=sections"]);
    assert_eq!(outline["nodes"][0]["kind"], "document-root");
    assert_eq!(outline["nodes"][0]["path"], "root");
    assert_eq!(outline["nodes"][1]["kind"], "document-section");

    let excerpt = run_json(&[path, "--node", "root"]);
    assert_eq!(excerpt["selections"][0]["kind"], "document-root");
    assert_eq!(
        excerpt["selections"][0]["blocks"][0]["children"][0]["value"],
        "Read the preface needle first."
    );

    let search = run_json(&[path, "--search", "preface needle"]);
    assert_eq!(search["total"], 1);
    assert_eq!(search["matches"][0]["node"]["kind"], "document-root");
    assert_eq!(search["matches"][0]["node"]["path"], "root");

    fs::remove_file(path).expect("remove Markdown fixture");
}

fn markdown_fixture_path() -> PathBuf {
    std::env::temp_dir().join(format!("mant-markdown-process-{}.md", std::process::id()))
}

#[test]
fn unknown_options_do_not_expose_rust_source_excerpts() {
    let output = Command::new(executable())
        .arg("--not-an-option")
        .output()
        .expect("run mant");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.starts_with("error: unexpected argument '--not-an-option'"));
    assert!(diagnostic.contains("Usage: mant"));
    assert!(diagnostic.contains("For more information, try '--help'."));
}
