//! Real Unix PTY checks complement the platform-neutral output policy matrix.

#[cfg(unix)]
#[test]
fn terminal_display_and_restoration() {
    let result = std::process::Command::new("python3")
        .args([
            "-c",
            include_str!("support/display_pty.py"),
            env!("CARGO_BIN_EXE_mant"),
        ])
        .output()
        .expect("Python 3 is required by the Unix verification scripts");
    assert!(
        result.status.success(),
        "PTY checks failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
