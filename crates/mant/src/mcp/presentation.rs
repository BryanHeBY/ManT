//! Compact, bounded presentations for the agent-facing MCP boundary.

use mant_protocol::{DocumentCatalog, QueryExcerpt, QueryOutline, QuerySearch};

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
    }
    page_text(&text, byte)
}

pub(super) fn render_outline(outline: &QueryOutline, byte: u32) -> Result<TextPage, String> {
    page_text(&mant_engine::render_outline_text(outline), byte)
}

pub(super) fn render_excerpt(excerpt: &QueryExcerpt, byte: u32) -> Result<TextPage, String> {
    page_text(&mant_engine::render_excerpt_markdown(excerpt), byte)
}

pub(super) fn render_search(search: &QuerySearch, byte: u32) -> Result<TextPage, String> {
    let mut page = search.clone();
    page.next_offset = None;
    page_text(&mant_engine::render_search_text(&page), byte)
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

fn page_text(text: &str, byte: u32) -> Result<TextPage, String> {
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
