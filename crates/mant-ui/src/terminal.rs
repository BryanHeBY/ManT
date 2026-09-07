//! Crossterm lifecycle boundary that always restores the host terminal.

use std::{
    io, panic,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mant_ir::ResolvedContent;
use mant_protocol::{CatalogQuery, DocumentAddress, DocumentCatalog};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{App, CopyRequest, UpdateOutcome};

#[cfg(unix)]
pub(crate) mod signals;
#[cfg(unix)]
use signals::TerminationSignals;

mod host;
mod session;
#[cfg(test)]
use host::discover_catalog_pages;

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
    discover_documents: D,
    open_document: F,
    open_external: E,
) -> io::Result<()>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
    F: FnMut(&DocumentAddress) -> Result<ResolvedContent, String>,
    E: FnMut(&crate::ExternalUri) -> Result<(), String>,
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
pub fn run_with_catalog_and_scope<D, F, E>(
    bundle: &ResolvedContent,
    catalog: DocumentCatalog,
    scope: &[ResolvedContent],
    discover_documents: D,
    open_document: F,
    open_external: E,
) -> io::Result<()>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
    F: FnMut(&DocumentAddress) -> Result<ResolvedContent, String>,
    E: FnMut(&crate::ExternalUri) -> Result<(), String>,
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
pub fn run_with_catalog_and_scope_and_copy<D, F, E, C>(
    bundle: &ResolvedContent,
    catalog: DocumentCatalog,
    scope: &[ResolvedContent],
    discover_documents: D,
    open_document: F,
    open_external: E,
    copy_to_clipboard: C,
) -> io::Result<()>
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String>,
    F: FnMut(&DocumentAddress) -> Result<ResolvedContent, String>,
    E: FnMut(&crate::ExternalUri) -> Result<(), String>,
    C: FnMut(CopyRequest) -> Result<(), String>,
{
    session::run(
        bundle,
        catalog,
        scope,
        discover_documents,
        open_document,
        open_external,
        copy_to_clipboard,
    )
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
                schema: CatalogSchema::V0Dot11,
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
                schema: CatalogSchema::V0Dot11,
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
