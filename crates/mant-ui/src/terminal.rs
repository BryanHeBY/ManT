//! Crossterm lifecycle boundary that always restores the host terminal.

use std::{io, panic, time::Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mant_ast::{CatalogQuery, DocumentAddress, DocumentCatalog, QueryBundle};
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
        |_| Err("document discovery is unavailable in this host".to_owned()),
        |_| Err("external links are unavailable in this host".to_owned()),
    )
}

/// Run the frontend with an initial catalog page and host-owned discovery and
/// document loading.
///
/// The UI never reads source configuration, manual paths, or Markdown files;
/// it sends bounded catalog queries through `discover_documents` and stable
/// catalog addresses through `open_document`. Safe external URI activation is
/// delegated through `open_external`, so the embedding host retains control of
/// platform integration and policy.
///
/// # Errors
///
/// Returns terminal setup, event, drawing, or restoration errors. Document
/// loading failures are shown inside the UI and leave the current page open.
pub fn run_with_catalog<D, F, E>(
    bundle: &QueryBundle,
    catalog: DocumentCatalog,
    mut discover_documents: D,
    mut open_document: F,
    mut open_external: E,
) -> io::Result<()>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
    F: FnMut(&DocumentAddress) -> Result<QueryBundle, String>,
    E: FnMut(&str) -> Result<(), String>,
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
                redraw |= service_discovery_request(&mut app, &mut discover_documents);
                redraw |= service_open_request(&mut app, &mut open_document);
                redraw |= service_external_request(&mut app, &mut open_external);
                continue;
            };
            if !event::poll(timeout)? {
                continue;
            }
            redraw |= route_event(&mut app, &event::read()?).needs_redraw();
            redraw |= service_discovery_request(&mut app, &mut discover_documents);
            redraw |= service_open_request(&mut app, &mut open_document);
            redraw |= service_external_request(&mut app, &mut open_external);
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

fn service_external_request<E>(app: &mut App, open_external: &mut E) -> bool
where
    E: FnMut(&str) -> Result<(), String>,
{
    let Some(uri) = app.take_external_request() else {
        return false;
    };
    match open_external(&uri) {
        Ok(()) => app.report_notice(format!("Opened {uri}")),
        Err(message) => app.report_open_error(message),
    }
    true
}

fn service_discovery_request<D>(app: &mut App, discover_documents: &mut D) -> bool
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
{
    let Some(query) = app.take_discovery_request() else {
        return false;
    };
    match discover_documents(&query) {
        Ok(catalog) => app.complete_discovery(catalog),
        Err(message) => app.report_discovery_error(message),
    }
    true
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
