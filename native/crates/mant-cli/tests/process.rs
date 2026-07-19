//! Black-box checks for the executable's stdout, stderr, and exit-code contract.

use std::{
    io::Write,
    process::{Command, Stdio},
};

fn executable() -> &'static str {
    env!("CARGO_BIN_EXE_mant-cli")
}

#[test]
fn protocol_version_is_a_clean_json_document() {
    let output = Command::new(executable())
        .args(["protocol-version", "--compact"])
        .output()
        .expect("run mant-cli");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("protocol JSON");
    assert_eq!(value["protocol"], "mant.cli/v1");
    assert_eq!(value["querySchema"], "mant.query/v1");
    assert_eq!(value["outlineSchema"], "mant.outline/v1");
    assert_eq!(value["excerptSchema"], "mant.excerpt/v1");
}

#[test]
fn invalid_stdin_request_uses_status_two_without_runtime_noise() {
    let mut child = Command::new(executable())
        .args(["--request-json", "--json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant-cli");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(br#"{"topic":"git","futureField":true}"#)
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
    assert_eq!(
        String::from_utf8(output.stderr).expect("UTF-8 diagnostic"),
        "mant-cli: unknown option '--not-an-option'\nTry 'mant-cli --help' for more information.\n"
    );
}
