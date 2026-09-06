//! Real Unix PTY checks complement the platform-neutral output policy matrix.

#[cfg(unix)]
#[test]
fn terminal_display_and_restoration() {
    let mut process = std::process::Command::new("python3")
        .args([
            "-c",
            include_str!("support/display_pty.py"),
            env!("CARGO_BIN_EXE_mant"),
        ])
        // Inherit output rather than filling a pipe while the watchdog polls.
        .spawn()
        .expect("Python 3 is required by the Unix verification scripts");
    // Independent of Python's per-case deadlines, including subprocess setup.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        if let Some(status) = process.try_wait().expect("poll PTY checks") {
            assert!(status.success(), "PTY checks failed: {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            panic!("PTY harness exceeded its 180-second deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
