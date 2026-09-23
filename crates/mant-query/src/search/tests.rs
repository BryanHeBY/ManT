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
fn fixed_body_uses_fixed_visible_coordinates_even_when_empty() {
    let mut query = crate::query_fixture::markdown("# Demo\n\nneedle\n", None).unwrap();
    query.document.as_mut().unwrap().body = mant_ir::DocumentBody::Fixed(mant_ir::FixedBody {
        surface: mant_ir::DisplaySurface {
            text: String::new(),
            rows: Vec::new(),
            runs: Vec::new(),
        },
        headings: Vec::new(),
        owners: Vec::new(),
        links: Vec::new(),
        anchors: Vec::new(),
        regions: Vec::new(),
    });
    let result = search_query(&query, &request("needle")).expect("empty Fixed search is valid");
    assert_eq!(result.total, 0);
    assert_eq!(
        result.render.schema,
        mant_protocol::SearchRenderSchema::Fixed
    );
}

#[test]
fn styled_hit_keeps_the_canonical_render_position() {
    let query =
        crate::query_fixture::markdown("# Demo\n\nBefore **access control** after.\n", None)
            .unwrap();
    let result = search_query(&query, &request("access control")).unwrap();

    assert_eq!(result.total, 1);
    let occurrence = &result.matches[0];
    assert_eq!(occurrence.matched_text, "access control");
    assert!(result.content_projection.is_some());
    assert!(matches!(
        occurrence.location,
        mant_protocol::SearchLocation::VisibleFlow { .. }
    ));
    serde_json::from_value::<mant_protocol::QuerySearch>(serde_json::to_value(result).unwrap())
        .unwrap();
}

#[test]
fn flow_fragment_cannot_claim_fixed_visible_render_on_the_wire() {
    let query = crate::query_fixture::markdown("# Demo\n\nneedle\n", None).unwrap();
    let mut result = search_query(&query, &request("needle")).unwrap();
    assert!(result.validate().is_ok());
    result.render.schema = mant_protocol::SearchRenderSchema::Fixed;
    result.render.format = mant_protocol::SearchRenderFormat::FixedVisible;
    assert!(result.validate().is_err());
    let wire = serde_json::to_value(result).unwrap();
    assert!(serde_json::from_value::<mant_protocol::QuerySearch>(wire).is_err());
}

#[test]
fn visible_search_preserves_canonical_render_cross_block_match() {
    let query = crate::query_fixture::markdown("# Demo\n\nalpha\n\nbeta\n", None).unwrap();
    let mut request = request("(?s)alpha.*beta");
    request.syntax = SearchSyntax::Regex;

    let result = search_query(&query, &request).unwrap();
    assert_eq!(result.total, 1);
    let projection = result.content_projection.as_ref().unwrap();
    assert_eq!(projection.fragments.len(), 2);
    assert!(projection.fragments.iter().all(|fragment| matches!(
        &fragment.source,
        mant_protocol::SearchFragmentSource::Flow(_)
    )));
    assert_eq!(
        projection.units[0].joins,
        vec![mant_protocol::SearchTextJoin::RenderSeparator { text: "\n".into() }]
    );
    assert_eq!(result.matches[0].matched_text, "alpha\nbeta");
}

#[test]
fn visible_search_boundary_breaks_are_joins_not_display_glyphs() {
    let query = crate::query_fixture::markdown("# Demo\n\nalpha\n\nbeta\n", None).unwrap();
    for (pattern, matched, expected_start, expected_slices) in
        [("alpha\\n", "alpha\n", 0, 1), ("\\nbeta", "\nbeta", 1, 1)]
    {
        let mut request = request(pattern);
        request.syntax = SearchSyntax::Regex;
        let result = search_query(&query, &request).unwrap();
        assert_eq!(result.total, 1, "pattern {pattern}");
        let hit = &result.matches[0];
        assert_eq!(hit.matched_text, matched);
        let projection = result.content_projection.as_ref().unwrap();
        let mant_protocol::SearchLocation::VisibleFlow {
            unit,
            start_byte,
            end_byte,
        } = hit.location
        else {
            panic!("visible search must return a Flow location");
        };
        assert_eq!(start_byte, expected_start);
        assert_eq!(end_byte - start_byte, matched.len() as u64);
        assert_eq!(hit.display_slices.len(), expected_slices);
        assert!(projection.units[0].joins.iter().any(|join| matches!(
            join,
            mant_protocol::SearchTextJoin::RenderSeparator { text } if text == "\n"
        )));
        let start = usize::try_from(start_byte).unwrap();
        let end = usize::try_from(end_byte).unwrap();
        assert_eq!(&projection.unit_text(unit).unwrap()[start..end], matched);
        serde_json::from_value::<mant_protocol::QuerySearch>(serde_json::to_value(result).unwrap())
            .unwrap();
    }
}

#[test]
fn pagination_counts_complete_occurrences_even_on_one_line() {
    let query = crate::query_fixture::markdown("# Demo\n\nneedle needle\n", None).unwrap();
    let mut request = request("needle");
    request.limit = 1;
    let first = search_query(&query, &request).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.returned, 1);
    assert_eq!(first.matches[0].ordinal, 1);
    assert_eq!(first.next_offset, Some(1));

    request.offset = 1;
    let second = search_query(&query, &request).unwrap();
    assert_eq!(second.returned, 1);
    assert_eq!(second.matches[0].ordinal, 2);
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
    let projection = result.content_projection.as_ref().unwrap();
    assert!(projection.fragments.iter().all(|fragment| matches!(
        &fragment.source,
        mant_protocol::SearchFragmentSource::Tldr(source) if source.path == "0")));
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
    assert!(result.content_projection.is_some());
    assert!(result.matches.iter().all(|hit| matches!(
        hit.location,
        mant_protocol::SearchLocation::VisibleFlow { .. }
    )));
}

#[test]
fn markdown_scope_searches_markup_and_pages_occurrences() {
    let query = crate::query_fixture::markdown("# Demo\n\nalpha **needle**\n", None).unwrap();
    let mut request = request("**");
    request.scope = SearchScope::Markdown;
    request.limit = 1;
    let first = search_query(&query, &request).unwrap();
    assert!(first.total >= 1);
    assert_eq!(first.returned, 1);
    assert!(first.content_projection.is_none());
    assert!(matches!(
        first.matches[0].location,
        mant_protocol::SearchLocation::MarkdownArtifact { .. }
    ));
    serde_json::from_value::<mant_protocol::QuerySearch>(serde_json::to_value(first).unwrap())
        .unwrap();
}

#[test]
fn markdown_scope_searches_exact_addressable_anchor_bytes() {
    let query =
        crate::query_fixture::markdown("# Demo\n\n[Jump](#other)\n\n## Other\n\nbody\n", None)
            .unwrap();
    let artifact = mant_codec::encode::render_markdown_with_options(
        &query,
        mant_codec::encode::MarkdownOptions::ADDRESSABLE,
    )
    .unwrap();
    assert!(artifact.contains("<a id="), "{artifact}");
    let mut request = request("<a id=");
    request.scope = SearchScope::Markdown;
    let result = search_query(&query, &request).unwrap();
    assert!(result.total > 0);
    for found in &result.matches {
        let mant_protocol::SearchLocation::MarkdownArtifact {
            start_byte,
            end_byte,
            ..
        } = found.location
        else {
            panic!("Markdown search must use artifact bytes");
        };
        let start = usize::try_from(start_byte).unwrap();
        let end = usize::try_from(end_byte).unwrap();
        assert_eq!(&artifact[start..end], found.matched_text);
    }
}

#[test]
fn markdown_scope_keeps_artifact_match_across_owner_boundaries() {
    let query =
        crate::query_fixture::markdown("# Demo\n\nalpha\n\n## Other\n\nbeta\n", None).unwrap();
    let artifact = mant_codec::encode::render_markdown_with_options(
        &query,
        mant_codec::encode::MarkdownOptions::ADDRESSABLE,
    )
    .unwrap();
    let mut request = request("(?s)alpha.*beta");
    request.syntax = SearchSyntax::Regex;
    request.scope = SearchScope::Markdown;
    let result = search_query(&query, &request).unwrap();
    assert_eq!(result.total, 1);
    let found = &result.matches[0];
    let mant_protocol::SearchLocation::MarkdownArtifact {
        start_byte,
        end_byte,
        ..
    } = found.location
    else {
        panic!("Markdown search must use artifact bytes");
    };
    assert_eq!(
        &artifact[usize::try_from(start_byte).unwrap()..usize::try_from(end_byte).unwrap()],
        found.matched_text,
    );
}

#[test]
fn markdown_scope_keeps_tldr_to_manual_artifact_match() {
    let query = crate::query_fixture::markdown(
        "<!-- mant:tldr:start -->\n# Quick\n\n> alpha\n<!-- mant:tldr:end -->\n\n# Manual\n\nbeta\n",
        None,
    )
    .unwrap();
    let artifact = mant_codec::encode::render_markdown_with_options(
        &query,
        mant_codec::encode::MarkdownOptions::ADDRESSABLE,
    )
    .unwrap();
    let mut request = request("(?s)alpha.*beta");
    request.syntax = SearchSyntax::Regex;
    request.scope = SearchScope::Markdown;
    let result = search_query(&query, &request).unwrap();
    assert_eq!(result.total, 1);
    let mant_protocol::SearchLocation::MarkdownArtifact {
        start_byte,
        end_byte,
        ..
    } = result.matches[0].location
    else {
        panic!("Markdown search must use artifact bytes");
    };
    assert_eq!(
        &artifact[usize::try_from(start_byte).unwrap()..usize::try_from(end_byte).unwrap()],
        result.matches[0].matched_text,
    );
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
fn old_grouped_search_wire_is_rejected() {
    let query = crate::query_fixture::markdown("# Demo\n\nneedle\n", None).unwrap();
    let result = search_query(&query, &request("needle")).unwrap();
    let mut wire = serde_json::to_value(result).unwrap();
    let matched = &mut wire["matches"][0];
    matched.as_object_mut().unwrap().remove("location");
    matched["occurrences"] = serde_json::json!([]);
    assert!(serde_json::from_value::<mant_protocol::QuerySearch>(wire).is_err());
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
