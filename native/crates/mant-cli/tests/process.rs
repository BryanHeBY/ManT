//! Black-box checks for the executable's stdout, stderr, and exit-code contract.

use std::{
    io::Write,
    process::{Command, Stdio},
};

fn executable() -> &'static str {
    env!("CARGO_BIN_EXE_mant-cli")
}

#[test]
fn help_groups_the_public_query_surface() {
    let output = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run mant-cli");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("mant-cli <TOPIC> [OPTIONS]"));
    assert!(help.contains("Document selection:"));
    assert!(help.contains("Search:"));
    assert!(help.contains("Integration:"));
    assert!(help.contains("-h, --help"));
    assert!(help.contains("--format <FORMAT>"));
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
        .expect("run mant-cli -h");
    let long = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run mant-cli --help");

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
        .expect("run mant-cli");

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
            .contains("mant.request/v2")
    );
}

#[test]
fn protocol_version_is_a_clean_json_document() {
    let output = Command::new(executable())
        .args(["--protocol-version", "--compact"])
        .output()
        .expect("run mant-cli");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("protocol JSON");
    assert_eq!(value["protocol"], "mant.cli/v2");
    assert_eq!(value["requestSchema"], "mant.request/v2");
    assert_eq!(value["querySchema"], "mant.query/v2");
    assert_eq!(value["outlineSchema"], "mant.outline/v2");
    assert_eq!(value["excerptSchema"], "mant.excerpt/v2");
    assert_eq!(value["searchSchema"], "mant.search/v1");
}

#[test]
fn invalid_stdin_request_uses_status_two_without_runtime_noise() {
    let mut child = Command::new(executable())
        .args(["--request-json", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant-cli");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"schema":"mant.request/v2","topic":"git","view":{"kind":"full"},"futureField":true}"#,
        )
        .expect("write request");
    let output = child.wait_with_output().expect("wait for mant-cli");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.starts_with("mant-cli: invalid query request JSON:"));
    assert!(!diagnostic.contains("panicked at"));
    assert!(!diagnostic.contains("stack backtrace"));
}

#[test]
fn unknown_options_do_not_expose_rust_source_excerpts() {
    let output = Command::new(executable())
        .arg("--not-an-option")
        .output()
        .expect("run mant-cli");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.starts_with("error: unexpected argument '--not-an-option'"));
    assert!(diagnostic.contains("Usage: mant-cli"));
    assert!(diagnostic.contains("For more information, try '--help'."));
}
