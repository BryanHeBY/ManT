//! Real terminal acquisition/restoration with failures in the production UI.
#![cfg(unix)]

#[test]
fn recovery_child() {
    let Ok(case) = std::env::var("MANT_RECOVERY_TEST_CASE") else {
        return;
    };
    let content = mant_engine::query_markdown_text("# Recovery\n\nBody.\n", None).unwrap();
    if case == "panic" {
        super::run_with_catalog(
            &content,
            mant_protocol::DocumentCatalog::default(),
            |_| panic!("injected host callback panic"),
            |_| Err("unused".into()),
            |_| Err("unused".into()),
        )
        .unwrap();
        panic!("expected discovery callback to panic");
    } else {
        assert_eq!(case, "initialization");
        initialization_failure(&content);
    }
}

#[test]
fn panic_and_initialization_failure_restore_a_real_pty() {
    let harness = include_str!("../../../tests/support/display_pty.py");
    let program = format!(
        "__name__ = 'recovery_harness'\n{harness}\n{}",
        r#"
environment = dict(os.environ, TERM="xterm-256color", NO_COLOR="1")
for case in ["panic", "initialization"]:
    env = dict(environment, MANT_RECOVERY_TEST_CASE=case)
    in_session(lambda: check_in_session(
        [sys.argv[1], "--exact", "delivery::terminal::tests::recovery_child", "--nocapture"], True, env,
        action=lambda _process, master: os.write(master, b"\x0f") if case == "panic" else None,
        returncodes=(101,) if case == "panic" else (0,),
        diagnostic=b"injected host callback panic" if case == "panic" else None,
    ))
    print("Rust UI recovery", case, "passed", flush=True)
"#
    );
    let mut child = std::process::Command::new("python3")
        .args(["-c", &program])
        .arg(std::env::current_exe().unwrap())
        .spawn()
        .expect("Python 3 PTY harness");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(65);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "PTY recovery failed: {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("PTY recovery supervisor timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[allow(unsafe_code)] // Test-only fd fault injection in a disposable child.
fn initialization_failure(content: &mant_ir::ResolvedContent) {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn dup2(old: std::ffi::c_int, new: std::ffi::c_int) -> std::ffi::c_int;
    }
    let tty = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .unwrap();
    let (broken, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(peer);
    // Only this disposable child changes fd 1; the libtest harness runs before
    // and after the injection with a working stdout. Neither file is closed
    // while its descriptor is passed to dup2.
    // Use EPIPE, not EBADF: Rust stdout deliberately treats a closed/invalid
    // descriptor as a sink, so a read-only fd would not test an I/O error.
    assert_eq!(unsafe { dup2(broken.as_raw_fd(), 1) }, 1);
    let result = super::run(content);
    assert_eq!(unsafe { dup2(tty.as_raw_fd(), 1) }, 1);
    // Stdout retains failed buffered sequences until fd restoration; the
    // harness sees both enter/leave commands and verifies final termios too.
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
}
