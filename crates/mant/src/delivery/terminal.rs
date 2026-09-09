//! Crossterm lifecycle boundary that always restores the host terminal.

use std::{
    io, panic,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use mant_ui::{App, ReaderOptions, ReaderServices};

#[cfg(unix)]
pub(crate) mod signals;
#[cfg(unix)]
use signals::TerminationSignals;

mod session;

const TERMINATION_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[cfg(not(unix))]
struct TerminationSignals;

// Keep the event-loop boundary identical to the Unix signal adapter. Windows
// has no POSIX termination registrations or deferred signal to consume, so
// these deliberately fallible, receiver-based operations are no-ops there.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps, clippy::unused_self)]
impl TerminationSignals {
    fn install() -> io::Result<Self> {
        Ok(Self)
    }

    const fn take(&self) -> Option<i32> {
        None
    }

    fn terminate(self, _signal: i32) -> io::Result<()> {
        Ok(())
    }
}

/// Run a process-owned terminal around the caller's existing snapshots.
pub(crate) fn run_reader(
    options: ReaderOptions,
    services: &mut ReaderServices<'_>,
) -> io::Result<()> {
    session::run(options, services)
}

#[cfg(all(test, unix))]
mod tests;
