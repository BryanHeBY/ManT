use mant_protocol::{MAX_SEARCH_PATTERN_CHARS, SearchCase, SearchQuery, SearchScope, SearchSyntax};

use super::{SearchError, search_query, validate_search_query};

fn request(pattern: &str) -> SearchQuery {
    SearchQuery {
        pattern: pattern.to_owned(),
        syntax: SearchSyntax::Literal,
        case: SearchCase::Insensitive,
        scope: SearchScope::Visible,
        word: false,
        context_lines: 1,
        limit: 100,
        offset: 0,
    }
}

#[test]
fn styled_hit_keeps_the_canonical_render_position() {
    let query =
        crate::query_fixture::markdown("# Demo\n\nBefore **access control** after.\n", None)
            .unwrap();
    let result = search_query(&query, &request("access control")).unwrap();

    assert_eq!(result.total, 1);
    let occurrence = &result.matches[0].occurrences[0];
    assert_eq!(occurrence.matched_text, "access control");
    assert!(result.content_projection.is_none());
    assert!(occurrence.root.is_none());
    assert!(occurrence.logical.is_none());
    assert!(occurrence.markdown.is_some());
    assert_eq!(occurrence.line_ranges.len(), 1);
    serde_json::from_value::<mant_protocol::QuerySearch>(serde_json::to_value(result).unwrap())
        .unwrap();
}

#[test]
fn visible_search_preserves_canonical_render_cross_block_match() {
    let query = crate::query_fixture::markdown("# Demo\n\nalpha\n\nbeta\n", None).unwrap();
    let mut request = request("(?s)alpha.*beta");
    request.syntax = SearchSyntax::Regex;

    assert_eq!(search_query(&query, &request).unwrap().total, 1);
}

#[test]
fn pagination_counts_matching_line_groups() {
    let query = crate::query_fixture::markdown("# Demo\n\nneedle needle\n", None).unwrap();
    let mut request = request("needle");
    request.limit = 1;
    let first = search_query(&query, &request).unwrap();
    assert_eq!(first.total, 1);
    assert_eq!(first.returned, 1);
    assert_eq!(first.matches[0].occurrence_count, 2);
    assert_eq!(first.matches[0].occurrences.len(), 2);
    assert_eq!(first.next_offset, None);

    request.offset = 1;
    let second = search_query(&query, &request).unwrap();
    assert_eq!(second.returned, 0);
    assert!(second.matches.is_empty());
    assert_eq!(second.next_offset, None);
}

#[test]
fn offset_beyond_total_returns_a_closed_empty_page() {
    let query = crate::query_fixture::markdown("# Demo\n\nneedle\n", None).unwrap();
    let mut request = request("needle");
    request.offset = 5;

    let result = search_query(&query, &request).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.returned, 0);
    assert!(!result.truncated);
    assert_eq!(result.next_offset, None);
    assert_eq!(result.content_projection, None);
    serde_json::from_value::<mant_protocol::QuerySearch>(serde_json::to_value(result).unwrap())
        .unwrap();
}

#[test]
fn tldr_only_search_retains_rendered_coordinates_without_a_fake_root() {
    let mut query = crate::query_fixture::markdown(
        "<!-- mant:tldr:start -->\n# quick\n\n> needle\n<!-- mant:tldr:end -->\n",
        None,
    )
    .unwrap();
    query.document = None;

    let result = search_query(&query, &request("needle")).unwrap();
    assert_eq!(result.total, 1);
    assert!(result.source_context.is_none());
    assert!(result.content_projection.is_none());
    assert!(result.matches[0].occurrences[0].root.is_none());
    assert!(result.matches[0].occurrences[0].markdown.is_some());
    serde_json::from_value::<mant_protocol::QuerySearch>(serde_json::to_value(result).unwrap())
        .unwrap();
}

#[test]
fn mixed_document_and_tldr_search_keeps_both_rendered_owners() {
    let query = crate::query_fixture::markdown(
        "<!-- mant:tldr:start -->\n# Quick\n\n> needle\n<!-- mant:tldr:end -->\n\n# Manual\n\nneedle\n",
        None,
    )
    .unwrap();
    let result = search_query(&query, &request("needle")).unwrap();
    assert_eq!(result.total, 2);
    assert!(result.content_projection.is_none());
    assert!(
        result
            .matches
            .iter()
            .all(|hit| hit.occurrences[0].root.is_none())
    );
    assert!(
        result
            .matches
            .iter()
            .all(|hit| hit.occurrences[0].markdown.is_some())
    );
}

#[test]
fn markdown_scope_searches_markup_and_pages_line_groups() {
    let query = crate::query_fixture::markdown("# Demo\n\nalpha **needle**\n", None).unwrap();
    let mut request = request("**");
    request.scope = SearchScope::Markdown;
    request.limit = 1;
    let first = search_query(&query, &request).unwrap();
    assert!(first.total >= 1);
    assert_eq!(first.returned, 1);
    assert!(first.content_projection.is_none());
    assert!(first.matches[0].occurrences[0].markdown.is_some());
    serde_json::from_value::<mant_protocol::QuerySearch>(serde_json::to_value(first).unwrap())
        .unwrap();
}

#[test]
fn tldr_only_zero_hit_and_beyond_end_pages_are_valid() {
    let mut query = crate::query_fixture::markdown(
        "<!-- mant:tldr:start -->\n# Quick\n\n> needle\n<!-- mant:tldr:end -->\n",
        None,
    )
    .unwrap();
    query.document = None;
    let zero = search_query(&query, &request("absent")).unwrap();
    assert_eq!(zero.total, 0);
    assert!(zero.matches.is_empty());
    let mut request = request("needle");
    request.offset = 9;
    let beyond = search_query(&query, &request).unwrap();
    assert_eq!(beyond.total, 1);
    assert!(beyond.matches.is_empty());
    assert!(!beyond.truncated);
}

#[test]
fn validates_bounded_nonempty_patterns() {
    assert_eq!(
        validate_search_query(&request("")),
        Err(SearchError::EmptyPattern)
    );
    assert_eq!(
        validate_search_query(&request(&"x".repeat(MAX_SEARCH_PATTERN_CHARS + 1))),
        Err(SearchError::PatternTooLong)
    );
}
