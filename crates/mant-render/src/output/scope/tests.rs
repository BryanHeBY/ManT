use super::*;
use mant_ir::{
    DocumentAddress, MarkdownOrigin, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey,
    SourceRecord,
};
use mant_protocol::{
    DocumentScope, DocumentTraversal, EvidenceCounts, EvidenceOrder, ExplanationOptions,
    ExplanationOutcome, ExplanationQuery, ExplanationTruncation, OutlineNodeReference,
    OutlineTrail, ResolvedDocumentScope, ScopeExplanation, ScopeQuerySchema, ScopeSearch,
    ScopeSearchSchema, ScopedQueryFailure, ScopedSearchCoverage, ScopedSearchDocument, SearchCase,
    SearchLocation, SearchMatch, SearchQuery, SearchRender, SearchRenderFormat, SearchRenderSchema,
    SearchRenderScope, SearchSyntax,
};

fn address(path: &str) -> DocumentAddress {
    DocumentAddress::Markdown {
        path: path.into(),
        origin: MarkdownOrigin::Documents,
    }
}
fn source_context() -> mant_protocol::SourceContext {
    mant_protocol::SourceContext {
        sources: vec![SourceRecord {
            key: SourceKey::FIRST,
            identity: SourceIdentity::Anonymous {
                name: "test".into(),
            },
            format: SourceFormat::Markdown,
            decoded_byte_length: 0,
            content_sha256: None,
            coordinates: SourceCoordinates::DecodedUtf8Bytes,
        }],
        root_source: SourceKey::FIRST,
    }
}
fn response(result: ScopeQueryResult) -> ScopeQueryResponse {
    ScopeQueryResponse {
        schema: ScopeQuerySchema::V0Dot12,
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
        schema: ScopeSearchSchema::V0Dot12,
        query: SearchQuery {
            pattern: "needle".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: mant_protocol::SearchScope::Markdown,
            word: false,
            context_lines: 0,
            limit: 2,
            offset: 9,
        },
        total: 12,
        returned: 2,
        offset: 9,
        truncated: true,
        next_offset: Some(11),
        semantics_complete: true,
        coverage_by_document: ["z-last", "a-first"]
            .into_iter()
            .map(|path| ScopedSearchCoverage {
                address: address(path),
                depth: 0,
                source_context: Some(source_context()),
                semantics_complete: true,
                coverage_details_omitted: 0,
                diagnostics: vec![],
            })
            .collect(),
        documents: ["z-last", "a-first"]
            .into_iter()
            .enumerate()
            .map(|(index, path)| ScopedSearchDocument {
                address: address(path),
                depth: 0,
                source_context: Some(source_context()),
                content_projection: None,
                matches: vec![SearchMatch {
                    ordinal: 10 + u32::try_from(index).unwrap(),
                    outline: OutlineTrail {
                        ancestors: vec![],
                        node: OutlineNodeReference::DocumentRoot {
                            path: "root".into(),
                            id: mant_ir::DOCUMENT_ROOT_ID.into(),
                            title: "OVERVIEW".into(),
                        },
                    },
                    matched_text: "needle".into(),
                    location: SearchLocation::MarkdownArtifact {
                        start_byte: 0,
                        end_byte: 6,
                        start_line: 1,
                        start_column: 1,
                        end_line: 1,
                        end_column: 7,
                    },
                    display_slices: vec![],
                    node_source: None,
                    preview: "needle".into(),
                    context: vec![],
                }],
                render: SearchRender {
                    schema: SearchRenderSchema::Markdown,
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
    assert!(search.validate().is_ok());
    let text = render_scope_query_text(&response);
    assert!(text.starts_with("documents/z-last\nz-last  Outline root: OVERVIEW\n  #10"));
    assert!(text.contains("documents/a-first\na-first  Outline root: OVERVIEW\n  #11"));
    assert_eq!(text.matches("Match: needle").count(), 2);
    assert_eq!(text.matches("next occurrence offset 11").count(), 1);
    let markdown = render_scope_query_markdown(&response);
    assert!(markdown.starts_with("## documents/z-last\n"));
    assert!(
        markdown.find("documents/z-last").unwrap() < markdown.find("documents/a-first").unwrap()
    );
    assert_eq!(markdown.matches("Next offset: `11`").count(), 1);
    assert!(!markdown.contains("Showing matching lines"));
    assert_eq!(response, before);
}

#[test]
fn scoped_zero_hit_page_does_not_invent_document_groups() {
    let mut search = search();
    search.documents.clear();
    search.total = 0;
    search.returned = 0;
    search.offset = 0;
    search.query.offset = 0;
    search.truncated = false;
    search.next_offset = None;
    assert!(search.validate().is_ok());
    let response = response(ScopeQueryResult::Search { search });
    assert_eq!(
        render_scope_query_text(&response),
        "No matches for \"needle\" in scope."
    );
    assert_eq!(
        render_scope_query_markdown(&response),
        "No matches for `needle` in scope."
    );
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
    search.coverage_by_document.truncate(1);
    search.coverage_by_document[0].address = search.documents[0].address.clone();
    search.total = 10;
    search.returned = 1;
    search.truncated = false;
    search.next_offset = None;
    assert!(search.validate().is_ok());
    let response = response(ScopeQueryResult::Search { search });
    let decorated = render_scope_query_text_with(&response, |role, text| match role {
        ScopeTextRole::Document => format!("<group>{text}</group>"),
        ScopeTextRole::Search(SearchTextRole::Match) => format!("<match>{text}</match>"),
        _ => text.to_owned(),
    });
    assert!(
        decorated.starts_with("<group>documents/ odd� </group>\n odd�   Outline root: OVERVIEW")
    );
    assert!(decorated.contains("Match: <match>needle</match>"));
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
