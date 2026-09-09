use super::*;
use std::{
    process::{Command, Stdio},
    sync::atomic::AtomicUsize,
    thread,
    time::{Duration, Instant},
};

use TerminalResource::{AlternateScreen, HiddenCursor, MouseCapture, RawMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Acquire(TerminalResource),
    Release(TerminalResource),
    After,
}

#[derive(Default)]
struct State {
    trace: Vec<Event>,
    active: Vec<TerminalResource>,
    fail_acquire: Option<TerminalResource>,
    fail_release: Vec<TerminalResource>,
    panic_acquire: bool,
}

struct Operations(Arc<Mutex<State>>);

impl TerminalOps for Operations {
    fn acquire(&mut self, resource: TerminalResource) -> io::Result<()> {
        let mut state = self.0.lock();
        state.trace.push(Event::Acquire(resource));
        state.active.push(resource);
        assert!(!state.panic_acquire, "injected acquisition panic");
        if state.fail_acquire == Some(resource) {
            return Err(io::Error::other("partially applied setup"));
        }
        Ok(())
    }

    fn release(&mut self, resource: TerminalResource) -> io::Result<()> {
        let mut state = self.0.lock();
        state.trace.push(Event::Release(resource));
        if let Some(index) = state.fail_release.iter().position(|item| *item == resource) {
            state.fail_release.remove(index);
            return Err(io::Error::other(format!("failed {resource:?}")));
        }
        state.active.retain(|item| *item != resource);
        Ok(())
    }
}

fn owner(state: &Arc<Mutex<State>>) -> Arc<PagerTerminal<Operations>> {
    Arc::new(PagerTerminal::new(Operations(Arc::clone(state))))
}

#[test]
fn every_partial_pager_setup_restores_before_returning_the_setup_error() {
    let order = [AlternateScreen, RawMode, MouseCapture, HiddenCursor];
    for failed in 0..order.len() {
        let state = Arc::new(Mutex::new(State {
            fail_acquire: Some(order[failed]),
            ..State::default()
        }));
        let terminal = owner(&state);
        assert_eq!(
            terminal.setup().unwrap_err().to_string(),
            "partially applied setup"
        );
        let expected: Vec<_> = order[..=failed]
            .iter()
            .copied()
            .map(Event::Acquire)
            .chain(order[..=failed].iter().rev().copied().map(Event::Release))
            .collect();
        assert_eq!(state.lock().trace, expected);
        assert!(state.lock().active.is_empty());
        terminal.restore().unwrap();
        drop(terminal);
        assert_eq!(state.lock().trace, expected);
    }
}

#[test]
fn cleanup_attempts_all_modes_preserves_first_error_and_retries_only_failures() {
    let state = Arc::new(Mutex::new(State {
        fail_release: vec![HiddenCursor, RawMode],
        ..State::default()
    }));
    let terminal = owner(&state);
    terminal.setup().unwrap();
    assert_eq!(
        terminal.restore().unwrap_err().to_string(),
        "failed HiddenCursor"
    );
    assert_eq!(
        &state.lock().trace[4..],
        &[
            Event::Release(HiddenCursor),
            Event::Release(MouseCapture),
            Event::Release(RawMode),
            Event::Release(AlternateScreen),
        ]
    );
    assert_eq!(state.lock().active, [RawMode, HiddenCursor]);
    drop(terminal);
    assert_eq!(
        &state.lock().trace[8..],
        &[Event::Release(HiddenCursor), Event::Release(RawMode)]
    );
    assert!(state.lock().active.is_empty());
}

#[test]
fn failure_cleanup_is_retried_by_the_same_owner_and_unused_owner_is_inert() {
    let state = Arc::new(Mutex::new(State::default()));
    let terminal = owner(&state);
    terminal.restore().unwrap();
    drop(terminal);
    assert!(state.lock().trace.is_empty());

    state.lock().fail_acquire = Some(MouseCapture);
    state.lock().fail_release = vec![MouseCapture];
    let terminal = owner(&state);
    assert_eq!(
        terminal.setup().unwrap_err().to_string(),
        "partially applied setup"
    );
    assert_eq!(state.lock().active, [MouseCapture]);
    terminal.restore().unwrap();
    let expected = state.lock().trace.clone();
    drop(terminal);
    assert_eq!(state.lock().trace, expected);
    assert!(state.lock().active.is_empty());
}

#[test]
fn signal_continuation_runs_after_every_release_even_when_cleanup_fails() {
    let state = Arc::new(Mutex::new(State {
        fail_release: vec![HiddenCursor],
        ..State::default()
    }));
    let terminal = owner(&state);
    terminal.setup().unwrap();
    let result = terminal.restore_then(|| {
        state.lock().trace.push(Event::After);
        // The ledger is no longer held while the signal's default action runs.
        assert!(terminal.lease.try_lock().is_some());
        Ok(())
    });
    assert_eq!(result.unwrap_err().to_string(), "failed HiddenCursor");
    assert_eq!(
        &state.lock().trace[4..],
        &[
            Event::Release(HiddenCursor),
            Event::Release(MouseCapture),
            Event::Release(RawMode),
            Event::Release(AlternateScreen),
            Event::After,
        ]
    );
}

#[test]
fn simultaneous_cleanup_paths_release_each_mode_only_once() {
    let state = Arc::new(Mutex::new(State::default()));
    let terminal = owner(&state);
    terminal.setup().unwrap();
    thread::scope(|scope| {
        for _ in 0..5 {
            let terminal = Arc::clone(&terminal);
            scope.spawn(move || terminal.restore().unwrap());
        }
    });
    drop(terminal);
    assert_eq!(
        &state.lock().trace[4..],
        &[
            Event::Release(HiddenCursor),
            Event::Release(MouseCapture),
            Event::Release(RawMode),
            Event::Release(AlternateScreen),
        ]
    );
    assert!(state.lock().active.is_empty());
}

#[test]
fn search_cursor_uses_the_owner_and_cannot_reacquire_after_cleanup() {
    let state = Arc::new(Mutex::new(State::default()));
    let terminal = owner(&state);
    terminal.setup().unwrap();
    terminal.cursor_visible(true).unwrap();
    terminal.cursor_visible(true).unwrap();
    terminal.cursor_visible(false).unwrap();
    terminal.cursor_visible(false).unwrap();
    assert_eq!(
        &state.lock().trace[4..],
        &[Event::Release(HiddenCursor), Event::Acquire(HiddenCursor),]
    );
    terminal.restore().unwrap();
    let trace = state.lock().trace.clone();
    for visible in [true, false] {
        assert_eq!(
            terminal.cursor_visible(visible).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
    assert_eq!(
        terminal.setup().unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    drop(terminal);
    assert_eq!(state.lock().trace, trace);
}

#[test]
fn active_output_preserves_bytes_but_late_write_and_flush_fail_without_output() {
    let state = Arc::new(Mutex::new(State::default()));
    let terminal = owner(&state);
    let mut writer = terminal.output(Vec::new());
    let bytes = "\x1b[1mCafe\u{301} 👩\u{200d}💻\x1b[0m\n".as_bytes();
    writer.write_all(bytes).unwrap();
    writer.flush().unwrap();
    assert_eq!(writer.inner, bytes);
    terminal.restore().unwrap();
    assert_eq!(
        writer
            .write_all(b"late Hide: \x1b[?25l")
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        writer.flush().unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(writer.inner, bytes);
    assert!(state.lock().trace.is_empty());
}

#[test]
fn stopped_search_returns_before_waiting_for_events_or_touching_output() {
    let terminal = PagerTerminal::new(PagerOperations);
    terminal.restore().unwrap();
    let state = super::super::native::PagerState::new().unwrap();
    let mut output = Vec::new();
    let result = super::super::native::search::fetch_input(&mut output, &state, &terminal);
    assert!(
        matches!(result, Err(MinusError::TerminalLifecycle(error)) if error.kind() == io::ErrorKind::BrokenPipe)
    );
    assert!(output.is_empty());
}

#[test]
fn writer_waiting_on_stdout_observes_stop_before_touching_its_output() {
    let state = Arc::new(Mutex::new(State::default()));
    let terminal = owner(&state);
    terminal.setup().unwrap();
    let mut writer = terminal.output(Vec::new());
    let stdout = io::stdout();
    let guard = stdout.lock();
    let (started, ready) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        started.send(()).unwrap();
        let result = writer.write_all(b"late frame");
        (writer.inner, result)
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    // This thread owns stdout throughout restoration. The peer cannot pass
    // its locked check, even if it was already attempting the write.
    terminal.restore().unwrap();
    drop(guard);
    let (bytes, result) = worker.join().unwrap();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert!(bytes.is_empty());
    assert!(state.lock().active.is_empty());
}

// Global hook tests run in a dedicated test process, never replacing another
// concurrently running unit test's hook. The watchdog also detects deadlocks.
#[test]
fn scoped_panic_hook_and_partial_acquisition_unwind_are_recoverable() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "delivery::pager::lifecycle::tests::panic_hook_child",
            "--nocapture",
        ])
        .env("MANT_TEST_PAGER_HOOK_CHILD", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("pager panic-hook child timed out");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn panic_hook_child() {
    if std::env::var_os("MANT_TEST_PAGER_HOOK_CHILD").is_none() {
        return;
    }
    let original = panic::take_hook();
    let calls = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&calls);
    panic::set_hook(Box::new(move |_| {
        recorded.fetch_add(1, Ordering::SeqCst);
    }));
    for _ in 0..3 {
        let state = Arc::new(Mutex::new(State::default()));
        let terminal = owner(&state);
        let weak = Arc::downgrade(&terminal);
        terminal
            .interactive(|| terminal.setup().map_err(MinusError::TerminalLifecycle))
            .unwrap();
        assert!(state.lock().active.is_empty());
        drop(terminal);
        assert!(
            weak.upgrade().is_none(),
            "normal return leaked a hook's terminal owner"
        );
    }

    let state = Arc::new(Mutex::new(State {
        panic_acquire: true,
        ..State::default()
    }));
    let terminal = owner(&state);
    let weak = Arc::downgrade(&terminal);
    assert!(
        panic::catch_unwind(AssertUnwindSafe(|| {
            terminal.interactive(|| terminal.setup().map_err(MinusError::TerminalLifecycle))
        }))
        .is_err()
    );
    assert!(state.lock().active.is_empty());
    assert_eq!(
        state.lock().trace,
        [
            Event::Acquire(AlternateScreen),
            Event::Release(AlternateScreen)
        ]
    );
    assert!(terminal.exited().load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(terminal);
    assert!(
        weak.upgrade().is_none(),
        "unwinding leaked a hook's terminal owner"
    );
    assert!(panic::catch_unwind(|| panic!("after pager return")).is_err());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "previous hook was not restored"
    );
    panic::set_hook(original);
}
