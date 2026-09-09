//! POSIX termination delivery for the terminal lifecycle boundary.

use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use signal_hook::{
    SigId,
    consts::signal::{SIGHUP, SIGINT, SIGQUIT, SIGTERM},
};

const TERMINATION_SIGNALS: &[i32] = &[SIGHUP, SIGINT, SIGQUIT, SIGTERM];

/// Own registrations from the first successful acquisition, including while
/// later handler registration is still fallible. Draining also makes explicit
/// terminal cleanup followed by Drop harmless.
struct Registrations<T, U: FnMut(T) -> bool> {
    tokens: Vec<T>,
    unregister: U,
}

impl<T, U: FnMut(T) -> bool> Registrations<T, U> {
    fn clear(&mut self) {
        for token in self.tokens.drain(..) {
            (self.unregister)(token);
        }
    }
}

impl<T, U: FnMut(T) -> bool> Drop for Registrations<T, U> {
    fn drop(&mut self) {
        self.clear();
    }
}

fn register_handlers<T, U: FnMut(T) -> bool>(
    signals: &[i32],
    mut register_default: impl FnMut(i32) -> io::Result<T>,
    mut register_pending: impl FnMut(i32) -> io::Result<T>,
    unregister: U,
) -> io::Result<Registrations<T, U>> {
    let mut registrations = Registrations {
        tokens: Vec::new(),
        unregister,
    };
    for &signal in signals {
        // The default-action handler must precede the pending-state handler.
        // A failure of either leaves every earlier token owned by this guard.
        registrations.tokens.push(register_default(signal)?);
        registrations.tokens.push(register_pending(signal)?);
    }
    Ok(registrations)
}

/// Convert async signals into state polled by the ordinary Rust event loop.
///
/// Signal handlers only touch lock-free atomics. Terminal restoration and
/// default signal emulation therefore happen outside signal context.
pub(crate) struct TerminationSignals {
    pending: Arc<AtomicUsize>,
    terminating: Arc<AtomicBool>,
    registrations: Registrations<SigId, fn(SigId) -> bool>,
}

impl TerminationSignals {
    pub(crate) fn install() -> io::Result<Self> {
        Self::install_for(TERMINATION_SIGNALS)
    }

    fn install_for(signals: &[i32]) -> io::Result<Self> {
        let pending = Arc::new(AtomicUsize::new(0));
        let terminating = Arc::new(AtomicBool::new(false));
        let registrations = register_handlers(
            signals,
            // This handler runs first. A second termination signal after the
            // event loop begins cleanup gets the platform's default action.
            |signal| {
                signal_hook::flag::register_conditional_default(signal, Arc::clone(&terminating))
            },
            |signal| {
                signal_hook::flag::register_usize(
                    signal,
                    Arc::clone(&pending),
                    usize::try_from(signal).expect("POSIX signal numbers are positive"),
                )
            },
            signal_hook::low_level::unregister as fn(SigId) -> bool,
        )?;
        Ok(Self {
            pending,
            terminating,
            registrations,
        })
    }

    pub(crate) fn take(&self) -> Option<i32> {
        let signal = self.pending.swap(0, Ordering::SeqCst);
        if signal == 0 {
            return None;
        }
        self.terminating.store(true, Ordering::SeqCst);
        i32::try_from(signal).ok()
    }

    pub(crate) fn terminate(mut self, signal: i32) -> io::Result<()> {
        self.unregister();
        signal_hook::low_level::emulate_default_handler(signal)
    }

    fn unregister(&mut self) {
        self.registrations.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        io,
        sync::atomic::Ordering,
        thread,
        time::{Duration, Instant},
    };

    use signal_hook::consts::signal::SIGUSR1;

    use super::{TerminationSignals, register_handlers};

    mod mask;

    #[test]
    fn partial_registration_failure_releases_only_successful_tokens_once() {
        for fail_at in 0..4 {
            let attempts = Cell::new(0);
            let released = RefCell::new(Vec::new());
            let register = |signal| {
                let token = attempts.get();
                attempts.set(token + 1);
                if token == fail_at {
                    Err(io::Error::other("injected registration failure"))
                } else {
                    Ok((signal, token))
                }
            };
            let result = register_handlers(&[11, 22], register, register, |token| {
                released.borrow_mut().push(token);
                true
            });
            let Err(error) = result else {
                panic!("registration must fail at {fail_at}");
            };
            assert_eq!(error.to_string(), "injected registration failure");
            assert_eq!(attempts.get(), fail_at + 1);
            let expected = (0..fail_at)
                .map(|token| (if token < 2 { 11 } else { 22 }, token))
                .collect::<Vec<_>>();
            assert_eq!(*released.borrow(), expected);
        }
    }

    #[test]
    fn explicit_registration_cleanup_and_drop_do_not_release_twice() {
        for explicit_cleanup in [false, true] {
            let released = RefCell::new(Vec::new());
            {
                let mut registrations = register_handlers(
                    &[11, 22],
                    |signal| Ok((signal, "default")),
                    |signal| Ok((signal, "pending")),
                    |token| {
                        released.borrow_mut().push(token);
                        true
                    },
                )
                .unwrap();
                assert!(released.borrow().is_empty());
                if explicit_cleanup {
                    registrations.clear();
                    registrations.clear();
                }
            }
            assert_eq!(
                *released.borrow(),
                vec![
                    (11, "default"),
                    (11, "pending"),
                    (22, "default"),
                    (22, "pending")
                ]
            );
        }
    }

    #[test]
    fn a_signal_is_deferred_until_the_event_loop_observes_it() {
        let signals = TerminationSignals::install_for(&[SIGUSR1]).expect("install signal handler");
        // Signal masks are inherited independently of installed handlers. Make
        // the blocked case explicit instead of depending on the parent runner.
        let mask = mask::SignalMask::block(SIGUSR1);
        assert_eq!(signals.take(), None);
        assert!(!signals.terminating.load(Ordering::SeqCst));
        signal_hook::low_level::raise(SIGUSR1).expect("raise test signal");
        assert_eq!(signals.take(), None, "blocked signal must remain pending");
        assert!(!signals.terminating.load(Ordering::SeqCst));
        mask.unblock();

        // A successful raise only queues a blocked signal; waiting alone
        // cannot deliver it. Exercise the event loop's polling contract after
        // explicitly unblocking this test signal on this thread,
        // with a deadline so a lost signal still fails instead of hanging.
        let deadline = Instant::now() + Duration::from_secs(5);
        let observed = loop {
            if let Some(signal) = signals.take() {
                break signal;
            }
            assert!(!signals.terminating.load(Ordering::SeqCst));
            assert!(
                Instant::now() < deadline,
                "SIGUSR1 was not observed within 5s after unblocking (inherited blocked={})",
                mask.was_blocked(SIGUSR1)
            );
            thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(observed, SIGUSR1);
        assert!(signals.terminating.load(Ordering::SeqCst));
        assert_eq!(signals.take(), None);
    }

    #[test]
    fn pending_signal_is_consumed_once_and_only_then_arms_termination() {
        // No OS registrations: pin the state transitions independently of
        // platform delivery timing and other tests' process-global handlers.
        let signals = TerminationSignals::install_for(&[]).unwrap();
        for _ in 0..3 {
            assert_eq!(signals.take(), None);
            assert!(!signals.terminating.load(Ordering::SeqCst));
        }
        signals
            .pending
            .store(usize::try_from(SIGUSR1).unwrap(), Ordering::SeqCst);
        assert!(!signals.terminating.load(Ordering::SeqCst));
        assert_eq!(signals.take(), Some(SIGUSR1));
        assert!(signals.terminating.load(Ordering::SeqCst));
        assert_eq!(signals.take(), None);
    }
}
