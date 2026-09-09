use super::*;
use mant_ir::{DocumentAddress, MarkdownOrigin};
use mant_protocol::{
    DocumentScope, DocumentTraversal, EvidenceCounts, EvidenceOrder, ExplanationOptions,
    ExplanationOutcome, ExplanationQuery, ExplanationTruncation, MarkdownSchema,
    ResolvedDocumentScope, ScopeExplanation, ScopeQuerySchema, ScopeSearch, ScopedQueryFailure,
    SearchCase, SearchRender, SearchRenderFormat, SearchRenderScope, SearchScope, SearchSyntax,
};

fn address(path: &str) -> DocumentAddress {
    DocumentAddress::Markdown {
        path: path.into(),
        origin: MarkdownOrigin::Documents,
    }
}
fn response(result: ScopeQueryResult) -> ScopeQueryResponse {
    ScopeQueryResponse {
        schema: ScopeQuerySchema::V0Dot11,
        scope: ResolvedDocumentScope {
            query: DocumentScope {
                documents: vec![],
                traversal: DocumentTraversal::default(),
            },
            documents: vec![],
            edges: vec![],
            frontier: vec![],
            unresolved: vec![],
            reference_limits: vec![],
        },
        result,
    }
}
fn explanation() -> ScopeExplanation {
    ScopeExplanation {
        order: EvidenceOrder::ClassThenSource,
        counts: EvidenceCounts::default(),
        evidence: vec![],
        query: ExplanationQuery {
            entry: "-x".into(),
            options: ExplanationOptions::default(),
        },
        outcome: ExplanationOutcome::NoEvidence,
        total: 0,
        returned: 0,
        next_offset: None,
        truncation: ExplanationTruncation::default(),
        documents: vec![],
        failures: vec![],
    }
}
fn search() -> ScopeSearch {
    ScopeSearch {
        query: SearchQuery {
            pattern: "needle".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 1,
            offset: 9,
        },
        total: 12,
        returned: 0,
        offset: 9,
        truncated: true,
        next_offset: Some(10),
        documents: ["z-last", "a-first"]
            .into_iter()
            .map(|path| ScopedSearchDocument {
                address: address(path),
                depth: 0,
                matches: vec![],
                render: SearchRender {
                    schema: MarkdownSchema::V1,
                    format: SearchRenderFormat::Markdown,
                    scope: SearchRenderScope::Full,
                    line_base: 1,
                    column_base: 1,
                    line_count: 1,
                },
            })
            .collect(),
    }
}

#[test]
fn search_grouping_preserves_dto_order_and_does_not_invent_local_pagination() {
    let search = search();
    let response = response(ScopeQueryResult::Search {
        search: search.clone(),
    });
    let before = response.clone();
    assert_eq!(
        render_scope_query_text(&response),
        "documents/z-last\nNo matches for \"needle\" in z-last.\n\ndocuments/a-first\nNo matches for \"needle\" in a-first."
    );
    let markdown = render_scope_query_markdown(&response);
    assert!(markdown.starts_with("## documents/z-last\n"));
    assert!(
        markdown.find("documents/z-last").unwrap() < markdown.find("documents/a-first").unwrap()
    );
    assert!(!markdown.contains("Next offset"));
    assert!(!markdown.contains("Showing matching lines"));
    let local = scoped_search_projection(&search.documents[0], &search.query);
    assert_eq!(
        (local.offset, local.next_offset, local.truncated),
        (0, None, false)
    );
    assert_eq!(local.query, search.query);
    assert_eq!(local.render, search.documents[0].render);
    assert_eq!(local.matches, search.documents[0].matches);
    assert_eq!(response, before);
}

#[test]
fn failure_coverage_and_reference_warnings_remain_in_source_order() {
    let mut explanation = explanation();
    explanation.failures = ["z-last", "a-first"]
        .into_iter()
        .map(|path| ScopedQueryFailure {
            address: address(path),
            reason: format!("failed {path}"),
        })
        .collect();
    let mut response = response(ScopeQueryResult::Explain { explanation });
    response
        .scope
        .reference_limits
        .push(mant_protocol::ScopeReferenceLimit {
            document: address("z-last"),
            coverage: mant_protocol::ReferenceCoverage {
                steps: 0,
                bytes: 0,
                status: mant_protocol::ReferenceCoverageStatus::NotScanned {},
            },
            retention_limit: None,
        });
    let text = render_scope_query_text(&response);
    assert!(text.ends_with("\nCoverage: loaded=0, unresolved=0, frontier=0\n\ndocuments/z-last\nfailed z-last\n\ndocuments/a-first\nfailed a-first\nReference scan incomplete for documents/z-last: NotScanned"));
    let markdown = render_scope_query_markdown(&response);
    assert!(markdown.contains("\nCoverage: loaded=0, unresolved=0, frontier=0\n\n## documents/z-last\nfailed z-last\n\n## documents/a-first\nfailed a-first"));
}

#[test]
fn decoration_keeps_identity_whitespace_and_search_role_boundaries() {
    let mut search = search();
    search.documents.truncate(1);
    search.documents[0].address = address(" odd\u{1b} ");
    let response = response(ScopeQueryResult::Search { search });
    let decorated = render_scope_query_text_with(&response, |role, text| match role {
        ScopeTextRole::Document => format!("<group>{text}</group>"),
        ScopeTextRole::Search(SearchTextRole::Match) => format!("<match>{text}</match>"),
        _ => text.to_owned(),
    });
    assert_eq!(
        decorated,
        "<group>documents/ odd� </group>\nNo matches for \"<match>needle</match>\" in  odd� ."
    );
    assert_eq!(
        render_scope_query_text(&response),
        render_scope_query_text_with(&response, |_, text| text.to_owned())
    );
}

#[test]
fn markdown_identity_mapping_is_explicit_and_does_not_change_the_dto() {
    let mut result = explanation();
    result.failures.push(ScopedQueryFailure {
        address: address("odd\u{1b}"),
        reason: "failure\u{1b}".into(),
    });
    let response = response(ScopeQueryResult::Explain {
        explanation: result,
    });
    let before = response.clone();
    assert!(
        render_scope_query_markdown(&response).contains("## documents/odd\u{1b}\nfailure\u{1b}")
    );
    let safe = render_scope_query_markdown_with(&response, |text| {
        sanitize_terminal_text(text).into_owned()
    });
    assert!(safe.contains("## documents/odd�\nfailure�"));
    assert!(!safe.contains('\u{1b}'));
    assert_eq!(response, before);
}
