//! Contains functions that initialize minus
//!
//! This module provides two main functions:-
//! * The [`init_core`] function which is responsible for setting the initial state of the
//!   Pager, do environment checks and initializing various core functions on either async
//!   tasks or native threads depending on the feature set
//!
//! * The [`start_reactor`] function displays the displays the output and also polls
//!   the [`Receiver`] held inside the [`Pager`] for events. Whenever a event is
//!   detected, it reacts to it accordingly.
use crate::delivery::pager::native::{
    Pager, PagerState,
    error::MinusError,
    hooks::Hook,
    input::InputEvent,
    minus_core::{
        RunMode,
        commands::Command,
        ev_handler::handle_event,
        utils::display::draw_full,
    },
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use crossterm::event;
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(not(test))]
use std::io::stdout;

use parking_lot::Condvar;
use parking_lot::Mutex;

use super::{CommandQueue, RUNMODE, utils::display::draw_for_change};
use crate::delivery::pager::lifecycle::PagerTerminal;

// The run mode belongs to this invocation, including direct-output errors,
// partially completed setup, worker errors and unwinding.
struct RunModeGuard<'a>(&'a Mutex<RunMode>);

impl<'a> RunModeGuard<'a> {
    fn acquire(state: &'a Mutex<RunMode>, mode: RunMode) -> Self {
        let mut current = state.lock();
        assert_eq!(*current, RunMode::Uninitialized, "another pager is already running");
        *current = mode;
        Self(state)
    }
}

impl Drop for RunModeGuard<'_> {
    fn drop(&mut self) {
        *self.0.lock() = RunMode::Uninitialized;
    }
}

#[cfg(test)]
mod run_mode_tests {
    use super::{Mutex, RunMode, RunModeGuard};

    #[test]
    fn run_mode_is_reusable_after_success_error_and_unwind() {
        let state = Mutex::new(RunMode::Uninitialized);
        for fail in [false, true] {
            let result: std::io::Result<()> = (|| {
                let _mode = RunModeGuard::acquire(&state, RunMode::Static);
                assert_eq!(*state.lock(), RunMode::Static);
                if fail {
                    return Err(std::io::Error::other("direct-output or setup failure"));
                }
                Ok(())
            })();
            assert_eq!(result.is_err(), fail);
            assert_eq!(*state.lock(), RunMode::Uninitialized);
        }
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _mode = RunModeGuard::acquire(&state, RunMode::Static);
            panic!("worker panic");
        })).is_err());
        assert_eq!(*state.lock(), RunMode::Uninitialized);
        let mode = RunModeGuard::acquire(&state, RunMode::Static);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            RunModeGuard::acquire(&state, RunMode::Static)
        })).is_err());
        assert_eq!(*state.lock(), RunMode::Static, "a rejected invocation does not own reset");
        drop(mode);
        assert_eq!(*state.lock(), RunMode::Uninitialized);
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn search_cannot_steal_an_event_between_reader_poll_and_read() {
        let active = Arc::new((Mutex::new(true), Condvar::new()));
        let pending = Arc::new(AtomicBool::new(true));
        let (polled, ready) = std::sync::mpsc::channel();
        let (continue_read, proceed) = std::sync::mpsc::channel();
        let reader = {
            let active = Arc::clone(&active);
            let pending = Arc::clone(&pending);
            std::thread::spawn(move || {
                read_active_event(&active, &AtomicBool::new(false), || {
                    assert!(pending.load(Ordering::SeqCst), "poll sees the pending event");
                    polled.send(()).unwrap();
                    proceed.recv_timeout(Duration::from_secs(2)).unwrap();
                    assert!(pending.swap(false, Ordering::SeqCst), "read retains its polled event");
                    Ok(Some(event::Event::FocusGained))
                }).unwrap()
            })
        };
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(active.0.try_lock().is_none(), "poll/read must retain the existing input gate");
        let search = {
            let active = Arc::clone(&active);
            let pending = Arc::clone(&pending);
            std::thread::spawn(move || {
                let mut enabled = active.0.lock();
                *enabled = false;
                pending.swap(false, Ordering::SeqCst)
            })
        };
        continue_read.send(()).unwrap();
        assert_eq!(reader.join().unwrap(), Some(event::Event::FocusGained));
        assert!(!search.join().unwrap(), "search begins only after the reader consumed its event");
        assert!(!*active.0.lock());
    }

    #[test]
    fn idle_and_paused_workers_stop_without_an_input_event() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "delivery::pager::native::minus_core::init::worker_tests::cancellation_child", "--nocapture"])
            .env("MANT_TEST_PAGER_CANCEL_CHILD", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if child.try_wait().unwrap().is_some() {
                let output = child.wait_with_output().unwrap();
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("pager cancellation child timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn cancellation_child() {
        if std::env::var_os("MANT_TEST_PAGER_CANCEL_CHILD").is_none() {
            return;
        }
        let terminal = Arc::new(PagerTerminal::new(crate::delivery::pager::lifecycle::PagerOperations));
        let exited = terminal.exited();
        let mut state = PagerState::new().unwrap();
        assert!(state.hooks.remove_callback(Hook::PostPagerExit, 1));
        let (started, ready) = std::sync::mpsc::channel();
        state.hooks.add_callback(Hook::PostPagerStart, 17, Box::new(move |_| {
            started.send(()).unwrap();
        }));
        let state = Arc::new(Mutex::new(state));
        let active = Arc::new((Mutex::new(true), Condvar::new()));
        let (_send, receive) = crossbeam_channel::unbounded();
        let _mode = RunModeGuard::acquire(&RUNMODE, RunMode::Static);
        let (done, completed) = std::sync::mpsc::channel();
        let reactor = {
            let state = Arc::clone(&state);
            let active = Arc::clone(&active);
            let terminal = Arc::clone(&terminal);
            let exited = Arc::clone(&exited);
            std::thread::spawn(move || {
                let result = start_reactor(&receive, &state, Vec::new(), &active, &exited, &terminal);
                done.send(result).unwrap();
            })
        };
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        // With the initial frame complete and no commands sent, this covers
        // the idle receive rather than only an already-cancelled startup.
        std::thread::sleep(Duration::from_millis(150));
        exited.store(true, Ordering::SeqCst);
        completed.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        reactor.join().unwrap();

        let exited = Arc::new(AtomicBool::new(false));
        let active = Arc::new((Mutex::new(false), Condvar::new()));
        let (send, _receive) = crossbeam_channel::unbounded();
        let (done, completed) = std::sync::mpsc::channel();
        let reader = {
            let active = Arc::clone(&active);
            let exited = Arc::clone(&exited);
            std::thread::spawn(move || {
                done.send(event_reader(&send, &state, &active, &exited)).unwrap();
            })
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        // notify_one reports a real waiter. The flag changes only after the
        // reader reached the paused-input wait, not before its initial check.
        while !active.1.notify_one() {
            assert!(Instant::now() < deadline, "reader never reached paused wait");
            std::thread::yield_now();
        }
        exited.store(true, Ordering::SeqCst);
        completed.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        reader.join().unwrap();
    }
}

/// The main entry point of minus
///
/// This is called by both [`dynamic_paging`](crate::delivery::pager::native::dynamic_paging) and
/// [`page_all`](crate::delivery::pager::native::page_all) functions.
///
/// It first receives all events present inside the [`Pager`]'s receiver
/// and creates the initial state that to be stored inside the [`PagerState`]
///
/// Then it checks if the minus is running in static mode and does some checks:-
/// * If standard output is not a terminal screen, that is if it is a file or block
///   device, minus will write all the data at once to the stdout and quit
///
/// * If the size of the data is less than the available number of rows in the terminal
///   then it displays everything on the main stdout screen at once and quits. This
///   behaviour can be turned off if [`Pager::set_run_no_overflow`] is called
///   by the main application
// Sorry... this behaviour would have been cool to have in async mode, just think about it!!! Many
// implementations were proposed but none were perfect
// It is because implementing this especially with line wrapping and terminal scrolling
// is a a nightmare because terminals are really naughty and more when you have to fight with it
// using your library... your only weapon
// So we just don't take any more proposals about this. It is really frustating to
// to thoroughly test each implementation and fix out all rough edges around it
///   Next it initializes the runtime and calls [`start_reactor`] and a [`event reader`] which is
///   selected based on the enabled feature set:-
///
/// # Errors
///
/// Setting/cleaning up the terminal can fail and IO to/from the terminal can
/// fail.
///
/// [`event reader`]: event_reader
#[allow(clippy::module_name_repetitions)]
#[allow(clippy::too_many_lines)]
pub fn init_core(
    pager: &Pager,
    rm: RunMode,
    terminal: &Arc<PagerTerminal>,
) -> std::result::Result<(), MinusError> {
    #[cfg(not(test))]
    let mut out = stdout();

    // Is the event reader running
    let input_thread_running = Arc::new((Mutex::new(true), Condvar::new()));

    assert_eq!(
        *super::RUNMODE.lock(),
        RunMode::Uninitialized,
        "Failed to set the RUNMODE. This is caused probably because another instance of minus is already running"
    );

    #[allow(unused_mut)]
    let mut ps = crate::delivery::pager::native::state::PagerState::generate_initial_state(&pager.rx)?;
    let _run_mode = RunModeGuard::acquire(&RUNMODE, rm);
    ps.run_hooks(Hook::PrePagerStart);

    // Static mode checks
    #[cfg(not(test))]
    if *RUNMODE.lock() == RunMode::Static {
        use {super::utils::display::write_raw_lines, crossterm::tty::IsTty};
        // If stdout is not a tty, write everything and quit
        if !out.is_tty() {
            write_raw_lines(&mut out, &[ps.screen.orig_text], None)?;
            return Ok(());
        }
        // If number of lines of text is less than available rows, write everything and quit
        // unless run_no_overflow is set to true
        if ps.screen.formatted_lines_count() <= ps.rows && !ps.run_no_overflow {
            write_raw_lines(&mut out, &ps.screen.formatted_lines, Some("\r"))?;
            ps.exit();
            return Ok(());
        }
    }

    terminal.interactive(|| {
        // The direct-output path above never acquires modes or replaces a hook.
        #[cfg(not(test))]
        {
            use crossterm::tty::IsTty;
            if !out.is_tty() {
                return Err(crate::delivery::pager::native::error::SetupError::InvalidTerminal.into());
            }
            terminal.setup().map_err(MinusError::TerminalLifecycle)?;
        }

        let is_exited = terminal.exited();

        let ps_mutex = Arc::new(Mutex::new(ps));

        let evtx = pager.tx.clone();
        let rx = pager.rx.clone();

        let p1 = ps_mutex.clone();

        let input_thread_running2 = input_thread_running.clone();

        std::thread::scope(|s| -> crate::delivery::pager::native::Result {
            let is_exited3 = is_exited.clone();
            let is_exited4 = is_exited.clone();

            #[cfg(test)]
            let mut out2 = Vec::new();
            #[cfg(not(test))]
            let mut out2 = terminal.output(stdout());

            let t1 = s.spawn(move || {
                let res = event_reader(
                    &evtx,
                    &p1,
                    &input_thread_running2,
                    &is_exited3,
                );

                if res.is_err() {
                    is_exited3.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                res
            });
            let t2 = s.spawn(move || {
                let res = start_reactor(
                    &rx,
                    &ps_mutex,
                    &mut out2,
                    &input_thread_running,
                    &is_exited4,
                    terminal,
                );

                if res.is_err() {
                    is_exited4.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                res
            });

            let r1 = t1.join().unwrap();
            let r2 = t2.join().unwrap();

            if r1.is_err() || r2.is_err() {
                // Keep the worker failure primary; the same lease retries any
                // failed releases at the outer interactive boundary and Drop.
                let _ = terminal.restore();
            }

            r1?;
            r2?;
            Ok(())
        })
    })
}

/// Continuously displays the output and reacts to events
///
/// This function displays the output continuously while also checking for user inputs.
///
/// Whenever a event like a user input or instruction from the main application is detected
/// it will call [`handle_event`] to take required action for the event.
/// Then it will be do some checks if it is really necessory to redraw the screen
/// and redraw if it event requires it to do so.
///
/// For example if all rows in a terminal aren't filled and a
/// [`AppendData`](super::commands::Command::AppendData) event occurs, it is absolutely necessary to
/// update the screen immediately; while if all rows are filled, we can omit to redraw the screen.
#[allow(clippy::too_many_lines)]
fn start_reactor(
    rx: &Receiver<Command>,
    ps: &Arc<Mutex<PagerState>>,
    mut out_lock: impl Write,
    input_thread_running: &Arc<(Mutex<bool>, Condvar)>,
    is_exited: &Arc<AtomicBool>,
    terminal: &PagerTerminal,
) -> Result<(), MinusError> {
    let mut command_queue = CommandQueue::new();

    {
        let mut p = ps.lock();

        draw_full(&mut out_lock, &mut p)?;
        p.run_hooks(Hook::PostPagerStart);

        if p.follow_output {
            draw_for_change(&mut out_lock, &mut p, &mut (usize::MAX - 1))?;
        }
    }

    let run_mode = *RUNMODE.lock();
    match run_mode {
        #[cfg(any())]
        RunMode::Dynamic => loop {
            if is_exited.load(Ordering::SeqCst) {
                terminal.restore().map_err(MinusError::TerminalLifecycle)?;
                ps.lock().run_hooks(Hook::PostPagerExit);
                break;
            }

            let next_command = if command_queue.is_empty() {
                rx.recv()
            } else {
                Ok(command_queue.pop_front().unwrap())
            };

            let mut p = ps.lock();
            if let Ok(Command::Io(ic)) = next_command {
                use crate::delivery::pager::native::minus_core::ev_handler::handle_io_command;

                handle_io_command(
                    ic,
                    &mut out_lock,
                    &mut p,
                    &mut command_queue,
                    input_thread_running,
                    terminal,
                )?;
            } else if let Ok(command) = next_command {
                handle_event(command, &mut p, &mut command_queue, is_exited);
            }
        },
        RunMode::Static => {
            loop {
                if is_exited.load(Ordering::SeqCst) {
                    // Cleanup the screen
                    //
                    // This is not needed in dynamic paging because this is already handled by handle_event
                    terminal.restore().map_err(MinusError::TerminalLifecycle)?;
                    ps.lock().run_hooks(Hook::PostPagerExit);

                    break;
                }
                let next_command = if command_queue.is_empty() {
                    rx.recv_timeout(std::time::Duration::from_millis(100))
                } else {
                    Ok(command_queue.pop_front().unwrap())
                };

                let mut p = ps.lock();

                if let Ok(Command::Io(ic)) = next_command {
                    use crate::delivery::pager::native::minus_core::ev_handler::handle_io_command;

                    handle_io_command(
                        ic,
                        &mut out_lock,
                        &mut p,
                        &mut command_queue,
                        input_thread_running,
                        terminal,
                    )?;
                } else if let Ok(command) = next_command {
                    handle_event(command, &mut p, &mut command_queue, is_exited);
                }
            }
        }
        RunMode::Uninitialized => panic!(
            "Static variable RUNMODE set to uninitialized.\
This is most likely a bug. Please open an issue to the developers"
        ),
    }
    Ok(())
}

// A search pause and a poll/read pair are mutually exclusive. No PagerState or
// stdout lock is acquired while waiting for this gate; callers release it
// before interpreting events or drawing.
fn read_active_event(
    user_input_active: &Arc<(Mutex<bool>, Condvar)>,
    is_exited: &AtomicBool,
    read: impl FnOnce() -> Result<Option<event::Event>, MinusError>,
) -> Result<Option<event::Event>, MinusError> {
    let (lock, cvar) = (&user_input_active.0, &user_input_active.1);
    let mut active = lock.lock();
    while !*active && !is_exited.load(Ordering::SeqCst) {
        cvar.wait_for(&mut active, std::time::Duration::from_millis(100));
    }
    if is_exited.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let event = read();
    drop(active);
    event
}

fn event_reader(
    evtx: &Sender<Command>,
    ps: &Arc<Mutex<PagerState>>,
    user_input_active: &Arc<(Mutex<bool>, Condvar)>,
    is_exited: &Arc<AtomicBool>,
) -> Result<(), MinusError> {
    loop {
        if is_exited.load(Ordering::SeqCst) {
            break;
        }

        let event = read_active_event(user_input_active, is_exited, || {
            // Search must acquire this same gate before pausing the reader.
            // Keep poll/read atomic with respect to that transition so search
            // cannot consume the event that made this poll report ready.
            let ready = event::poll(std::time::Duration::from_millis(100))
                .map_err(|e| MinusError::HandleEvent(e.into()))?;
            if ready {
                Ok(Some(event::read().map_err(|e| MinusError::HandleEvent(e.into()))?))
            } else {
                Ok(None)
            }
        })?; // Release the input gate before taking PagerState (search holds it).

        if let Some(ev) = event {
            let mut guard = ps.lock();
            // Get the events
            let input = guard.input_classifier.classify_input(ev, &guard);
            if let Some(iev) = input {
                if !matches!(iev, InputEvent::Number(_)) {
                    guard.prefix_num.clear();
                    guard.format_prompt();
                }
                if let Err(TrySendError::Disconnected(_)) = evtx.try_send(Command::UserInput(iev)) {
                    break;
                }
            } else {
                guard.prefix_num.clear();
                guard.format_prompt();
            }
        }
    }
    Result::<(), MinusError>::Ok(())
}
