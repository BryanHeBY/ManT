//! Source-neutral definition row and word relationships in unreleased v0.12.

use mant_ir::{DefinitionBodyAlignment, DefinitionLayout, HeadBodyRelation};
use serde_json::json;

use crate::{QueryBundle, inline_contract_tests::query_fixture};

fn definition_query(relation: &serde_json::Value) -> serde_json::Value {
    let mut query = query_fixture();
    query["document"]["sections"][0]["blocks"] = json!([{
        "type":"definition-list", "compact":true, "items":[{
            "terms":[[{"type":"strong","children":[{"type":"text","value":"中e\u{301}"}]}]],
            "description":[{"type":"paragraph","children":[{
                "type":"link", "target":{"kind":"external","uri":"https://ex.org"},
                "children":[{"type":"text","value":"BODY"}]
            }]}],
            "layout":{"headBodyRelation":relation}
        }]
    }]);
    query
}

#[test]
fn shared_word_boundaries_and_alignment_round_trip_independently() {
    for alignment in [
        DefinitionBodyAlignment::AfterTerm,
        DefinitionBodyAlignment::Indented,
    ] {
        for relation in [
            HeadBodyRelation::joined(alignment),
            HeadBodyRelation::separated(alignment),
        ] {
            let shape = serde_json::to_value(relation).unwrap();
            assert_eq!(shape["type"], "shared");
            let query = definition_query(&shape);
            let parsed: QueryBundle = serde_json::from_str(&query.to_string()).unwrap();
            let wire = serde_json::to_string(&parsed).unwrap();
            let restored: QueryBundle = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_value(restored).unwrap(), query);
            assert!(!wire.contains("nativeCause"));
        }
    }
    let absent: DefinitionLayout = serde_json::from_str("{}").unwrap();
    assert_eq!(absent.head_body_relation, HeadBodyRelation::Separate);
    assert_eq!(
        serde_json::to_value(HeadBodyRelation::Separate).unwrap(),
        json!({"type":"separate"})
    );
}

#[test]
fn retired_and_mixed_execution_cause_relations_are_rejected() {
    for shape in [
        json!("separate"),
        json!("run-in"),
        json!("joined-no-space"),
        json!("flush-at-body"),
        json!({"type":"run-in"}),
        json!({"type":"separate","wordBoundary":"joined"}),
        json!({"type":"shared","wordBoundary":"joined"}),
        json!({"type":"shared","bodyAlignment":"indented"}),
        json!({"type":"shared","wordBoundary":"joined","bodyAlignment":"indented","nativeCause":"flush"}),
        json!({"type":"shared","wordBoundary":"unknown","bodyAlignment":"indented"}),
        json!({"type":"shared","wordBoundary":"joined","bodyAlignment":"unknown"}),
    ] {
        assert!(
            serde_json::from_str::<HeadBodyRelation>(&shape.to_string()).is_err(),
            "{shape}"
        );
        assert!(
            serde_json::from_str::<QueryBundle>(&definition_query(&shape).to_string()).is_err(),
            "{shape}"
        );
    }
    for wire in [
        r#"{"headBodyRelation":{"type":"shared","wordBoundary":"joined","wordBoundary":"separated","bodyAlignment":"indented"}}"#,
        r#"{"headBodyRelation":{"type":"shared","wordBoundary":"joined","bodyAlignment":"indented"},"inlineTerm":true}"#,
        r#"{"headBodyRelation":{"type":"separate"},"nativeCause":"flush"}"#,
    ] {
        assert!(
            serde_json::from_str::<DefinitionLayout>(wire).is_err(),
            "{wire}"
        );
    }
}
