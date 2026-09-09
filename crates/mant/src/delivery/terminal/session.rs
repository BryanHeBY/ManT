//! Original TUI acquisition/event order with one shared restoration ledger.
use super::{
    App, Instant, ReaderOptions, ReaderServices, TERMINATION_POLL_INTERVAL, Terminal,
    TerminationSignals,
    backend::{self, CrosstermTerminalOps, LeasedBackend},
    event, io, panic,
};
use crate::delivery::terminal_lease::TerminalLease;
use std::{cell::RefCell, rc::Rc};
pub(super) fn run(options: ReaderOptions, services: &mut ReaderServices<'_>) -> io::Result<()> {
    let termination = TerminationSignals::install()?;
    let lease = Rc::new(RefCell::new(
        TerminalLease::new(CrosstermTerminalOps::new()),
    ));
    backend::acquire_tui(&lease)?;
    let backend = LeasedBackend::new(io::stdout(), Rc::clone(&lease));
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::from_shared(options);

    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> io::Result<Option<i32>> {
        let mut redraw = true;
        loop {
            if let Some(signal) = termination.take() {
                return Ok(Some(signal));
            }
            if app.should_quit() {
                return Ok(None);
            }
            let now = Instant::now();
            redraw |= app.tick(now).needs_redraw();
            if redraw {
                terminal.draw(|frame| app.draw(frame))?;
                redraw = false;
            }
            let timeout = app
                .next_wakeup(Instant::now())
                .map_or(TERMINATION_POLL_INTERVAL, |timeout| {
                    timeout.min(TERMINATION_POLL_INTERVAL)
                });
            if !event::poll(timeout)? {
                continue;
            }
            redraw |= app.handle_event(&event::read()?).needs_redraw();
            redraw |= app.service_pending(services);
        }
    }));

    let restore_result = backend::restore(&lease);
    match result {
        Ok(Ok(Some(signal))) => {
            let signal_result = termination.terminate(signal);
            restore_result.and(signal_result)
        }
        Ok(Ok(None)) => restore_result,
        Ok(Err(error)) => Err(error),
        Err(payload) => {
            let _ = restore_result;
            panic::resume_unwind(payload);
        }
    }
}
