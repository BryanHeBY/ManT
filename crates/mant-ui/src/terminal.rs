//! Crossterm lifecycle boundary that always restores the host terminal.

use std::{io, panic, time::Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mant_ir::ResolvedContent;
use mant_protocol::{CatalogQuery, DocumentAddress, DocumentCatalog};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{App, UpdateOutcome};

/// Run the interactive frontend until the user requests exit.
///
/// # Errors
///
/// Returns terminal setup, event, drawing, or restoration errors.
pub fn run(bundle: &ResolvedContent) -> io::Result<()> {
    run_with_catalog(
        bundle,
        DocumentCatalog::default(),
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
    bundle: &ResolvedContent,
    catalog: DocumentCatalog,
    mut discover_documents: D,
    mut open_document: F,
    mut open_external: E,
) -> io::Result<()>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
    F: FnMut(&DocumentAddress) -> Result<ResolvedContent, String>,
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
    match discover_catalog_pages(&query, discover_documents) {
        Ok(catalog) => app.complete_discovery(catalog),
        Err(message) => app.report_discovery_error(message),
    }
    true
}

fn discover_catalog_pages<D>(
    query: &CatalogQuery,
    discover_documents: &mut D,
) -> Result<DocumentCatalog, String>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
{
    let mut catalog = discover_documents(query)?;
    if query.pattern.is_some() {
        return Ok(catalog);
    }
    let mut previous_offset = query.offset;
    while let Some(next_offset) = catalog.next_offset {
        if next_offset <= previous_offset {
            return Err("document discovery returned a non-advancing page".to_owned());
        }
        let mut next_query = query.clone();
        next_query.offset = next_offset;
        let page = discover_documents(&next_query)?;
        if page.offset != next_offset
            || page.schema != catalog.schema
            || page.total != catalog.total
        {
            return Err("document discovery returned inconsistent catalog pages".to_owned());
        }
        catalog.documents.extend(page.documents);
        catalog.returned = u32::try_from(catalog.documents.len()).unwrap_or(u32::MAX);
        catalog.truncated = page.truncated;
        catalog.next_offset = page.next_offset;
        previous_offset = next_offset;
    }
    Ok(catalog)
}

fn service_open_request<F>(app: &mut App, open_document: &mut F) -> bool
where
    F: FnMut(&DocumentAddress) -> Result<ResolvedContent, String>,
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

#[cfg(test)]
mod tests {
    use mant_protocol::{CatalogSchema, DocumentSummary};

    use super::*;

    #[test]
    fn empty_finder_queries_collect_every_catalog_page() {
        let mut offsets = Vec::new();
        let catalog = discover_catalog_pages(&CatalogQuery::default(), &mut |query| {
            offsets.push(query.offset);
            let next_offset = (query.offset == 0).then_some(1);
            Ok(DocumentCatalog {
                schema: CatalogSchema::V0Dot8,
                query: query.clone(),
                coverage: mant_protocol::CatalogCoverage::default(),
                total: 2,
                returned: 1,
                offset: query.offset,
                truncated: next_offset.is_some(),
                next_offset,
                documents: vec![manual_summary(if query.offset == 0 {
                    "git"
                } else {
                    "man"
                })],
            })
        })
        .expect("collect catalog");

        assert_eq!(offsets, [0, 1]);
        assert_eq!(catalog.returned, 2);
        assert!(!catalog.truncated);
        assert_eq!(
            catalog
                .documents
                .iter()
                .map(|document| document.address.name())
                .collect::<Vec<_>>(),
            ["git", "man"]
        );
    }

    #[test]
    fn live_finder_queries_keep_the_bounded_ranked_page() {
        let mut calls = 0;
        let query = CatalogQuery {
            pattern: Some("man".to_owned()),
            ..CatalogQuery::default()
        };
        let catalog = discover_catalog_pages(&query, &mut |_| {
            calls += 1;
            Ok(DocumentCatalog {
                schema: CatalogSchema::V0Dot8,
                query: query.clone(),
                coverage: mant_protocol::CatalogCoverage::default(),
                total: 20_000,
                returned: 1,
                offset: 0,
                truncated: true,
                next_offset: Some(1),
                documents: vec![manual_summary("man")],
            })
        })
        .expect("load ranked page");

        assert_eq!(calls, 1);
        assert_eq!(catalog.returned, 1);
        assert!(catalog.truncated);
    }

    fn manual_summary(name: &str) -> DocumentSummary {
        DocumentSummary {
            address: DocumentAddress::Manual {
                name: name.to_owned(),
                manual_section: "1".to_owned(),
            },
        }
    }
}
