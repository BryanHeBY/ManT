//! Cross-language golden tests for the versioned query protocol suite.

use mant_ir::{Block, Inline, SourceFormat};
use mant_protocol::{
    EntryProjection, QueryBundle, QueryInput, QueryOutline, QueryRequest, QuerySchema, QueryView,
    RequestSchema, ScopeQueryRequest, ScopeQueryResponse, ScopeQueryResult, ScopeQueryView,
    ScopeRequestSchema, SearchCase, SearchScope, SearchSyntax,
};
use serde_json::Value;

const MINIMAL_QUERY: &str = include_str!("../../../tests/contracts/minimal-query-v0.12.json");
const ROOTED_OUTLINE: &str = include_str!("../../../tests/contracts/rooted-outline-v0.12.json");
const SCOPE_SEARCH: &str = include_str!("../../../tests/contracts/scope-search-v0.12.json");
const SCOPE_EXPLAIN: &str = include_str!("../../../tests/contracts/scope-explain-v0.12.json");
const EXPLANATION: &str = include_str!("../../../tests/contracts/explanation-v0.12.json");

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
    assert_eq!(manual.source.format, SourceFormat::Man);
    assert_eq!(manual.sections[0].heading.plain_text(), "NAME");
    assert_eq!(manual.sections[1].id, "options-1");
    assert!(matches!(
        &manual.sections[0].blocks[0],
        Block::Paragraph { children, .. }
            if matches!(&children[0], Inline::Strong { .. })
    ));
    let Block::Paragraph { children, .. } = &manual.sections[0].blocks[0] else {
        panic!("NAME starts with a paragraph");
    };
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. } if uri == "https://example.test/ls")
    ));
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Email { address }, .. } if address == "docs@example.test")
    ));
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Section { id: target }, .. } if target == "options-1")
    ));
    assert!(matches!(
        &manual.sections[1].blocks[0],
        Block::Paragraph { children, .. }
            if matches!(&children[0], Inline::Anchor { id, .. } if id == "all-option")
    ));

    let expected: Value = serde_json::from_str(MINIMAL_QUERY).expect("fixture JSON value");
    let actual = serde_json::to_value(query).expect("serialize query");
    assert_eq!(actual, expected);
}

#[test]
fn search_rejects_a_complete_summary_after_known_content_loss() {
    let mut search = serde_json::json!({
        "schema": "mant.search/v0.12",
        "label": "review",
        "diagnostics": [{
            "level": "warning",
            "impact": "content-coverage",
            "message": "visible content omitted"
        }],
        "query": {"pattern": "x"},
        "render": {
            "schema": "mant.markdown/v1",
            "format": "markdown",
            "scope": "full",
            "lineBase": 1,
            "columnBase": 1,
            "lineCount": 1
        },
        "total": 0,
        "returned": 0,
        "offset": 0,
        "truncated": false,
        "matches": []
    });
    let wire = serde_json::to_string(&search).unwrap();
    assert!(serde_json::from_str::<mant_protocol::QuerySearch>(&wire).is_err());
    assert!(serde_json::from_value::<mant_protocol::QuerySearch>(search.clone()).is_err());

    search["contentComplete"] = true.into();
    let wire = serde_json::to_string(&search).unwrap();
    assert!(serde_json::from_str::<mant_protocol::QuerySearch>(&wire).is_err());
    assert!(serde_json::from_value::<mant_protocol::QuerySearch>(search.clone()).is_err());

    search["contentComplete"] = false.into();
    let wire = serde_json::to_string(&search).unwrap();
    let retained: mant_protocol::QuerySearch = serde_json::from_str(&wire).unwrap();
    assert!(!retained.content_complete);

    // A bounded transport can omit diagnostic detail while retaining the
    // producer's false summary across a text JSON round trip.
    search.as_object_mut().unwrap().remove("diagnostics");
    let wire = serde_json::to_string(&search).unwrap();
    let bounded: mant_protocol::QuerySearch = serde_json::from_str(&wire).unwrap();
    assert!(!bounded.content_complete);
    assert!(bounded.diagnostics.is_empty());
}

fn assert_inbound_coverage_contract<T: serde::de::DeserializeOwned>(
    mut value: Value,
    pointer: &str,
    has_semantics_summary: bool,
) {
    let response = value.pointer_mut(pointer).unwrap().as_object_mut().unwrap();
    response.insert(
        "diagnostics".into(),
        serde_json::json!([{
            "level": "warning",
            "impact": "content-coverage",
            "message": "known source content loss"
        }]),
    );
    if has_semantics_summary {
        response.insert("semanticsComplete".into(), false.into());
    }
    response.remove("contentComplete");
    assert!(serde_json::from_str::<T>(&value.to_string()).is_err());
    assert!(serde_json::from_value::<T>(value.clone()).is_err());

    value.pointer_mut(pointer).unwrap()["contentComplete"] = true.into();
    assert!(serde_json::from_str::<T>(&value.to_string()).is_err());

    value.pointer_mut(pointer).unwrap()["contentComplete"] = false.into();
    let retained = serde_json::from_str::<T>(&value.to_string());
    assert!(
        retained.is_ok(),
        "{} with retained details: {:?}",
        std::any::type_name::<T>(),
        retained.err()
    );
    // Some envelopes require the diagnostic field even after a bounded
    // transport removes every detail; an empty list is the common shape.
    value.pointer_mut(pointer).unwrap()["diagnostics"] = serde_json::json!([]);
    let bounded = serde_json::from_str::<T>(&value.to_string());
    assert!(
        bounded.is_ok(),
        "{} without details: {:?}",
        std::any::type_name::<T>(),
        bounded.err()
    );

    if has_semantics_summary {
        value.pointer_mut(pointer).unwrap()["semanticsComplete"] = true.into();
        assert!(serde_json::from_str::<T>(&value.to_string()).is_err());

        value.pointer_mut(pointer).unwrap()["contentComplete"] = true.into();
        value.pointer_mut(pointer).unwrap()["diagnostics"] = serde_json::json!([{
            "level": "warning",
            "impact": "semantic-coverage",
            "message": "known semantic evidence loss"
        }]);
        assert!(serde_json::from_str::<T>(&value.to_string()).is_err());
        value.pointer_mut(pointer).unwrap()["semanticsComplete"] = false.into();
        assert!(serde_json::from_str::<T>(&value.to_string()).is_ok());
    }
}

#[test]
fn all_document_derived_responses_reject_contradictory_coverage_summaries() {
    let outline: Value = serde_json::from_str(ROOTED_OUTLINE).unwrap();
    assert_inbound_coverage_contract::<QueryOutline>(outline, "", true);

    let excerpt = serde_json::json!({
        "schema": "mant.excerpt/v0.12",
        "label": "review",
        "selections": []
    });
    assert_inbound_coverage_contract::<mant_protocol::QueryExcerpt>(excerpt, "", true);

    let explanation: Value = serde_json::from_str(EXPLANATION).unwrap();
    assert_inbound_coverage_contract::<mant_protocol::QueryExplanation>(explanation, "", true);

    let mut scope_search: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    scope_search["result"]["search"]["contentComplete"] = false.into();
    assert_inbound_coverage_contract::<ScopeQueryResponse>(
        scope_search.clone(),
        "/result/search/documents/0",
        false,
    );
    let mut inconsistent_aggregate = scope_search;
    inconsistent_aggregate["result"]["search"]["documents"][0]["contentComplete"] = false.into();
    inconsistent_aggregate["result"]["search"]["contentComplete"] = true.into();
    assert!(serde_json::from_value::<ScopeQueryResponse>(inconsistent_aggregate.clone()).is_err());
    inconsistent_aggregate["result"]["search"]["contentComplete"] = false.into();
    assert!(serde_json::from_value::<ScopeQueryResponse>(inconsistent_aggregate).is_ok());

    let scope_explanation: Value = serde_json::from_str(SCOPE_EXPLAIN).unwrap();
    assert_inbound_coverage_contract::<ScopeQueryResponse>(
        scope_explanation,
        "/result/explanation/documents/0",
        true,
    );
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
fn v0_12_rejects_v0_11_and_mixed_query_document_markers() {
    let legacy = include_str!("../../../tests/contracts/minimal-query-v0.11.json");
    assert!(serde_json::from_str::<QueryBundle>(legacy).is_err());
    let mixed = MINIMAL_QUERY.replace("mant.document/v0.12", "mant.document/v0.11");
    assert!(serde_json::from_str::<QueryBundle>(&mixed).is_err());
}

#[test]
fn v0_12_scope_search_rejects_unknown_fields_at_each_changed_level() {
    let original: Value = serde_json::from_str(SCOPE_SEARCH).unwrap();
    for pointer in ["", "/result/search", "/result/search/documents/0"] {
        let mut value = original.clone();
        let object = value.pointer_mut(pointer).unwrap().as_object_mut().unwrap();
        object.insert("future".into(), true.into());
        assert!(
            serde_json::from_value::<ScopeQueryResponse>(value).is_err(),
            "{pointer}"
        );
    }
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
        r#"{"schema":"mant.request/v0.12","input":{"kind":"document","selector":"tar"},"view":{"kind":"search","pattern":"--acls","syntax":"literal","case":"insensitive","scope":"visible","word":false,"contextLines":2,"limit":20,"offset":0}}"#,
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
