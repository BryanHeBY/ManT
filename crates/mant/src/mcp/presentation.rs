//! Compact, bounded presentations for the agent-facing MCP boundary.

use std::fmt::Write as _;

use mant_protocol::{
    DocumentCatalog, QueryExcerpt, QueryOutline, ScopeQueryResponse, ScopeQueryResult,
    TraversalLimit, sanitize_terminal_text,
};

use super::params::PageRequest;
use crate::arguments::QueryFormat;

/// One stateless character page of a canonical rendered result.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TextPage {
    pub(super) text: String,
    pub(super) start_char: usize,
    pub(super) end_char: usize,
    pub(super) total_chars: usize,
}

pub(super) fn render_find(catalog: &DocumentCatalog, page: PageRequest) -> TextPage {
    let mut text = format!("{} matches", catalog.total);
    if catalog.offset != 0 || catalog.returned < catalog.total {
        let _ = write!(
            text,
            "; offset={}, returned={}",
            catalog.offset, catalog.returned
        );
    }
    if let Some(next_offset) = catalog.next_offset {
        let _ = write!(text, ", nextOffset={next_offset}");
    }
    let records = mant_protocol::render_catalog_text(catalog, false);
    if !records.is_empty() {
        text.push('\n');
        text.push_str(records.trim_end());
    } else if let Some(coverage) = mant_protocol::render_catalog_coverage_text(catalog) {
        text.push_str("; ");
        text.push_str(&coverage.replace('\n', "; "));
    }
    page_text(&text, page)
}

pub(super) fn render_outline(outline: &QueryOutline, page: PageRequest) -> TextPage {
    page_text(&mant_engine::render_outline_text(outline), page)
}

pub(super) fn render_excerpt(excerpt: &QueryExcerpt, page: PageRequest) -> TextPage {
    page_text(&mant_engine::render_excerpt_markdown(excerpt), page)
}

pub(super) fn render_scope_explain(
    response: &ScopeQueryResponse,
    page: PageRequest,
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
        crate::presentation::RenderOptions {
            format: QueryFormat::Markdown,
            pretty: false,
            preserve_anchors: false,
            color: false,
            target: crate::presentation::OutputTarget::Stream,
        },
    )
    .map_err(crate::error::Failure::into_message)?;
    if matches.is_empty() && failures.is_empty() {
        let document = response.scope.documents.first().map_or_else(
            || "DOCUMENT".to_owned(),
            |document| document.address.catalog_path(),
        );
        let document = serde_json::to_string(&document)
            .expect("serializing a String for an MCP hint cannot fail");
        let entry =
            serde_json::to_string(entry).expect("serializing a String for an MCP hint cannot fail");
        text = format!(
            "0 matches for semantic entry {entry} across {} documents\n\
             Next: call mant_outline(document={document}, entries={{\"kind\":\"all\"}}) for \
             available selectors, repeating it for other resolved documents; call mant_search \
             with the same documents and pattern={entry} when the term may occur only in prose.",
            response.scope.documents.len(),
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
    Ok(page_text(&text, page))
}

pub(super) fn render_scope_search(
    response: &ScopeQueryResponse,
    page: PageRequest,
) -> Result<TextPage, String> {
    let ScopeQueryResult::Search { search } = &response.result else {
        return Err("scope response does not contain search results".to_owned());
    };
    let mut text = crate::presentation::render_scope_query_result(
        response,
        crate::presentation::RenderOptions {
            format: QueryFormat::Text,
            pretty: false,
            preserve_anchors: false,
            color: false,
            target: crate::presentation::OutputTarget::Stream,
        },
    )
    .map_err(crate::error::Failure::into_message)?;
    if search.returned == 0 {
        text = format!(
            "0 matches across {} documents",
            response.scope.documents.len()
        );
    }
    append_status_line(&mut text, &search_status(search));
    append_scope_status(&mut text, response);
    Ok(page_text(&text, page))
}

fn search_status(search: &mant_protocol::ScopeSearch) -> String {
    let mut status = format!(
        "[search: offset={}, returned={}, totalMatchingLineGroups={}",
        search.offset, search.returned, search.total
    );
    if let Some(next_offset) = search.next_offset {
        let _ = write!(status, ", nextOffset={next_offset}");
    }
    status.push(']');
    status
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
    let document_frontier = response
        .scope
        .frontier
        .iter()
        .filter(|edge| edge.limit == TraversalLimit::MaxDocuments)
        .count();
    let content_frontier = response
        .scope
        .frontier
        .iter()
        .filter(|edge| edge.limit == TraversalLimit::MaxContentBytes)
        .count();
    if !response.scope.query.traversal.follow_links
        && unresolved_roots == 0
        && unresolved_links == 0
    {
        return;
    }
    append_status_line(
        text,
        &format!(
            "[scope: documents={}, unresolved-roots={unresolved_roots}, unresolved-links={unresolved_links}, depth-frontier={depth_frontier}, document-frontier={document_frontier}, content-frontier={content_frontier}]",
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

/// Attach the stable, model-visible page metadata to a successful result.
pub(super) fn finish_page(page: &TextPage) -> String {
    let mut output = format!(
        "[mant-page chars={}..{} totalChars={}",
        page.start_char, page.end_char, page.total_chars
    );
    if page.end_char < page.total_chars {
        let _ = write!(output, " nextChar={}", page.end_char);
    }
    output.push(']');
    if !page.text.is_empty() {
        output.push_str("\n\n");
        output.push_str(&page.text);
    }
    output
}

pub(super) fn page_text(text: &str, page: PageRequest) -> TextPage {
    // MCP success bodies are model-visible protocol data. Sanitize the whole
    // canonical body here so every tool and every dynamically rendered
    // identity shares the same control-character boundary before paging.
    let text = sanitize_model_text(text);
    let text = text.as_str();
    let total_chars = text.chars().count();
    let requested_start = usize::try_from(page.start_char).unwrap_or(usize::MAX);
    let start_char = requested_start.min(total_chars);
    let max_chars = usize::try_from(page.max_chars).unwrap_or(usize::MAX);
    let end_char = start_char.saturating_add(max_chars).min(total_chars);
    let start_byte = char_offset_to_byte(text, start_char, total_chars);
    let end_byte = char_offset_to_byte(text, end_char, total_chars);
    TextPage {
        text: text[start_byte..end_byte].to_owned(),
        start_char,
        end_char,
        total_chars,
    }
}

fn sanitize_model_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\t') {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

fn char_offset_to_byte(text: &str, offset: usize, total_chars: usize) -> usize {
    if offset >= total_chars {
        return text.len();
    }
    text.char_indices()
        .nth(offset)
        .map_or(text.len(), |(byte, _)| byte)
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

pub(super) fn prepare_scope(response: &mut ScopeQueryResponse) {
    for unresolved in &mut response.scope.unresolved {
        "document could not be resolved".clone_into(&mut unresolved.reason);
    }
    match &mut response.result {
        ScopeQueryResult::Explain {
            matches, failures, ..
        } => {
            for found in matches {
                prepare_excerpt(&mut found.excerpt);
            }
            for failure in failures {
                let reason = sanitize_terminal_text(&failure.reason).into_owned();
                reason.clone_into(&mut failure.reason);
            }
        }
        ScopeQueryResult::Search { .. } => {}
    }
}

fn discard_document_source_path(source: &mut Option<mant_ir::DocumentSource>) {
    if let Some(source) = source {
        source.path = None;
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::DocumentAddress;
    use mant_protocol::{
        DocumentScope, DocumentSelector, DocumentTraversal, ResolvedDocumentScope,
        ScopeQueryResponse, ScopeQueryResult, ScopeQuerySchema, ScopedQueryFailure,
    };

    use super::{finish_page, page_text, prepare_scope};
    use crate::mcp::params::{MAX_PAGE_CHARS, PageRequest};

    #[test]
    fn text_pages_are_utf8_safe_bounded_and_continuable() {
        let source = "段落 → content\n\n".repeat(40);
        let first = page_text(
            &source,
            PageRequest {
                start_char: 0,
                max_chars: 17,
            },
        );
        assert_eq!(first.text.chars().count(), 17);
        let next = first.end_char;
        let rendered = finish_page(&first);
        assert!(rendered.starts_with(&format!(
            "[mant-page chars=0..17 totalChars={} nextChar=17]",
            source.chars().count()
        )));

        let second = page_text(
            &source,
            PageRequest {
                start_char: u32::try_from(next).expect("small fixture"),
                max_chars: 17,
            },
        );
        assert_eq!(second.text.chars().count(), 17);
    }

    #[test]
    fn maximum_character_page_has_a_bounded_utf8_body() {
        let text = "😀".repeat(MAX_PAGE_CHARS as usize + 1);
        let page = page_text(
            &text,
            PageRequest {
                start_char: 0,
                max_chars: MAX_PAGE_CHARS,
            },
        );

        assert_eq!(page.text.chars().count(), 32_768);
        assert_eq!(page.text.len(), 131_072);
        assert_eq!(page.end_char, 32_768);
        assert_eq!(page.total_chars, 32_769);
    }

    #[test]
    fn text_pages_preserve_all_whitespace_across_continuations() {
        let source = "code  \n\tindented\n\n".repeat(4_000);
        let mut reconstructed = String::new();
        let mut start_char = 0;
        loop {
            let page = page_text(
                &source,
                PageRequest {
                    start_char,
                    max_chars: 997,
                },
            );
            reconstructed.push_str(&page.text);
            if page.end_char == page.total_chars {
                break;
            }
            start_char = u32::try_from(page.end_char).expect("small fixture");
        }

        assert_eq!(reconstructed, source);
    }

    #[test]
    fn character_offsets_are_unicode_scalar_based_and_past_end_is_empty() {
        let page = page_text(
            "aé中→z",
            PageRequest {
                start_char: 1,
                max_chars: 3,
            },
        );
        assert_eq!(page.text, "é中→");
        assert_eq!(
            (page.start_char, page.end_char, page.total_chars),
            (1, 4, 5)
        );

        let empty = page_text(
            "short",
            PageRequest {
                start_char: 99,
                max_chars: 10,
            },
        );
        assert!(empty.text.is_empty());
        assert_eq!((empty.start_char, empty.end_char), (5, 5));
    }

    #[test]
    fn model_visible_pages_mask_terminal_controls_before_counting() {
        let page = page_text(
            "manual/1\u{1b}[31m/tool",
            PageRequest {
                start_char: 0,
                max_chars: 100,
            },
        );

        assert_eq!(page.text, "manual/1�[31m/tool");
        assert!(!page.text.contains('\u{1b}'));
        assert_eq!(page.total_chars, page.text.chars().count());
    }

    #[test]
    fn scope_failures_keep_their_selector_guidance_but_mask_controls() {
        let mut response = ScopeQueryResponse {
            schema: ScopeQuerySchema::V0Dot10,
            scope: ResolvedDocumentScope {
                query: DocumentScope {
                    documents: vec![DocumentSelector {
                        selector: "tool".to_owned(),
                        source: None,
                        manual_section: None,
                    }],
                    traversal: DocumentTraversal::default(),
                },
                documents: Vec::new(),
                edges: Vec::new(),
                frontier: Vec::new(),
                unresolved: Vec::new(),
            },
            result: ScopeQueryResult::Explain {
                entry: "-f".to_owned(),
                matches: Vec::new(),
                missed: 0,
                failures: vec![ScopedQueryFailure {
                    address: DocumentAddress::Manual {
                        name: "tool".to_owned(),
                        manual_section: "1".to_owned(),
                    },
                    reason: "multiple entries\u{1b}[2J: 1/e1 (first)".to_owned(),
                }],
            },
        };

        prepare_scope(&mut response);
        let ScopeQueryResult::Explain { failures, .. } = response.result else {
            panic!("fixture must stay an explanation");
        };
        assert_eq!(failures[0].reason, "multiple entries�[2J: 1/e1 (first)");
    }
}
