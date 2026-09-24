//! Cross-language golden tests for the versioned query protocol suite.

use mant_ir::{Block, Inline, SourceFormat};
use mant_protocol::{
    EntryProjection, QueryBundle, QueryInput, QueryOutline, QueryRequest, QuerySchema, QueryView,
    RequestSchema, ScopeQueryRequest, ScopeQueryResponse, ScopeQueryResult, ScopeQueryView,
    ScopeRequestSchema, SearchCase, SearchScope, SearchSyntax,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

const MINIMAL_QUERY: &str = include_str!("../../../tests/contracts/minimal-query-v0.12.json");
const ROOTED_OUTLINE: &str = include_str!("../../../tests/contracts/rooted-outline-v0.12.json");
const SCOPE_SEARCH: &str = include_str!("../../../tests/contracts/scope-search-v0.12.json");
const SCOPE_EXPLAIN: &str = include_str!("../../../tests/contracts/scope-explain-v0.12.json");
const EXPLANATION: &str = include_str!("../../../tests/contracts/explanation-v0.12.json");

fn assert_source_only_diagnostic_contract<T: DeserializeOwned>(
    mut response: Value,
    report_pointer: &str,
) {
    let report = response
        .pointer_mut(report_pointer)
        .expect("report carrying diagnostics");
    if report.get("sourceContext").is_none_or(Value::is_null) {
        report["sourceContext"] = serde_json::json!({
            "sources":[{
                "key":1,
                "identity":{"kind":"anonymous","name":"test"},
                "format":"man",
                "decodedByteLength":0,
                "coordinates":{"kind":"decoded-utf8-bytes"}
            }],
            "rootSource":1
        });
    }
    report["diagnostics"] = serde_json::json!([{
        "level":"warning", "impact":"none", "message":"expanded finding", "sourceKey":1
    }]);
    serde_json::from_value::<T>(response.clone()).unwrap_or_else(|error| {
        panic!(
            "{} rejected valid source-only diagnostic: {error}",
            std::any::type_name::<T>()
        )
    });

    let mut unknown = response.clone();
    unknown.pointer_mut(report_pointer).unwrap()["diagnostics"][0]["sourceKey"] = 2.into();
    assert!(serde_json::from_value::<T>(unknown).is_err());

    let mut missing_context = response.clone();
    missing_context
        .pointer_mut(report_pointer)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("sourceContext");
    assert!(serde_json::from_value::<T>(missing_context).is_err());

    let mut scoped = response.clone();
    let scoped_diagnostic = &mut scoped.pointer_mut(report_pointer).unwrap()["diagnostics"][0];
    scoped_diagnostic
        .as_object_mut()
        .unwrap()
        .remove("sourceKey");
    scoped_diagnostic["coverageScope"] = serde_json::json!({"kind":"source","key":1});
    assert!(serde_json::from_value::<T>(scoped.clone()).is_ok());
    let mut unknown_scope = scoped.clone();
    unknown_scope.pointer_mut(report_pointer).unwrap()["diagnostics"][0]["coverageScope"]["key"] =
        2.into();
    assert!(serde_json::from_value::<T>(unknown_scope).is_err());
    scoped
        .pointer_mut(report_pointer)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("sourceContext");
    assert!(serde_json::from_value::<T>(scoped).is_err());

    response.pointer_mut(report_pointer).unwrap()["diagnostics"][0]["source"] =
        serde_json::json!({"source":1,"line":1,"column":1});
    assert!(serde_json::from_value::<T>(response).is_err());
}

#[test]
fn source_only_diagnostics_close_against_each_query_context() {
    // Protocol-only wire cases: no roff behavior is asserted here.
    let search = serde_json::json!({
        "schema":"mant.search/v0.12", "label":"test",
        "query":{"pattern":"x","scope":"visible","limit":10},
        "render":{"schema":"mant.markdown/v1","format":"markdown","scope":"full",
            "lineBase":1,"columnBase":1,"lineCount":0},
        "total":0,"returned":0,"offset":0,"truncated":false,
        "semanticsComplete":true,"coverageDetailsOmitted":0,
        "diagnostics":[],"matches":[]
    });
    assert_source_only_diagnostic_contract::<mant_protocol::QuerySearch>(search, "");
    let outline: Value = serde_json::from_str(ROOTED_OUTLINE).unwrap();
    assert_source_only_diagnostic_contract::<QueryOutline>(outline, "");
    let excerpt = serde_json::json!({
        "schema":"mant.excerpt/v0.12","label":"test","selections":[]
    });
    assert_source_only_diagnostic_contract::<mant_protocol::QueryExcerpt>(excerpt, "");
    let explanation: Value = serde_json::from_str(EXPLANATION).unwrap();
    assert_source_only_diagnostic_contract::<mant_protocol::QueryExplanation>(explanation, "");
    let scope_search: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    assert_source_only_diagnostic_contract::<mant_protocol::ScopedSearchCoverage>(
        scope_search["result"]["search"]["coverageByDocument"][0].clone(),
        "",
    );
    assert_source_only_diagnostic_contract::<ScopeQueryResponse>(
        scope_search,
        "/result/search/coverageByDocument/0",
    );
    let scope_explanation: Value = serde_json::from_str(SCOPE_EXPLAIN).unwrap();
    assert_source_only_diagnostic_contract::<ScopeQueryResponse>(
        scope_explanation,
        "/result/explanation/documents/0",
    );
}

#[test]
fn independent_evidence_contract_preserves_ordinary_owners_and_omission_state() {
    let expected: Value = serde_json::from_str(EXPLANATION).unwrap();
    let result: mant_protocol::QueryExplanation = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(result.total, 2);
    assert!(result.evidence[1].entry.is_none());
    assert!(result.evidence[0].content_omitted);
    assert_eq!(serde_json::to_value(result).unwrap(), expected);
    for invalid in [
        r#"{"kind":"name"}"#,
        r#"{"kind":"form"}"#,
        r#"{"kind":"identity"}"#,
        r#"{"kind":"name","matches":[],"extra":true}"#,
        r#"{"kind":"identity","fields":["title"]}"#,
        r#"{"kind":"literal","extra":true}"#,
        r#"{"kind":"related","from":"a","declarations":["b"],"extra":true}"#,
    ] {
        assert!(serde_json::from_str::<mant_protocol::EvidenceBasis>(invalid).is_err());
    }
    let invalid = EXPLANATION.replace("\"limit\": 50", "\"limit\": 50, \"unknown\": true");
    assert!(serde_json::from_str::<mant_protocol::QueryExplanation>(&invalid).is_err());
}

#[test]
fn explanation_rejects_unknown_sources_in_previews_and_support_blocks() {
    let context = serde_json::json!({
        "sources":[{
            "key":1,
            "identity":{"kind":"anonymous","name":"test"},
            "format":"markdown",
            "decodedByteLength":0,
            "coordinates":{"kind":"decoded-utf8-bytes"}
        }],
        "rootSource":1
    });
    let mut preview: Value = serde_json::from_str(EXPLANATION).unwrap();
    preview["sourceContext"] = context.clone();
    preview["evidence"][0]["previews"] = serde_json::json!([{
        "blockPath":"root/b0",
        "source":{"source":2,"line":1,"column":1},
        "text":"x",
        "matchStartChar":0,
        "matchEndChar":1,
        "contentRanges":[],
        "clippedBefore":false,
        "clippedAfter":false
    }]);
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(preview).is_err());

    let mut support: Value = serde_json::from_str(EXPLANATION).unwrap();
    support["sourceContext"] = context;
    support["supports"] = serde_json::json!([{
        "kind":"owned-entry",
        "block":{
            "type":"definition-list",
            "source":{"source":2,"line":1,"column":1},
            "items":[{
                "entry":{"id":"command-example","kind":{"kind":"command"},
                    "case":"sensitive","names":[],"forms":[]},
                "terms":[],"description":[]
            }]
        }
    }]);
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(support).is_err());
}

#[test]
fn fixed_mention_preview_needs_no_flow_block_or_body_but_checks_its_own_sources() {
    // Pure protocol wire validation; no roff behavior expectation is asserted.
    let mut response: Value = serde_json::from_str(EXPLANATION).unwrap();
    let evidence = &mut response["evidence"][1];
    evidence.as_object_mut().unwrap().remove("blockPath");
    evidence.as_object_mut().unwrap().remove("content");
    evidence["previews"] = serde_json::json!([]);
    evidence["fixedPreviews"] = serde_json::json!([{
        "selection": {"parts": [{
            "slice": {"run": 1, "startByte": 0, "endByte": 4},
            "row": 1, "runColumn": 0, "column": 0, "width": 2,
            "style": {"bold": false, "underline": false}, "text": "中a"
        }], "joins": []},
        "matchStartScalar": 0, "matchEndScalar": 2,
        "clippedBefore": false, "clippedAfter": false
    }]);
    let decoded: mant_protocol::QueryExplanation =
        serde_json::from_value(response.clone()).unwrap();
    assert!(decoded.evidence[1].content.is_none());
    assert!(decoded.evidence[1].block_path.is_none());
    assert_eq!(decoded.evidence[1].fixed_previews.len(), 1);
    assert_eq!(serde_json::to_value(decoded).unwrap(), response);

    let mut mixed = response.clone();
    mixed["evidence"][1]["previews"] =
        serde_json::from_str::<Value>(EXPLANATION).unwrap()["evidence"][1]["previews"].clone();
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(mixed).is_err());
    let mut excessive = response.clone();
    let representative = excessive["evidence"][1]["fixedPreviews"][0].clone();
    excessive["evidence"][1]["fixedPreviews"] = serde_json::json!([
        representative.clone(),
        representative.clone(),
        representative
    ]);
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(excessive).is_err());
    let mut wrong_source = response.clone();
    wrong_source["evidence"][1]["fixedPreviews"][0]["selection"]["parts"][0]["source"] = 1.into();
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(wrong_source).is_err());
    let mut wrong_authorship = response;
    wrong_authorship["evidence"][1]["fixedPreviews"][0]["source"] =
        serde_json::json!({"source":1,"line":1,"column":1});
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(wrong_authorship).is_err());

    let mut sourced: Value = serde_json::from_str(EXPLANATION).unwrap();
    sourced["evidence"][1]
        .as_object_mut()
        .unwrap()
        .remove("blockPath");
    sourced["evidence"][1]
        .as_object_mut()
        .unwrap()
        .remove("content");
    sourced["evidence"][1]["previews"] = serde_json::json!([]);
    sourced["evidence"][1]["fixedPreviews"] = serde_json::json!([{
        "selection": {"parts": [{
            "slice": {"run": 1, "startByte": 0, "endByte": 1},
            "row": 1, "runColumn": 0, "column": 0, "width": 1,
            "style": {"bold": false, "underline": false}, "text": "x", "source": 1
        }], "joins": []},
        "matchStartScalar": 0, "matchEndScalar": 1,
        "source": {"source": 1, "line": 1, "column": 1},
        "clippedBefore": false, "clippedAfter": false
    }]);
    sourced["sourceContext"] = serde_json::json!({
        "sources": [{
            "key": 1,
            "identity": {"kind": "anonymous", "name": "test"},
            "format": "markdown",
            "decodedByteLength": 0,
            "coordinates": {"kind": "decoded-utf8-bytes"}
        }],
        "rootSource": 1
    });
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(sourced.clone()).is_ok());
    sourced["evidence"][1]["fixedPreviews"][0]["source"]["source"] = 2.into();
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(sourced).is_err());
}

#[test]
fn scope_results_validate_spans_against_each_document_context() {
    let mut coverage: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    coverage["result"]["search"]["coverageByDocument"][0]["diagnostics"] = serde_json::json!([{"level":"warning","impact":"none", "message":"source finding",
            "source":{"source":2,"line":1,"column":1}}]);
    assert!(serde_json::from_value::<ScopeQueryResponse>(coverage).is_err());

    let mut missing_coverage_context: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    missing_coverage_context["result"]["search"]["coverageByDocument"][0]
        .as_object_mut()
        .unwrap()
        .remove("sourceContext");
    missing_coverage_context["result"]["search"]["coverageByDocument"][0]["diagnostics"] = serde_json::json!([{"level":"warning","impact":"none", "message":"source finding",
            "source":{"source":1,"line":1,"column":1}}]);
    assert!(serde_json::from_value::<ScopeQueryResponse>(missing_coverage_context).is_err());

    let mut search: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    search["result"]["search"]["documents"][0]["matches"][0]["nodeSource"] =
        serde_json::json!({"source":2,"line":1,"column":1});
    assert!(serde_json::from_value::<ScopeQueryResponse>(search).is_err());

    let mut missing_search_context: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    missing_search_context["result"]["search"]["documents"][0]
        .as_object_mut()
        .unwrap()
        .remove("sourceContext");
    missing_search_context["result"]["search"]["documents"][0]["matches"][0]["nodeSource"] =
        serde_json::json!({"source":1,"line":1,"column":1});
    assert!(serde_json::from_value::<ScopeQueryResponse>(missing_search_context).is_err());

    let mut explanation: Value = serde_json::from_str(SCOPE_EXPLAIN).unwrap();
    explanation["result"]["explanation"]["documents"][0]["supports"] = serde_json::json!([{
        "kind":"owned-entry",
        "block":{
            "type":"definition-list",
            "source":{"source":2,"line":1,"column":1},
            "items":[{
                "entry":{"id":"command-example","kind":{"kind":"command"},
                    "case":"sensitive","names":[],"forms":[]},
                "terms":[],"description":[]
            }]
        }
    }]);
    assert!(serde_json::from_value::<ScopeQueryResponse>(explanation).is_err());

    let mut missing_explanation_context: Value = serde_json::from_str(SCOPE_EXPLAIN).unwrap();
    missing_explanation_context["result"]["explanation"]["documents"][0]
        .as_object_mut()
        .unwrap()
        .remove("sourceContext");
    missing_explanation_context["result"]["explanation"]["documents"][0]["diagnostics"] = serde_json::json!([{
        "severity":"warning",
        "message":"source-qualified warning",
        "source":{"source":1,"line":1,"column":1},
        "impact":{"kind":"informational"}
    }]);
    assert!(serde_json::from_value::<ScopeQueryResponse>(missing_explanation_context).is_err());
}

#[test]
fn search_rejects_old_grouped_hit_and_invalid_projection_source() {
    let mut old_group: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    let matched = &mut old_group["result"]["search"]["documents"][0]["matches"][0];
    matched.as_object_mut().unwrap().remove("location");
    matched["occurrences"] = serde_json::json!([{"matchedText":"index","root":1}]);
    assert!(serde_json::from_value::<ScopeQueryResponse>(old_group).is_err());

    let mut bad_source: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    bad_source["result"]["search"]["documents"][0]["contentProjection"]["fragments"][0]["source"] = serde_json::json!({
        "kind":"tldr", "path":"9", "startByte":0, "endByte":5
    });
    assert!(serde_json::from_value::<ScopeQueryResponse>(bad_source).is_err());
}

#[test]
fn classified_explanations_have_required_closed_shapes() {
    let expected: Value = serde_json::from_str(EXPLANATION).unwrap();
    for field in [
        "class",
        "previews",
        "previewsOmitted",
        "matchDetailsOmitted",
        "nameBindingsOmitted",
    ] {
        let mut invalid = expected.clone();
        invalid["evidence"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            serde_json::from_value::<mant_protocol::QueryExplanation>(invalid).is_err(),
            "{field}"
        );
    }
    for (pointer, field) in [
        ("", "unknown"),
        ("/counts/directEntry", "unknown"),
        ("/evidence/0", "isDirect"),
        ("/evidence/1/previews/0", "byteOffset"),
    ] {
        let mut invalid = expected.clone();
        invalid
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(field.into(), Value::Bool(true));
        assert!(
            serde_json::from_value::<mant_protocol::QueryExplanation>(invalid).is_err(),
            "{pointer}"
        );
    }
    let mut invalid = expected;
    invalid["order"] = "source-only".into();
    assert!(serde_json::from_value::<mant_protocol::QueryExplanation>(invalid).is_err());
    let mut scope: Value = serde_json::from_str(SCOPE_EXPLAIN).unwrap();
    scope["result"]["explanation"]["documents"][0]["explanation"] = serde_json::json!({});
    assert!(serde_json::from_value::<ScopeQueryResponse>(scope).is_err());
}

#[test]
fn shared_query_fixture_round_trips_without_shape_changes() {
    let query: QueryBundle = serde_json::from_str(MINIMAL_QUERY).expect("valid shared fixture");

    assert_eq!(query.schema, QuerySchema::V0Dot12);
    assert_eq!(query.label, "ls");
    let manual = query.document.as_ref().expect("manual document");
    assert_eq!(manual.source_context.sources[0].format, SourceFormat::Man);
    let document: mant_ir::Document = manual.clone().into();
    let flow = document.flow().expect("fixture has a Flow body");
    assert_eq!(
        document
            .content()
            .heading_plain_text(&flow.sections[0].heading)
            .expect("fixture heading resolves"),
        "NAME"
    );
    assert_eq!(flow.sections[1].id, "options-1");
    assert!(matches!(
        &flow.sections[0].blocks[0],
        Block::Paragraph { children, .. }
            if matches!(&children[0], Inline::Strong { .. })
    ));
    let Block::Paragraph { children, .. } = &flow.sections[0].blocks[0] else {
        panic!("NAME starts with a paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        document.content().link(inline),
        Ok(Some(link))
            if matches!(link.target(), mant_ir::LinkTarget::External { uri } if uri == "https://example.test/ls")
    )));
    assert!(children.iter().any(|inline| matches!(
        document.content().link(inline),
        Ok(Some(link))
            if matches!(link.target(), mant_ir::LinkTarget::Email { address } if address == "docs@example.test")
    )));
    assert!(children.iter().any(|inline| matches!(
        document.content().link(inline),
        Ok(Some(link))
            if matches!(link.target(), mant_ir::LinkTarget::Section { id } if id == "options-1")
    )));
    assert!(matches!(
        &flow.sections[1].blocks[0],
        Block::Paragraph { children, .. }
            if matches!(&children[0], Inline::Anchor { id, .. } if id == "all-option")
    ));

    let expected: Value = serde_json::from_str(MINIMAL_QUERY).expect("fixture JSON value");
    let actual = serde_json::to_value(query).expect("serialize query");
    assert_eq!(actual, expected);
}

#[test]
fn v0_12_breaking_projection_shapes_have_cross_language_golden_examples() {
    let outline: QueryOutline = serde_json::from_str(ROOTED_OUTLINE).expect("rooted outline");
    assert_eq!(
        outline.root,
        Some(mant_protocol::ContentSelector::id("options"))
    );
    assert!(matches!(outline.entries, EntryProjection::Kinds { .. }));
    assert_eq!(
        serde_json::to_value(outline).expect("outline value"),
        serde_json::from_str::<Value>(ROOTED_OUTLINE).expect("outline fixture")
    );

    let search: ScopeQueryResponse = serde_json::from_str(SCOPE_SEARCH).expect("scope search");
    let ScopeQueryResult::Search { search: result } = &search.result else {
        panic!("scope search wrapper");
    };
    assert_eq!(
        (result.total, result.offset, result.next_offset),
        (7, 3, Some(5))
    );
    assert_eq!(
        result
            .documents
            .iter()
            .flat_map(|document| document.matches.iter())
            .map(|matched| matched.ordinal)
            .collect::<Vec<_>>(),
        [4, 5]
    );
    assert_eq!(
        serde_json::to_value(search).expect("search value"),
        serde_json::from_str::<Value>(SCOPE_SEARCH).expect("search fixture")
    );

    let explain: ScopeQueryResponse = serde_json::from_str(SCOPE_EXPLAIN).expect("scope explain");
    assert!(matches!(
        explain.result,
        ScopeQueryResult::Explain { ref explanation } if explanation.outcome == mant_protocol::ExplanationOutcome::NoEvidence && explanation.total == 0
    ));
}

#[test]
fn unknown_query_schema_is_rejected() {
    let incompatible = MINIMAL_QUERY.replace("mant.query/v0.12", "mant.query/v1");
    let error = serde_json::from_str::<QueryBundle>(&incompatible).expect_err("unknown schema");

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn native_query_request_covers_every_projection_and_rejects_unknown_fields() {
    let request: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"printf","manualSection":"3"},"view":{"kind":"full"}}"#,
    )
    .expect("valid full query request");
    assert_eq!(request.schema, RequestSchema::V0Dot12);
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "printf".to_owned(),
            source: None,
            manual_section: Some("3".to_owned()),
        }
    );
    assert_eq!(request.view, QueryView::Full {});

    let outline: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"outline","entries":{"kind":"all"}}}"#,
    )
    .expect("valid outline request");
    assert_eq!(
        outline.view,
        QueryView::Outline {
            entries: EntryProjection::All,
            root: None,
            references: mant_protocol::ReferenceProjection::default(),
        }
    );

    let excerpt: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"excerpt","selectors":[{"kind":"id","id":"acls"}]}}"#,
    )
    .expect("valid excerpt request");
    assert_eq!(
        excerpt.view,
        QueryView::Excerpt {
            selectors: vec![mant_protocol::ContentSelector::id("acls")],
        }
    );

    let explain: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"explain","entry":"--exclude"}}"#,
    )
    .expect("valid explanation request");
    assert_eq!(
        explain.view,
        QueryView::Explain {
            entry: "--exclude".to_owned(),
            options: mant_protocol::ExplanationOptions::default()
        }
    );
}

#[test]
fn native_search_defaults_and_closed_request_fields_are_enforced() {
    let search: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"search","pattern":"--acls","syntax":"literal","case":"insensitive","word":false,"contextLines":2,"limit":20,"offset":0}}"#,
    )
    .expect("valid search request");
    assert_eq!(
        search.view,
        QueryView::Search {
            pattern: "--acls".to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Insensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 2,
            limit: 20,
            offset: 0,
        }
    );

    let search_defaults: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"search","pattern":"acls"}}"#,
    )
    .expect("search defaults");
    assert_eq!(
        search_defaults.view,
        QueryView::Search {
            pattern: "acls".to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Insensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 100,
            offset: 0,
        }
    );

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"ls"},"view":{"kind":"full"},"mode":"html"}"#,
    )
    .expect_err("unknown request field");
    assert!(error.to_string().contains("unknown field"));
    let markdown_scope: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"search","pattern":"acls","scope":"markdown"}}"#,
    )
    .expect("Markdown search scope is accepted");
    assert!(matches!(
        markdown_scope.view,
        QueryView::Search {
            scope: SearchScope::Markdown,
            ..
        }
    ));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"input":{"kind":"document","selector":"ls"},"view":{"kind":"full"}}"#,
    )
    .expect_err("missing request schema");
    assert!(error.to_string().contains("missing field `schema`"));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v1","input":{"kind":"document","selector":"ls"},"view":{"kind":"full"}}"#,
    )
    .expect_err("unknown request schema");
    assert!(error.to_string().contains("unknown variant"));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"ls"},"view":{"kind":"full","future":true}}"#,
    )
    .expect_err("unknown view field");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn request_v0_12_rejects_the_obsolete_outline_detail_field() {
    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"outline","detail":"options"}}"#,
    )
    .expect_err("request v0.12 uses the entry projection object");
    assert!(error.to_string().contains("unknown field `detail`"));
}

#[test]
fn request_v0_12_rejects_unknown_fields_inside_outline_unions() {
    for request in [
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"outline","entries":{"kind":"all","future":true}}}"#,
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"outline","entries":{"kind":"kinds","kinds":[{"kind":"parameter","parameterKind":"option","typo":true}]}}}"#,
    ] {
        let error = serde_json::from_str::<QueryRequest>(request)
            .expect_err("nested request union must be closed");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}

#[test]
fn scope_request_is_closed_bounded_and_keeps_single_document_views_separate() {
    let request: ScopeQueryRequest = serde_json::from_str(
        r#"{"schema":"mant.scope-request/v0.12","scope":{"documents":[{"selector":"git"},{"selector":"manual/1/git-add"}],"traversal":{"followLinks":true,"maxDepth":3,"maxDocuments":12}},"view":{"kind":"search","pattern":"index"}}"#,
    )
    .expect("valid scope request");
    assert_eq!(request.schema, ScopeRequestSchema::V0Dot12);
    assert_eq!(request.scope.documents.len(), 2);
    assert!(request.scope.traversal.follow_links);
    assert_eq!(request.scope.traversal.max_depth, Some(3));
    assert_eq!(request.scope.traversal.max_documents, Some(12));
    assert!(matches!(
        request.view,
        ScopeQueryView::Search {
            syntax: SearchSyntax::Literal,
            case: SearchCase::Insensitive,
            scope: SearchScope::Visible,
            limit: 100,
            offset: 0,
            ..
        }
    ));

    let error = serde_json::from_str::<ScopeQueryRequest>(
        r#"{"schema":"mant.scope-request/v0.12","scope":{"documents":[{"selector":"git"}]},"view":{"kind":"outline"}}"#,
    )
    .expect_err("outline remains a single-document request");
    assert!(error.to_string().contains("unknown variant"));

    let error = serde_json::from_str::<ScopeQueryRequest>(
        r#"{"schema":"mant.scope-request/v0.12","scope":{"documents":[{"selector":"git"}],"future":true},"view":{"kind":"explain","entry":"clone"}}"#,
    )
    .expect_err("scope objects are closed");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn request_v0_12_selects_one_configured_source_without_accepting_v4() {
    let selected: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"printf","source":"team"},"view":{"kind":"full"}}"#,
    )
    .expect("valid explicit source request");
    assert!(matches!(
        selected.input,
        QueryInput::Document {
            source: Some(ref source),
            manual_section: None,
            ..
        } if source == "team"
    ));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v4","input":{"kind":"document","selector":"ls"},"view":{"kind":"full"}}"#,
    )
    .expect_err("old request schema is intentionally unsupported");
    assert!(error.to_string().contains("unknown variant"));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"ls"},"view":{"kind":"full"}}"#,
    )
    .expect_err("the last experimental request schema is intentionally unsupported");
    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn an_empty_catalog_is_protocol_owned_and_versioned() {
    let catalog = mant_protocol::DocumentCatalog::default();

    assert_eq!(catalog.schema, mant_protocol::CatalogSchema::V0Dot12);
    assert_eq!(catalog.total, 0);
    assert_eq!(catalog.returned, 0);
    assert!(catalog.documents.is_empty());
    assert!(!catalog.truncated);
    assert_eq!(catalog.next_offset, None);
}
