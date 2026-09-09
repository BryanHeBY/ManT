//! Native terminal acquisition, event loop and restoration in original order.
use super::host::{
    route_event, service_copy_request, service_discovery_request, service_external_request,
    service_open_request,
};
use super::{
    App, CatalogQuery, CopyRequest, CrosstermBackend, DisableMouseCapture, DocumentCatalog,
    DocumentOpenTarget, EnableMouseCapture, EnterAlternateScreen, Instant, LeaveAlternateScreen,
    ResolvedContent, TERMINATION_POLL_INTERVAL, Terminal, TerminationSignals, disable_raw_mode,
    enable_raw_mode, event, execute, io, panic,
};
pub(super) fn run<D, F, E, C>(
    bundle: &ResolvedContent,
    catalog: DocumentCatalog,
    scope: &[ResolvedContent],
    mut discover_documents: D,
    mut open_document: F,
    mut open_external: E,
    mut copy_to_clipboard: C,
) -> io::Result<()>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
    F: FnMut(&DocumentOpenTarget) -> Result<ResolvedContent, String>,
    E: FnMut(&crate::ExternalUri) -> Result<(), String>,
    C: FnMut(CopyRequest) -> Result<(), String>,
{
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
            redraw |= route_event(&mut app, &event::read()?).needs_redraw();
            redraw |= service_discovery_request(&mut app, &mut discover_documents);
            redraw |= service_open_request(&mut app, &mut open_document);
            redraw |= service_external_request(&mut app, &mut open_external);
            redraw |= service_copy_request(&mut app, &mut copy_to_clipboard);
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
