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
use mant_ir::ResolvedContent;
use mant_protocol::{CatalogQuery, DocumentCatalog, DocumentOpenTarget};
use ratatui::{Terminal, backend::CrosstermBackend};

use mant_ui::{App, CopyRequest, ExternalUri, ReaderServices};

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

/// Run the interactive frontend until the user requests exit.
///
/// # Errors
///
/// Returns terminal setup, event, drawing, or restoration errors.
#[cfg(all(test, unix))]
pub(crate) fn run(bundle: &ResolvedContent) -> io::Result<()> {
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
/// it sends bounded catalog queries through `discover_documents` and typed
/// local document targets through `open_document`. Unqualified manual targets
/// retain their absent section and require manual-only host resolution.
/// Safe external URI activation is
/// delegated through `open_external`, so the embedding host retains control of
/// platform integration and policy.
///
/// # Errors
///
/// Returns terminal setup, event, drawing, or restoration errors. Document
/// loading failures are shown inside the UI and leave the current page open.
#[cfg(all(test, unix))]
pub(crate) fn run_with_catalog<D, F, E>(
    bundle: &ResolvedContent,
    catalog: DocumentCatalog,
    discover_documents: D,
    open_document: F,
    open_external: E,
) -> io::Result<()>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
    F: FnMut(&DocumentOpenTarget) -> Result<ResolvedContent, String>,
    E: FnMut(&ExternalUri) -> Result<(), String>,
{
    run_with_catalog_and_scope(
        bundle,
        catalog,
        std::slice::from_ref(bundle),
        discover_documents,
        open_document,
        open_external,
    )
}

/// Run the frontend with a pre-resolved document set used by interactive text
/// search. The first `bundle` remains the initial page; the catalog finder is
/// still global and is not restricted to this scope.
///
/// # Errors
///
/// Returns terminal setup, input, drawing, or restoration failures.
#[cfg(all(test, unix))]
pub(crate) fn run_with_catalog_and_scope<D, F, E>(
    bundle: &ResolvedContent,
    catalog: DocumentCatalog,
    scope: &[ResolvedContent],
    discover_documents: D,
    open_document: F,
    open_external: E,
) -> io::Result<()>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
    F: FnMut(&DocumentOpenTarget) -> Result<ResolvedContent, String>,
    E: FnMut(&ExternalUri) -> Result<(), String>,
{
    run_with_catalog_and_scope_and_copy(
        bundle,
        catalog,
        scope,
        discover_documents,
        open_document,
        open_external,
        |_| Err("clipboard access is unavailable in this host".to_owned()),
    )
}

/// Run the frontend with host-owned clipboard integration in addition to
/// discovery, document loading, external links, and scoped search.
///
/// Selection requests already contain terminal-safe plain text. Semantic node
/// requests retain the complete resolved document plus its stable selector so
/// the host can render the requested format without reconstructing UI state.
///
/// # Errors
///
/// Returns terminal setup, input, drawing, or restoration failures. Clipboard
/// failures are shown inside the UI and leave the current selection intact.
pub(crate) fn run_with_catalog_and_scope_and_copy<D, F, E, C>(
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
    E: FnMut(&ExternalUri) -> Result<(), String>,
    C: FnMut(CopyRequest) -> Result<(), String>,
{
    session::run(
        bundle,
        catalog,
        scope,
        &mut ReaderServices {
            discover_documents: Some(&mut discover_documents),
            open_document: Some(&mut open_document),
            open_external: Some(&mut open_external),
            copy_to_clipboard: Some(&mut copy_to_clipboard),
        },
    )
}

#[cfg(all(test, unix))]
mod tests;
