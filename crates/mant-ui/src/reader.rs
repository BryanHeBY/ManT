//! Explicit reader capabilities; this module never acquires host resources.
use crossterm::event::Event;
use mant_ir::ResolvedContent;
use mant_protocol::{CatalogQuery, DocumentCatalog, DocumentOpenTarget};

use crate::{App, CopyRequest, ExternalUri, UpdateOutcome};

/// Discover an already bounded catalog page.
pub type DiscoverDocuments<'a> = dyn FnMut(&CatalogQuery) -> Result<DocumentCatalog, String> + 'a;
/// Load a typed document target under the embedding host's policy.
pub type OpenDocument<'a> = dyn FnMut(&DocumentOpenTarget) -> Result<ResolvedContent, String> + 'a;
/// Activate a validated external URI.
pub type OpenExternal<'a> = dyn FnMut(&ExternalUri) -> Result<(), String> + 'a;
/// Deliver an explicit copy request.
pub type CopyToClipboard<'a> = dyn FnMut(CopyRequest) -> Result<(), String> + 'a;

/// Capabilities explicitly supplied by the embedding host.
///
/// Missing capabilities report unavailability in the reader; they never fall
/// back to filesystem discovery, process execution, or clipboard access.
#[derive(Default)]
pub struct ReaderServices<'a> {
    /// Catalog discovery, including bounded-page continuation for an empty finder.
    pub discover_documents: Option<&'a mut DiscoverDocuments<'a>>,
    /// Document acquisition without exposing private navigation transactions.
    pub open_document: Option<&'a mut OpenDocument<'a>>,
    /// Host-controlled external link activation.
    pub open_external: Option<&'a mut OpenExternal<'a>>,
    /// Host-controlled clipboard delivery.
    pub copy_to_clipboard: Option<&'a mut CopyToClipboard<'a>>,
}

impl App {
    /// Service pending capabilities once, in discovery/open/external/copy order.
    ///
    /// Returns whether any request was consumed and the reader should redraw.
    /// Failures become notices and do not commit a failed document navigation.
    pub fn service_pending(&mut self, services: &mut ReaderServices<'_>) -> bool {
        let mut unavailable_discovery =
            |_: &CatalogQuery| Err("document discovery is unavailable in this host".to_owned());
        let mut unavailable_open =
            |_: &DocumentOpenTarget| Err("document loading is unavailable in this host".to_owned());
        let mut unavailable_external =
            |_: &ExternalUri| Err("external links are unavailable in this host".to_owned());
        let mut unavailable_copy =
            |_: CopyRequest| Err("clipboard access is unavailable in this host".to_owned());
        let mut redraw = service_discovery_request(
            self,
            services
                .discover_documents
                .as_deref_mut()
                .unwrap_or(&mut unavailable_discovery),
        );
        redraw |= service_open_request(
            self,
            services
                .open_document
                .as_deref_mut()
                .unwrap_or(&mut unavailable_open),
        );
        redraw |= service_external_request(
            self,
            services
                .open_external
                .as_deref_mut()
                .unwrap_or(&mut unavailable_external),
        );
        redraw |= service_copy_request(
            self,
            services
                .copy_to_clipboard
                .as_deref_mut()
                .unwrap_or(&mut unavailable_copy),
        );
        redraw
    }

    /// Route a host-provided event without reading from the terminal.
    pub fn handle_event(&mut self, event: &Event) -> UpdateOutcome {
        match event {
            Event::Key(key) if key.is_press() => self.handle_key(*key),
            Event::Mouse(mouse) => self.handle_mouse(*mouse),
            Event::Resize(_, _) => UpdateOutcome::Redraw,
            Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Key(_) => {
                UpdateOutcome::Unchanged
            }
        }
    }
}

fn service_copy_request<C>(app: &mut App, copy_to_clipboard: &mut C) -> bool
where
    C: FnMut(CopyRequest) -> Result<(), String> + ?Sized,
{
    let Some(request) = app.take_copy_request() else {
        return false;
    };
    let label = request.label();
    match copy_to_clipboard(request) {
        Ok(()) => app.report_copy_success(format!("Copied {label}")),
        Err(message) => app.report_notice(message),
    }
    true
}

fn service_external_request<E>(app: &mut App, open_external: &mut E) -> bool
where
    E: FnMut(&crate::ExternalUri) -> Result<(), String> + ?Sized,
{
    let Some(uri) = app.take_external_request() else {
        return false;
    };
    match open_external(&uri) {
        Ok(()) => app.report_notice(format!("Sent {} to the system opener", uri.as_str())),
        Err(message) => app.report_open_error(message),
    }
    true
}

fn service_discovery_request<D>(app: &mut App, discover_documents: &mut D) -> bool
where
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String> + ?Sized,
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
    D: FnMut(&CatalogQuery) -> Result<DocumentCatalog, String> + ?Sized,
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
    F: FnMut(&DocumentOpenTarget) -> Result<ResolvedContent, String> + ?Sized,
{
    let Some(address) = app.take_open_request() else {
        return false;
    };
    match open_document(&address.document) {
        Ok(bundle) => app.complete_open_shared(std::sync::Arc::new(bundle), address),
        Err(message) => app.report_open_error(message),
    }
    true
}

#[cfg(test)]
mod tests {
    use mant_protocol::{CatalogSchema, DocumentAddress, DocumentSummary};

    use super::*;

    mod capabilities;

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
