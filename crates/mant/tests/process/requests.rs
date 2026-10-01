use super::PROTOCOL_REFERENCE;
use super::process_support::{executable, run_text_input};
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

#[test]
fn default_file_stdin_and_request_outputs_are_text() {
    let path = std::env::temp_dir().join(format!("mant-text-default-{}.md", std::process::id()));
    let source = "# Text Default\n\n## Options\n\n<!-- mant:entries role=option -->\n- `--flag`: A **strong** description.\n\n```sh\necho example\n```\n";
    fs::write(&path, source).expect("write text fixture");
    let path_text = path.to_str().expect("UTF-8 fixture path");
    for (arguments, input) in [
        (vec!["--input", path_text], String::new()),
        (
            vec!["--input", "-", "--input-format", "markdown"],
            source.to_owned(),
        ),
        (
            vec!["--request-json"],
            serde_json::json!({
                "schema": "mant.request/v0.12",
                "input": {"kind": "file", "path": path_text, "format": "markdown"},
                "view": {"kind": "full"}
            })
            .to_string(),
        ),
    ] {
        let default = run_text_input(&arguments, &input);
        assert!(default.status.success(), "{:?}", default.stderr);
        assert!(default.stderr.is_empty());
        let body = String::from_utf8_lossy(&default.stdout);
        assert!(body.contains("A strong description."), "{body}");
        assert!(body.contains("echo example"));
        assert!(!body.contains("```"));
        assert!(!body.contains('\x1b'));
        let mut explicit = arguments.clone();
        explicit.extend(["--format", "text"]);
        assert_eq!(default.stdout, run_text_input(&explicit, &input).stdout);
        let mut markdown = arguments;
        markdown.extend(["--format", "markdown", "--color", "always"]);
        let markdown = run_text_input(&markdown, &input);
        assert!(markdown.status.success());
        let body = String::from_utf8_lossy(&markdown.stdout);
        assert!(body.contains("**strong**"));
        assert!(body.contains("```sh"));
        assert!(!body.contains('\x1b'));
    }
    for extra in [vec![], vec!["--format", "text"]] {
        let mut args = vec!["--input", path_text, "--color", "always"];
        args.extend(extra);
        let colored = run_text_input(&args, "");
        assert!(colored.status.success());
        assert!(colored.stdout.contains(&0x1b));
    }
    fs::remove_file(path).expect("remove text fixture");
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
            .contains("mant.request/v0.12")
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
    assert_eq!(value["protocol"], "mant.cli/v0.12");
    assert_eq!(value["requestSchema"], "mant.request/v0.12");
    assert_eq!(value["querySchema"], "mant.query/v0.12");
    assert_eq!(value["outlineSchema"], "mant.outline/v0.12");
    assert_eq!(value["excerptSchema"], "mant.excerpt/v0.12");
    assert_eq!(value["searchSchema"], "mant.search/v0.12");

    for (label, reference) in [
        ("protocol manual", PROTOCOL_REFERENCE),
        (
            "command manual",
            include_str!("../../../../docs/manuals/mant.md"),
        ),
    ] {
        let descriptor = reference
            .split("```json")
            .skip(1)
            .filter_map(|block| block.split_once("```"))
            .filter_map(|(json, _)| serde_json::from_str::<serde_json::Value>(json.trim()).ok())
            .find(|example| example["protocol"] == value["protocol"])
            .unwrap_or_else(|| panic!("{label} must contain a complete protocol descriptor"));
        assert_eq!(descriptor, value, "{label} protocol descriptor is stale");
    }

    for (field, marker) in value.as_object().expect("protocol descriptor") {
        let documented = format!(
            "\"{field}\": {}",
            serde_json::to_string(marker).expect("protocol marker")
        );
        assert!(
            PROTOCOL_REFERENCE.contains(&documented),
            "the protocol reference must document {field}"
        );
    }

    for tool in [
        "mant_find",
        "mant_outline",
        "mant_read",
        "mant_explain",
        "mant_search",
    ] {
        assert!(
            PROTOCOL_REFERENCE.contains(&format!("`{tool}`")),
            "the protocol reference must document MCP tool {tool}"
        );
    }
    for retired in [
        "mant_documents_list",
        "mant_document_outline",
        "mant_document_get",
        "mant_document_explain",
        "mant_document_search",
    ] {
        assert!(
            !PROTOCOL_REFERENCE.contains(retired),
            "the protocol reference must not retain retired MCP tool {retired}"
        );
    }
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
            br#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"git"},"view":{"kind":"full"},"futureField":true}"#,
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
        .args([
            "--input",
            "-",
            "--input-format",
            "markdown",
            "--format",
            "json",
            "--compact",
        ])
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
        value["document"]["sections"][0]["blocks"][0]["items"][0]["entry"]["names"][0],
        "--help"
    );
}

#[test]
#[cfg(feature = "roff")]
fn explicit_roff_files_and_stdin_use_the_native_parser() {
    let path =
        std::env::temp_dir().join(format!("mant-direct-roff-process-{}.1", std::process::id()));
    let source = b".TH DIRECT-ROFF 1\n.SH NAME\ndirect-roff \\- standalone input\n";
    fs::write(&path, source).expect("write roff input");

    let file = Command::new(executable())
        .args([
            "--input",
            path.to_str().expect("UTF-8 path"),
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("query roff file");
    fs::remove_file(path).expect("remove roff input");
    assert!(file.status.success(), "{file:?}");
    assert!(file.stderr.is_empty());
    let file: serde_json::Value = serde_json::from_slice(&file.stdout).expect("roff file JSON");
    assert_eq!(file["document"]["source"]["format"], "man");
    assert_eq!(file["document"]["meta"]["manualSection"], "1");

    let mut child = Command::new(executable())
        .args([
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            "json",
            "--compact",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start roff stdin query");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source)
        .expect("write roff stdin");
    let stdin = child.wait_with_output().expect("wait for roff stdin query");
    assert!(stdin.status.success(), "{stdin:?}");
    assert!(stdin.stderr.is_empty());
    let stdin: serde_json::Value = serde_json::from_slice(&stdin.stdout).expect("roff stdin JSON");
    assert_eq!(stdin["label"], "DIRECT-ROFF");
    assert_eq!(stdin["document"]["source"]["format"], "man");
}

#[test]
fn direct_and_protocol_queries_read_local_markdown_files_by_path() {
    let path = markdown_fixture_path();
    fs::write(&path, "# Local\n\nBody.\n").expect("write Markdown fixture");

    let direct = Command::new(executable())
        .args([
            "--input",
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
    assert_eq!(value["document"]["heading"]["content"][0]["value"], "Local");
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
        "schema": "mant.request/v0.12",
        "input": {
            "kind": "file",
            "path": path.to_str().expect("UTF-8 path"),
            "format": "markdown",
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

fn markdown_fixture_path() -> PathBuf {
    std::env::temp_dir().join(format!("mant-markdown-process-{}.md", std::process::id()))
}
