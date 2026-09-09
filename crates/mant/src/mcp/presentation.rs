//! Compact, bounded presentations for the agent-facing MCP boundary.

use std::fmt::Write as _;

use mant_protocol::{
    DocumentCatalog, QueryExcerpt, QueryOutline, ScopeQueryResponse, ScopeQueryResult,
    TraversalLimit,
};

use super::params::MAX_PAGE_CHARS;
use super::params::PageRequest;

/// Consume complete responses so cleanup cannot be omitted by tool handlers.
pub(super) fn present_find(catalog: &DocumentCatalog, page: PageRequest) -> String {
    finish_page(&render_find(catalog, page))
}
pub(super) fn present_outline(mut outline: QueryOutline, page: PageRequest) -> String {
    prepare_outline(&mut outline);
    finish_page(&render_outline(&outline, page))
}
pub(super) fn present_excerpt(mut excerpt: QueryExcerpt, page: PageRequest) -> String {
    prepare_excerpt(&mut excerpt);
    finish_page(&render_excerpt(&excerpt, page))
}
pub(super) fn present_scope_explain(
    mut response: ScopeQueryResponse,
    page: PageRequest,
) -> Result<String, String> {
    prepare_scope(&mut response);
    render_scope_explain(&response, page)
        .map(|page| finish_page(&page))
        .map_err(finish_error)
}
pub(super) fn present_scope_search(
    mut response: ScopeQueryResponse,
    page: PageRequest,
) -> Result<String, String> {
    prepare_scope(&mut response);
    render_scope_search(&response, page)
        .map(|page| finish_page(&page))
        .map_err(finish_error)
}

/// One stateless character page of a canonical rendered result.
#[derive(Debug, PartialEq, Eq)]
struct TextPage {
    text: String,
    start_char: usize,
    end_char: usize,
    total_chars: usize,
}

fn render_find(catalog: &DocumentCatalog, page: PageRequest) -> TextPage {
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
    let records = mant_render::render_catalog_text(catalog, false);
    if !records.is_empty() {
        text.push('\n');
        text.push_str(records.trim_end());
    } else if let Some(coverage) = mant_render::render_catalog_coverage_text(catalog) {
        text.push_str("; ");
        text.push_str(&coverage.replace('\n', "; "));
    }
    page_text(&text, page)
}

fn render_outline(outline: &QueryOutline, page: PageRequest) -> TextPage {
    let mut text = mant_render::render_outline_text(outline);
    if let Some(address) = &outline.address {
        let mut sources = std::collections::BTreeSet::new();
        for record in &outline.references.records {
            if sources.insert(&record.source_read) {
                text.push('\n');
                text.push_str(&read_hint_selector(address, &record.source_read));
            }
        }
    }
    page_text(&text, page)
}

fn render_excerpt(excerpt: &QueryExcerpt, page: PageRequest) -> TextPage {
    page_text(&mant_render::render_excerpt_markdown(excerpt), page)
}

fn render_scope_explain(
    response: &ScopeQueryResponse,
    page: PageRequest,
) -> Result<TextPage, String> {
    let ScopeQueryResult::Explain { explanation } = &response.result else {
        return Err("scope response does not contain an explanation".to_owned());
    };
    let mut text = mant_render::render_scope_query_markdown(response);
    if explanation.outcome == mant_protocol::ExplanationOutcome::NoEvidence {
        let document = response.scope.documents.first().map_or_else(
            || "DOCUMENT".to_owned(),
            |document| document.address.catalog_path(),
        );
        let document = serde_json::to_string(&document)
            .expect("serializing a String for an MCP hint cannot fail");
        let entry = serde_json::to_string(&explanation.query.entry)
            .expect("serializing a String for an MCP hint cannot fail");
        let _ = write!(
            text,
            "\nNext: call mant_outline(document={document}, entries={{\"kind\":\"all\"}}) for \
             available selectors, repeating it for other resolved documents; call mant_search \
             with the same documents and pattern={entry} for a broader literal search."
        );
    }
    for record in &explanation.evidence {
        let evidence = &record.evidence;
        if evidence.has_omitted_content()
            || matches!(
                evidence.class,
                mant_protocol::EvidenceClass::EntryMention
                    | mant_protocol::EvidenceClass::ContextMention
            )
        {
            let document = &explanation.documents[record.document_index].address;
            text.push('\n');
            text.push_str(&read_hint(document, evidence.outline.path()));
        }
    }
    append_scope_status(&mut text, response);
    Ok(page_text(&text, page))
}

/// JSON-quoted arguments inside a delimiter that cannot be closed by source text.
fn read_hint(address: &mant_ir::DocumentAddress, path: &str) -> String {
    read_hint_selector(address, &mant_protocol::ContentSelector::path(path))
}

fn read_hint_selector(
    address: &mant_ir::DocumentAddress,
    selector: &mant_protocol::ContentSelector,
) -> String {
    let document = serde_json::to_string(&address.catalog_path()).expect("String serialization");
    let selector = serde_json::to_string(selector).expect("selector serialization");
    let call = format!("mant_read(document={document}, selectors=[{selector}])");
    let longest = call.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let delimiter = "`".repeat(longest + 1);
    format!("Read original: call {delimiter}{call}{delimiter}.")
}

fn render_scope_search(
    response: &ScopeQueryResponse,
    page: PageRequest,
) -> Result<TextPage, String> {
    let ScopeQueryResult::Search { search } = &response.result else {
        return Err("scope response does not contain search results".to_owned());
    };
    let mut text = mant_render::render_scope_query_text(response);
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
            "[scope: documents={}, unresolved-roots={unresolved_roots}, unresolved-links={unresolved_links}, depth-frontier={depth_frontier}, document-frontier={document_frontier}, content-frontier={content_frontier}, incomplete-reference-scans={}]",
            response.scope.documents.len(),
            response.scope.reference_limits.len()
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
fn finish_page(page: &TextPage) -> String {
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

/// Apply the same model-visible control and size boundary to tool failures.
pub(super) fn finish_error(error: impl AsRef<str>) -> String {
    sanitize_model_text(error.as_ref())
        .chars()
        .take(usize::try_from(MAX_PAGE_CHARS).unwrap_or(usize::MAX))
        .collect()
}

fn page_text(text: &str, page: PageRequest) -> TextPage {
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

fn prepare_excerpt(excerpt: &mut QueryExcerpt) {
    excerpt.diagnostics.clear();
    discard_document_source_path(&mut excerpt.source);
    for selection in &mut excerpt.selections {
        if let mant_protocol::ExcerptSelection::Tldr { document, .. } = selection {
            document.source_path.clear();
        }
    }
}

fn prepare_outline(outline: &mut QueryOutline) {
    outline.diagnostics.clear();
    discard_document_source_path(&mut outline.source);
}

fn prepare_scope(response: &mut ScopeQueryResponse) {
    for unresolved in &mut response.scope.unresolved {
        "document could not be resolved".clone_into(&mut unresolved.reason);
    }
    match &mut response.result {
        ScopeQueryResult::Explain { explanation } => {
            for found in &mut explanation.documents {
                found.diagnostics.clear();
            }
            for failure in &mut explanation.failures {
                "document could not be projected".clone_into(&mut failure.reason);
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

    use super::{finish_error, finish_page, page_text, prepare_scope};
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
    fn tool_errors_are_control_free_and_scalar_bounded() {
        let error = format!("bad\u{1b}{}", "界".repeat(40_000));
        let presented = finish_error(error);
        assert!(!presented.contains('\u{1b}'));
        assert!(presented.contains('\u{fffd}'));
        assert_eq!(presented.chars().count(), 32_768);
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
    fn original_read_hint_cannot_close_its_code_span() {
        let address = mant_ir::DocumentAddress::Markdown {
            path: "odd`[label]".into(),
            origin: mant_ir::MarkdownOrigin::Documents,
        };
        assert_eq!(
            super::read_hint(&address, "1/e2"),
            "Read original: call ``mant_read(document=\"documents/odd`[label]\", selectors=[{\"kind\":\"path\",\"path\":\"1/e2\"}])``."
        );
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
            schema: ScopeQuerySchema::V0Dot11,
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
                reference_limits: Vec::new(),
            },
            result: ScopeQueryResult::Explain {
                explanation: mant_protocol::ScopeExplanation {
                    order: mant_protocol::EvidenceOrder::ClassThenSource,
                    counts: mant_protocol::EvidenceCounts::default(),
                    evidence: Vec::new(),
                    query: mant_protocol::ExplanationQuery {
                        entry: "-f".to_owned(),
                        options: mant_protocol::ExplanationOptions::default(),
                    },
                    outcome: mant_protocol::ExplanationOutcome::NoEvidence,
                    total: 0,
                    returned: 0,
                    next_offset: None,
                    truncation: mant_protocol::ExplanationTruncation::default(),
                    documents: Vec::new(),
                    failures: vec![ScopedQueryFailure {
                        address: DocumentAddress::Manual {
                            name: "tool".to_owned(),
                            manual_section: "1".to_owned(),
                        },
                        reason: "multiple entries\u{1b}[2J: 1/e1 (first)".to_owned(),
                    }],
                },
            },
        };

        let page = super::present_scope_explain(
            response.clone(),
            PageRequest {
                start_char: 0,
                max_chars: MAX_PAGE_CHARS,
            },
        )
        .expect("valid explanation response");
        assert!(page.starts_with("[mant-page chars=0.."));
        assert!(page.contains("Coverage: loaded=0, unresolved=0, frontier=0"));
        assert!(page.contains("document could not be projected"));
        assert!(!page.contains("multiple entries"));
        assert!(page.contains("Next: call mant_outline(document=\"DOCUMENT\""));
        assert!(page.contains("pattern=\"-f\""));
        assert!(!page.contains('\u{1b}'));
        prepare_scope(&mut response);
        let ScopeQueryResult::Explain { explanation } = response.result else {
            panic!("fixture must stay an explanation");
        };
        assert_eq!(
            explanation.failures[0].reason,
            "document could not be projected"
        );
    }
}
