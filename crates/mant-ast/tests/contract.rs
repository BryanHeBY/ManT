//! Cross-language golden tests for the versioned query protocol suite.

use mant_ast::{
    Block, Inline, OutlineDetail, QueryBundle, QueryInput, QueryRequest, QuerySchema, QueryView,
    RequestSchema, SearchCase, SearchScope, SearchSyntax, SourceFormat,
};
use serde_json::Value;

const MINIMAL_QUERY: &str = include_str!("../../../tests/contracts/minimal-query-v4.json");

#[test]
fn shared_query_fixture_round_trips_without_shape_changes() {
    let query: QueryBundle = serde_json::from_str(MINIMAL_QUERY).expect("valid shared fixture");

    assert_eq!(query.schema, QuerySchema::V4);
    assert_eq!(query.label, "ls");
    let manual = query.document.as_ref().expect("manual document");
    assert_eq!(manual.source.format, SourceFormat::Man);
    assert_eq!(manual.sections[0].title, "NAME");
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
        |inline| matches!(inline, Inline::ExternalLink { uri, .. } if uri == "https://example.test/ls")
    ));
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::EmailLink { address, .. } if address == "docs@example.test")
    ));
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::SectionReference { target, .. } if target == "options-1")
    ));
    assert!(matches!(
        &manual.sections[1].blocks[0],
        Block::Paragraph { children, .. }
            if matches!(&children[0], Inline::Anchor { id } if id == "all-option")
    ));

    let expected: Value = serde_json::from_str(MINIMAL_QUERY).expect("fixture JSON value");
    let actual = serde_json::to_value(query).expect("serialize query");
    assert_eq!(actual, expected);
}

#[test]
fn unknown_query_schema_is_rejected() {
    let incompatible = MINIMAL_QUERY.replace("mant.query/v4", "mant.query/v1");
    let error = serde_json::from_str::<QueryBundle>(&incompatible).expect_err("unknown schema");

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn native_query_request_covers_every_projection_and_rejects_unknown_fields() {
    let request: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"printf","section":"3"},"view":{"kind":"full"}}"#,
    )
    .expect("valid full query request");
    assert_eq!(request.schema, RequestSchema::V5);
    assert_eq!(
        request.input,
        QueryInput::Document {
            name: "printf".to_owned(),
            source: None,
            section: Some("3".to_owned()),
        }
    );
    assert_eq!(request.view, QueryView::Full {});

    let outline: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"tar"},"view":{"kind":"outline","detail":"entries"}}"#,
    )
    .expect("valid outline request");
    assert_eq!(
        outline.view,
        QueryView::Outline {
            detail: OutlineDetail::Entries,
        }
    );

    let excerpt: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"tar"},"view":{"kind":"excerpt","nodes":["acls"]}}"#,
    )
    .expect("valid excerpt request");
    assert_eq!(
        excerpt.view,
        QueryView::Excerpt {
            nodes: vec!["acls".to_owned()],
        }
    );

    let explain: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"tar"},"view":{"kind":"explain","entry":"--exclude"}}"#,
    )
    .expect("valid explanation request");
    assert_eq!(
        explain.view,
        QueryView::Explain {
            entry: "--exclude".to_owned(),
        }
    );

    let search: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"tar"},"view":{"kind":"search","pattern":"--acls","syntax":"literal","case":"insensitive","scope":"visible","word":false,"contextLines":2,"limit":20,"offset":0}}"#,
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
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"tar"},"view":{"kind":"search","pattern":"acls"}}"#,
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
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"ls"},"view":{"kind":"full"},"mode":"html"}"#,
    )
    .expect_err("unknown request field");
    assert!(error.to_string().contains("unknown field"));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"input":{"kind":"document","name":"ls"},"view":{"kind":"full"}}"#,
    )
    .expect_err("missing request schema");
    assert!(error.to_string().contains("missing field `schema`"));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v1","input":{"kind":"document","name":"ls"},"view":{"kind":"full"}}"#,
    )
    .expect_err("unknown request schema");
    assert!(error.to_string().contains("unknown variant"));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"ls"},"view":{"kind":"full","future":true}}"#,
    )
    .expect_err("unknown view field");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn request_v5_selects_one_configured_source_without_accepting_v4() {
    let selected: QueryRequest = serde_json::from_str(
        r#"{"schema":"mant.request/v5","input":{"kind":"document","name":"printf","source":"team"},"view":{"kind":"full"}}"#,
    )
    .expect("valid explicit source request");
    assert!(matches!(
        selected.input,
        QueryInput::Document {
            source: Some(ref source),
            section: None,
            ..
        } if source == "team"
    ));

    let error = serde_json::from_str::<QueryRequest>(
        r#"{"schema":"mant.request/v4","input":{"kind":"document","name":"ls"},"view":{"kind":"full"}}"#,
    )
    .expect_err("old request schema is intentionally unsupported");
    assert!(error.to_string().contains("unknown variant"));
}
