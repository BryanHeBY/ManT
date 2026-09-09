//! Explicit host callbacks; UI state never acquires filesystem or clipboard authority.
use super::{
    App, CatalogQuery, CopyRequest, DocumentCatalog, DocumentOpenTarget, Event, ResolvedContent,
    UpdateOutcome,
};
pub(super) fn service_copy_request<C>(app: &mut App, copy_to_clipboard: &mut C) -> bool
where
    C: FnMut(CopyRequest) -> Result<(), String>,
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

pub(super) fn service_external_request<E>(app: &mut App, open_external: &mut E) -> bool
where
    E: FnMut(&crate::ExternalUri) -> Result<(), String>,
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

pub(super) fn service_discovery_request<D>(app: &mut App, discover_documents: &mut D) -> bool
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

pub(super) fn discover_catalog_pages<D>(
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

pub(super) fn service_open_request<F>(app: &mut App, open_document: &mut F) -> bool
where
    F: FnMut(&DocumentOpenTarget) -> Result<ResolvedContent, String>,
{
    let Some(address) = app.take_open_request() else {
        return false;
    };
    match open_document(&address.document) {
        Ok(bundle) => app.complete_open(&bundle, address),
        Err(message) => app.report_open_error(message),
    }
    true
}

pub(super) fn route_event(app: &mut App, event: &Event) -> UpdateOutcome {
    match event {
        Event::Key(key) if key.is_press() => app.handle_key(*key),
        Event::Mouse(mouse) => app.handle_mouse(*mouse),
        Event::Resize(_, _) => UpdateOutcome::Redraw,
        Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Key(_) => {
            UpdateOutcome::Unchanged
        }
    }
}
