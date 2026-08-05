//! Verifies the temporary Rust TUI executable at its process boundary.

use std::process::Command;

fn run(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mantui-rs"))
        .args(arguments)
        .output()
        .expect("run mantui-rs")
}

#[test]
fn help_is_available_without_an_interactive_terminal() {
    for argument in ["-h", "--help"] {
        let output = run(&[argument]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
        assert!(stdout.contains("Usage: mantui-rs"));
        assert!(stdout.contains("--force-libmandoc"));
    }
}

#[test]
fn redirected_use_fails_before_querying_the_manual_database() {
    let output = run(&["a-topic-that-must-not-be-queried"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).expect("UTF-8 diagnostic"),
        "mantui-rs: interactive view requires a terminal; use mant for Markdown or JSON output\n"
    );
}

#[test]
fn invalid_arguments_use_status_two_without_runtime_excerpts() {
    let output = run(&["--unknown"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(stderr.contains("unexpected argument '--unknown'"));
    assert!(!stderr.contains("panicked at"));
}
