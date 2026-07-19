//! Cross-language golden contract tests for the first stable query schema.

use mant_ast::{Block, Inline, QueryBundle, QueryRequest, QuerySchema, SourceFormat};
use serde_json::Value;

const MINIMAL_QUERY: &str = include_str!("../../../../tests/contracts/minimal-query-v1.json");

#[test]
fn shared_query_fixture_round_trips_without_shape_changes() {
    let query: QueryBundle = serde_json::from_str(MINIMAL_QUERY).expect("valid shared fixture");

    assert_eq!(query.schema, QuerySchema::V1);
    assert_eq!(query.topic, "ls");
    let manual = query.manual.as_ref().expect("manual document");
    assert_eq!(manual.source.format, SourceFormat::Man);
    assert_eq!(manual.sections[0].title, "NAME");
    assert!(matches!(
        &manual.sections[0].blocks[0],
        Block::Paragraph { children, .. }
            if matches!(&children[0], Inline::Strong { .. })
    ));

    let expected: Value = serde_json::from_str(MINIMAL_QUERY).expect("fixture JSON value");
    let actual = serde_json::to_value(query).expect("serialize query");
    assert_eq!(actual, expected);
}

#[test]
fn unknown_query_schema_is_rejected() {
    let incompatible = MINIMAL_QUERY.replace("mant.query/v1", "mant.query/v2");
    let error = serde_json::from_str::<QueryBundle>(&incompatible).expect_err("unknown schema");

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn native_query_request_is_small_and_rejects_unknown_fields() {
    let request: QueryRequest =
        serde_json::from_str(r#"{"topic":"printf","section":"3"}"#).expect("valid query request");
    assert_eq!(request.topic, "printf");
    assert_eq!(request.section.as_deref(), Some("3"));

    let error = serde_json::from_str::<QueryRequest>(r#"{"topic":"ls","mode":"html"}"#)
        .expect_err("unknown request field");
    assert!(error.to_string().contains("unknown field"));
}
