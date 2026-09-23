use std::num::NonZeroU32;

use mant_protocol::{
    OutlineNodeReference, OutlineTrail, QuerySearch, SearchCase, SearchContentProjection,
    SearchContextLine, SearchDisplaySlice, SearchFragment, SearchFragmentSource, SearchLocation,
    SearchMatch, SearchQuery, SearchRender, SearchRenderDerivedFragmentSource,
    SearchRenderDerivedSourceKind, SearchRenderFormat, SearchRenderSchema, SearchRenderScope,
    SearchSchema, SearchScope, SearchSyntax, SearchTextUnit,
};

use super::{SearchTextRole, render_search_markdown, render_search_text_with};

fn trail() -> OutlineTrail {
    OutlineTrail {
        ancestors: Vec::new(),
        node: OutlineNodeReference::DocumentRoot {
            path: "root".into(),
            id: mant_ir::DOCUMENT_ROOT_ID.into(),
            title: "OVERVIEW".into(),
        },
    }
}

fn artifact_match(ordinal: u32, text: &str, start: u64) -> SearchMatch {
    SearchMatch {
        ordinal,
        outline: trail(),
        matched_text: text.into(),
        location: SearchLocation::MarkdownArtifact {
            start_byte: start,
            end_byte: start + text.len() as u64,
            start_line: 1,
            start_column: u32::try_from(start).unwrap() + 1,
            end_line: 1,
            end_column: u32::try_from(start).unwrap() + u32::try_from(text.len()).unwrap() + 1,
        },
        display_slices: Vec::new(),
        node_source: None,
        preview: format!("before {text} after"),
        context: Vec::new(),
    }
}

fn search(matches: Vec<SearchMatch>) -> QuerySearch {
    let total = u32::try_from(matches.len()).unwrap();
    QuerySearch {
        schema: SearchSchema::V0Dot12,
        label: "tar".into(),
        source_context: None,
        meta: None,
        content_projection: None,
        query: SearchQuery {
            pattern: "needle".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Markdown,
            word: false,
            context_lines: 0,
            limit: 100,
            offset: 0,
        },
        render: SearchRender {
            schema: SearchRenderSchema::Markdown,
            format: SearchRenderFormat::Markdown,
            scope: SearchRenderScope::Full,
            line_base: 1,
            column_base: 1,
            line_count: 1,
        },
        total,
        returned: total,
        offset: 0,
        truncated: false,
        next_offset: None,
        semantics_complete: true,
        coverage_details_omitted: 0,
        diagnostics: Vec::new(),
        matches,
    }
}

#[test]
fn one_occurrence_is_one_item_with_its_global_ordinal() {
    let page = search(vec![
        artifact_match(1, "needle", 7),
        artifact_match(2, "needle", 20),
    ]);
    let text = render_search_text_with(&page, |role, value| {
        if role == SearchTextRole::Match {
            format!("<{value}>")
        } else {
            value.to_owned()
        }
    });
    assert_eq!(text.matches("Match: <needle>").count(), 2);
    assert!(text.contains("#1  1:8..1:14"));
    assert!(text.contains("#2  1:21..1:27"));
    let markdown = render_search_markdown(&page);
    assert_eq!(markdown.matches("## ").count(), 2);
    assert_eq!(markdown.matches("- Match: `needle`").count(), 2);
    assert!(!markdown.contains("> before needle after"));
}

#[test]
fn visible_match_uses_typed_unit_and_exact_text_without_markdown_reparse() {
    let mut page = search(vec![artifact_match(1, "*needle*", 0)]);
    page.query.pattern = "*needle*".into();
    page.query.scope = SearchScope::Visible;
    page.content_projection = Some(SearchContentProjection {
        fragments: vec![SearchFragment {
            key: NonZeroU32::new(1).unwrap(),
            text: "literal *needle*".into(),
            source: SearchFragmentSource::RenderDerived(SearchRenderDerivedFragmentSource {
                kind: SearchRenderDerivedSourceKind::RenderDerived,
            }),
        }],
        units: vec![SearchTextUnit {
            key: NonZeroU32::new(1).unwrap(),
            fragments: vec![NonZeroU32::new(1).unwrap()],
            joins: Vec::new(),
        }],
    });
    page.matches[0].location = SearchLocation::VisibleFlow {
        unit: NonZeroU32::new(1).unwrap(),
        start_byte: 8,
        end_byte: 16,
    };
    page.matches[0].display_slices = vec![SearchDisplaySlice {
        fragment: NonZeroU32::new(1).unwrap(),
        start_byte: 8,
        end_byte: 16,
    }];
    page.matches[0].preview = "literal *needle*".into();
    page.matches[0].context = vec![SearchContextLine {
        line: 1,
        text: "literal *needle*".into(),
        matched: true,
    }];
    assert!(page.validate().is_ok());
    let text = render_search_text_with(&page, |role, value| {
        if role == SearchTextRole::Match {
            format!("<{value}>")
        } else {
            value.to_owned()
        }
    });
    assert!(text.contains("visible-flow/u1:8..16"));
    assert!(text.contains("Match: <*needle*>"));
    let markdown = render_search_markdown(&page);
    assert_eq!(markdown.matches("*needle*").count(), 2); // query and one result
    assert!(!text.contains("Preview: literal *needle*"));
}

#[test]
fn markdown_report_escapes_preview_and_keeps_exact_match_in_code_span() {
    let mut page = search(vec![artifact_match(1, "*needle*", 0)]);
    page.matches[0].preview = "[unsafe](https://example.test)".into();
    let markdown = render_search_markdown(&page);
    assert!(markdown.contains("- Match: `*needle*`"));
    assert!(markdown.contains(r"\[unsafe\](https\://example.test)"));
}
