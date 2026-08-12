//! Crossterm lifecycle boundary that always restores the host terminal.

use std::{io, panic, time::Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mant_ast::{DocumentAddress, DocumentCatalog, QueryBundle};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{App, UpdateOutcome};

/// Run the interactive frontend until the user requests exit.
///
/// # Errors
///
/// Returns terminal setup, event, drawing, or restoration errors.
pub fn run(bundle: &QueryBundle) -> io::Result<()> {
    run_with_catalog(
        bundle,
        DocumentCatalog {
            schema: mant_ast::CatalogSchema::V7,
            total: 0,
            returned: 0,
            offset: 0,
            truncated: false,
            next_offset: None,
            documents: Vec::new(),
        },
        |_| Err("document discovery is unavailable in this host".to_owned()),
    )
}

/// Run the frontend with a catalog and a host-owned document loader.
///
/// The UI never reads source configuration, manual paths, or Markdown files;
/// it sends stable catalog addresses back through `open_document`.
///
/// # Errors
///
/// Returns terminal setup, event, drawing, or restoration errors. Document
/// loading failures are shown inside the UI and leave the current page open.
pub fn run_with_catalog<F>(
    bundle: &QueryBundle,
    catalog: DocumentCatalog,
    mut open_document: F,
) -> io::Result<()>
where
    F: FnMut(&DocumentAddress) -> Result<QueryBundle, String>,
{
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    let mut guard = TerminalGuard { active: true };
    // Install the restoration guard before either terminal command can fail.
    // Otherwise an unsupported mouse/alternate-screen sequence could leave the
    // caller in raw mode without ever entering the event loop.
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::with_catalog(bundle, catalog);

    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> io::Result<()> {
        let mut redraw = true;
        while !app.should_quit() {
            let now = Instant::now();
            redraw |= app.tick(now).needs_redraw();
            if redraw {
                terminal.draw(|frame| app.draw(frame))?;
                redraw = false;
            }
            let Some(timeout) = app.next_wakeup(Instant::now()) else {
                redraw |= route_event(&mut app, &event::read()?).needs_redraw();
                redraw |= service_open_request(&mut app, &mut open_document);
                continue;
            };
            if !event::poll(timeout)? {
                continue;
            }
            redraw |= route_event(&mut app, &event::read()?).needs_redraw();
            redraw |= service_open_request(&mut app, &mut open_document);
        }
        Ok(())
    }));

    let restore_result = guard.restore();
    match result {
        Ok(run_result) => run_result.and(restore_result),
        Err(payload) => {
            let _ = restore_result;
            panic::resume_unwind(payload);
        }
    }
}

fn service_open_request<F>(app: &mut App, open_document: &mut F) -> bool
where
    F: FnMut(&DocumentAddress) -> Result<QueryBundle, String>,
{
    let Some(address) = app.take_open_request() else {
        return false;
    };
    match open_document(address.address()) {
        Ok(bundle) => app.complete_open(&bundle, address),
        Err(message) => app.report_open_error(message),
    }
    true
}

fn route_event(app: &mut App, event: &Event) -> UpdateOutcome {
    match event {
        Event::Key(key) if key.is_press() => app.handle_key(*key),
        Event::Mouse(mouse) => app.handle_mouse(*mouse),
        Event::Resize(_, _) => UpdateOutcome::Redraw,
        Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Key(_) => {
            UpdateOutcome::Unchanged
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
