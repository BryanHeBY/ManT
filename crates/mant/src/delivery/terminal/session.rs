//! Native terminal acquisition, event loop and restoration in original order.
use super::{
    App, CrosstermBackend, DisableMouseCapture, DocumentCatalog, EnableMouseCapture,
    EnterAlternateScreen, Instant, LeaveAlternateScreen, ReaderServices, ResolvedContent,
    TERMINATION_POLL_INTERVAL, Terminal, TerminationSignals, disable_raw_mode, enable_raw_mode,
    event, execute, io, panic,
};
pub(super) fn run(
    bundle: &ResolvedContent,
    catalog: DocumentCatalog,
    scope: &[ResolvedContent],
    services: &mut ReaderServices<'_>,
) -> io::Result<()> {
    let termination = TerminationSignals::install()?;
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    let mut guard = TerminalGuard { active: true };
    // Install the restoration guard before either terminal command can fail.
    // Otherwise an unsupported mouse/alternate-screen sequence could leave the
    // caller in raw mode without ever entering the event loop.
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::with_catalog_and_scope(bundle, catalog, scope);

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

    let restore_result = guard.restore();
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

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn restore(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        }
    }
}
