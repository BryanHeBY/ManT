//! Compact, bounded presentations for the agent-facing MCP boundary.

use mant_protocol::{
    DocumentCatalog, QueryExcerpt, QueryOutline, QuerySearch, ScopeQueryResponse, ScopeQueryResult,
    TraversalLimit,
};

use crate::arguments::QueryFormat;

/// Maximum UTF-8 bytes returned by one successful tool call.
pub(super) const MAX_OUTPUT_BYTES: usize = 32 * 1024;
const CURSOR_FOOTER_RESERVE: usize = 320;
const PAGE_BODY_BYTES: usize = MAX_OUTPUT_BYTES - CURSOR_FOOTER_RESERVE;

/// One bounded rendered page before an opaque continuation token is attached.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TextPage {
    pub(super) text: String,
    pub(super) next_byte: Option<u32>,
}

pub(super) fn render_find(catalog: &DocumentCatalog, byte: u32) -> Result<TextPage, String> {
    let mut text = format!("{} matches", catalog.total);
    let records = mant_protocol::render_catalog_text(catalog, false);
    if !records.is_empty() {
        text.push('\n');
        text.push_str(records.trim_end());
    } else if let Some(coverage) = mant_protocol::render_catalog_coverage_text(catalog) {
        text.push_str("; ");
        text.push_str(&coverage.replace('\n', "; "));
    }
    page_text(&text, byte)
}

pub(super) fn render_outline(outline: &QueryOutline, byte: u32) -> Result<TextPage, String> {
    page_text(&mant_engine::render_outline_text(outline), byte)
}

pub(super) fn render_excerpt(excerpt: &QueryExcerpt, byte: u32) -> Result<TextPage, String> {
    page_text(&mant_engine::render_excerpt_markdown(excerpt), byte)
}

pub(super) fn render_scope_explain(
    response: &ScopeQueryResponse,
    byte: u32,
) -> Result<TextPage, String> {
    let ScopeQueryResult::Explain {
        entry,
        matches,
        missed,
        failures,
    } = &response.result
    else {
        return Err("scope response does not contain an explanation".to_owned());
    };
    let mut text = crate::presentation::render_scope_query_result(
        response,
        QueryFormat::Markdown,
        false,
        false,
        false,
    )
    .map_err(crate::error::Failure::into_message)?;
    if matches.is_empty() && failures.is_empty() {
        text = format!(
            "0 matches for semantic entry `{entry}` across {} documents",
            response.scope.documents.len()
        );
    }
    append_status_line(
        &mut text,
        &format!(
            "[explain: matched={}, missed={missed}, failed={}]",
            matches.len(),
            failures.len()
        ),
    );
    append_scope_status(&mut text, response);
    page_text(&text, byte)
}

pub(super) fn render_scope_search(
    response: &ScopeQueryResponse,
    byte: u32,
) -> Result<TextPage, String> {
    let ScopeQueryResult::Search { search } = &response.result else {
        return Err("scope response does not contain search results".to_owned());
    };
    let mut text = crate::presentation::render_scope_query_result(
        response,
        QueryFormat::Text,
        false,
        false,
        false,
    )
    .map_err(crate::error::Failure::into_message)?;
    if search.returned == 0 {
        text = format!(
            "0 matches across {} documents",
            response.scope.documents.len()
        );
    }
    append_scope_status(&mut text, response);
    page_text(&text, byte)
}

fn append_scope_status(text: &mut String, response: &ScopeQueryResponse) {
    let unresolved_roots = response
        .scope
        .unresolved
        .iter()
        .filter(|failure| failure.from.is_none())
        .count();
    let unresolved_links = response
        .scope
        .unresolved
        .len()
        .saturating_sub(unresolved_roots);
    let depth_frontier = response
        .scope
        .frontier
        .iter()
        .filter(|edge| edge.limit == TraversalLimit::MaxDepth)
        .count();
    let budget_frontier = response.scope.frontier.len().saturating_sub(depth_frontier);
    if !response.scope.query.traversal.follow_links
        && unresolved_roots == 0
        && unresolved_links == 0
    {
        return;
    }
    append_status_line(
        text,
        &format!(
            "[scope: documents={}, unresolved-roots={unresolved_roots}, unresolved-links={unresolved_links}, depth-frontier={depth_frontier}, budget-frontier={budget_frontier}]",
            response.scope.documents.len()
        ),
    );
}

fn append_status_line(text: &mut String, status: &str) {
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(status);
}

/// Attach the only transport-specific framing used by successful results.
pub(super) fn finish_page(mut page: TextPage, cursor: Option<&str>) -> String {
    if let Some(cursor) = cursor {
        if !page.text.is_empty() {
            page.text.push_str("\n\n");
        }
        page.text.push_str("[more cursor=");
        page.text.push_str(cursor);
        page.text.push(']');
    }
    debug_assert!(page.text.len() <= MAX_OUTPUT_BYTES);
    page.text
}

pub(super) fn page_text(text: &str, byte: u32) -> Result<TextPage, String> {
    let start = usize::try_from(byte).map_err(|_| "cursor position is too large".to_owned())?;
    if start > text.len() || !text.is_char_boundary(start) {
        return Err("cursor no longer addresses this result; restart without it".to_owned());
    }
    if start == text.len() {
        return Ok(TextPage {
            text: String::new(),
            next_byte: None,
        });
    }

    let hard_end = text.len().min(start.saturating_add(PAGE_BODY_BYTES));
    let mut end = floor_char_boundary(text, hard_end);
    if end < text.len() {
        let minimum = start + (end - start) / 2;
        if let Some(boundary) = text[minimum..end].rfind("\n\n") {
            end = minimum + boundary + 2;
        } else if let Some(boundary) = text[minimum..end].rfind('\n') {
            end = minimum + boundary + 1;
        }
    }
    if end == start {
        end = floor_char_boundary(text, hard_end.max(start + 1));
    }

    Ok(TextPage {
        text: text[start..end].to_owned(),
        next_byte: (end < text.len()).then(|| u32::try_from(end).unwrap_or(u32::MAX)),
    })
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub(super) fn prepare_excerpt(excerpt: &mut QueryExcerpt) {
    excerpt.diagnostics.clear();
    discard_document_source_path(&mut excerpt.source);
    for selection in &mut excerpt.selections {
        if let mant_protocol::ExcerptSelection::Tldr { document, .. } = selection {
            document.source_path.clear();
        }
    }
}

pub(super) fn prepare_outline(outline: &mut QueryOutline) {
    outline.diagnostics.clear();
    discard_document_source_path(&mut outline.source);
}

pub(super) fn prepare_search(search: &mut QuerySearch) {
    discard_document_source_path(&mut search.source);
}

pub(super) fn prepare_scope(response: &mut ScopeQueryResponse) {
    for unresolved in &mut response.scope.unresolved {
        "document could not be resolved".clone_into(&mut unresolved.reason);
    }
    match &mut response.result {
        ScopeQueryResult::Explain { matches, .. } => {
            for found in matches {
                prepare_excerpt(&mut found.excerpt);
            }
        }
        ScopeQueryResult::Search { search } => {
            for found in &mut search.documents {
                prepare_search(&mut found.search);
            }
        }
    }
}

fn discard_document_source_path(source: &mut Option<mant_ir::DocumentSource>) {
    if let Some(source) = source {
        source.path = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_OUTPUT_BYTES, finish_page, page_text};

    #[test]
    fn text_pages_are_utf8_safe_bounded_and_continuable() {
        let source = "段落 → content\n\n".repeat(4_000);
        let first = page_text(&source, 0).expect("first page");
        let next = first.next_byte.expect("continuation");
        let token = "c1-r-0000000000000000-0000000000000001";
        let rendered = finish_page(first, Some(token));
        assert!(rendered.len() <= MAX_OUTPUT_BYTES);
        assert!(rendered.ends_with(&format!("[more cursor={token}]")));

        let second = page_text(&source, next).expect("second page");
        assert!(!second.text.is_empty());
    }

    #[test]
    fn text_pages_preserve_all_whitespace_across_continuations() {
        let source = "code  \n\tindented\n\n".repeat(4_000);
        let mut reconstructed = String::new();
        let mut byte = 0;
        loop {
            let page = page_text(&source, byte).expect("page");
            reconstructed.push_str(&page.text);
            let Some(next) = page.next_byte else {
                break;
            };
            byte = next;
        }

        assert_eq!(reconstructed, source);
    }

    #[test]
    fn stale_byte_positions_are_rejected() {
        assert!(page_text("é", 1).is_err());
        assert!(page_text("short", 99).is_err());
    }
}
