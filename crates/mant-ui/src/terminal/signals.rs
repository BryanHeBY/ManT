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

/// Convert async signals into state polled by the ordinary Rust event loop.
///
/// Signal handlers only touch lock-free atomics. Terminal restoration and
/// default signal emulation therefore happen outside signal context.
pub(super) struct TerminationSignals {
    pending: Arc<AtomicUsize>,
    terminating: Arc<AtomicBool>,
    registrations: Vec<SigId>,
}

impl TerminationSignals {
    pub(super) fn install() -> io::Result<Self> {
        Self::install_for(TERMINATION_SIGNALS)
    }

    fn install_for(signals: &[i32]) -> io::Result<Self> {
        let pending = Arc::new(AtomicUsize::new(0));
        let terminating = Arc::new(AtomicBool::new(false));
        let mut registrations = Vec::with_capacity(signals.len() * 2);
        for &signal in signals {
            // This handler runs first. A second termination signal after the
            // event loop begins cleanup gets the platform's default action.
            registrations.push(signal_hook::flag::register_conditional_default(
                signal,
                Arc::clone(&terminating),
            )?);
            registrations.push(signal_hook::flag::register_usize(
                signal,
                Arc::clone(&pending),
                usize::try_from(signal).expect("POSIX signal numbers are positive"),
            )?);
        }
        Ok(Self {
            pending,
            terminating,
            registrations,
        })
    }

    pub(super) fn take(&self) -> Option<i32> {
        let signal = self.pending.swap(0, Ordering::SeqCst);
        if signal == 0 {
            return None;
        }
        self.terminating.store(true, Ordering::SeqCst);
        i32::try_from(signal).ok()
    }

    pub(super) fn terminate(mut self, signal: i32) -> io::Result<()> {
        self.unregister();
        signal_hook::low_level::emulate_default_handler(signal)
    }

    fn unregister(&mut self) {
        for registration in self.registrations.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}

impl Drop for TerminationSignals {
    fn drop(&mut self) {
        self.unregister();
    }
}

#[cfg(test)]
mod tests {
    use signal_hook::consts::signal::SIGUSR1;

    use super::TerminationSignals;

    #[test]
    fn a_signal_is_deferred_until_the_event_loop_observes_it() {
        let signals = TerminationSignals::install_for(&[SIGUSR1]).expect("install signal handler");
        signal_hook::low_level::raise(SIGUSR1).expect("raise test signal");

        assert_eq!(signals.take(), Some(SIGUSR1));
        assert_eq!(signals.take(), None);
    }
}
