//! Source-neutral definition row and word relationships in unreleased v0.12.

use mant_ir::{Block, DefinitionBodyAlignment, DefinitionItem, DefinitionLayout, HeadBodyRelation};
use serde_json::json;

use crate::{QueryBundle, inline_contract_tests::query_fixture};

fn definition_query(
    relation: &serde_json::Value,
    alignment: DefinitionBodyAlignment,
) -> serde_json::Value {
    let mut query = query_fixture();
    query["document"]["sections"][0]["blocks"] = json!([{
        "type":"definition-list", "compact":true, "items":[{
            "terms":[{"content":[{"type":"strong","children":[{"type":"text","value":"中e\u{301}"}]}]}],
            "description":[{"type":"paragraph","children":[{
                "type":"link", "target":{"kind":"external","uri":"https://ex.org"},
                "children":[{"type":"text","value":"BODY"}]
            }]}],
            "headBodyRelation":relation,
            "layout":{"bodyAlignment":alignment}
        }]
    }]);
    query
}

fn definition_item(query: &QueryBundle) -> &DefinitionItem {
    let Block::DefinitionList { items, .. } =
        &query.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("definition owner")
    };
    &items[0]
}

#[test]
fn shared_word_boundaries_and_alignment_round_trip_independently() {
    for alignment in [
        DefinitionBodyAlignment::AfterTerm,
        DefinitionBodyAlignment::Indented,
    ] {
        for relation in [HeadBodyRelation::joined(), HeadBodyRelation::separated()] {
            let shape = serde_json::to_value(relation).unwrap();
            assert_eq!(shape["type"], "shared");
            let query = definition_query(&shape, alignment);
            let parsed: QueryBundle = serde_json::from_str(&query.to_string()).unwrap();
            assert_eq!(definition_item(&parsed).layout.body_alignment, alignment);
            let wire = serde_json::to_string(&parsed).unwrap();
            let restored: QueryBundle = serde_json::from_str(&wire).unwrap();
            assert_eq!(definition_item(&restored).layout.body_alignment, alignment);
            assert_eq!(restored, parsed);
            let value = serde_json::to_value(restored).unwrap();
            assert_eq!(
                value["document"]["sections"][0]["blocks"][0]["items"][0]["headBodyRelation"],
                shape
            );
            let item = &value["document"]["sections"][0]["blocks"][0]["items"][0];
            assert!(item["layout"].get("headBodyRelation").is_none());
            let encoded_alignment = item["layout"]
                .get("bodyAlignment")
                .map_or(DefinitionBodyAlignment::Indented, |value| {
                    serde_json::from_value(value.clone()).unwrap()
                });
            assert_eq!(encoded_alignment, alignment);
            assert!(!wire.contains("nativeCause"));
        }
    }
    let absent: DefinitionLayout = serde_json::from_str("{}").unwrap();
    assert_eq!(absent.body_alignment, DefinitionBodyAlignment::Indented);
    assert_eq!(
        serde_json::to_value(HeadBodyRelation::Separate).unwrap(),
        json!({"type":"separate"})
    );
}

#[test]
fn current_item_wire_rejects_duplicate_relation_boundary_and_alignment_fields() {
    let accepted = r#"{"terms":[],"description":[],"headBodyRelation":{"type":"shared","wordBoundary":"joined"},"layout":{"bodyAlignment":"after-term"}}"#;
    let item: DefinitionItem = serde_json::from_str(accepted).unwrap();
    assert_eq!(item.head_body_relation, HeadBodyRelation::joined());
    assert_eq!(
        item.layout.body_alignment,
        DefinitionBodyAlignment::AfterTerm
    );
    for wire in [
        r#"{"terms":[],"description":[],"headBodyRelation":{"type":"shared","wordBoundary":"joined","wordBoundary":"separated"},"layout":{"bodyAlignment":"after-term"}}"#,
        r#"{"terms":[],"description":[],"headBodyRelation":{"type":"shared","wordBoundary":"joined"},"headBodyRelation":{"type":"separate"},"layout":{"bodyAlignment":"after-term"}}"#,
        r#"{"terms":[],"description":[],"headBodyRelation":{"type":"shared","wordBoundary":"joined"},"layout":{"bodyAlignment":"after-term","bodyAlignment":"indented"}}"#,
        r#"{"terms":[],"description":[],"headBodyRelation":{"type":"shared","wordBoundary":"joined"},"layout":{"bodyAlignment":"unknown"}}"#,
        r#"{"terms":[],"description":[],"headBodyRelation":{"type":"shared","wordBoundary":"joined"},"layout":{"bodyAlignment":null}}"#,
    ] {
        assert!(
            serde_json::from_str::<DefinitionItem>(wire).is_err(),
            "{wire}"
        );
    }
    let mut query = definition_query(
        &json!({"type":"shared","wordBoundary":"joined"}),
        DefinitionBodyAlignment::AfterTerm,
    );
    assert!(serde_json::from_str::<QueryBundle>(&query.to_string()).is_ok());
    let item = &mut query["document"]["sections"][0]["blocks"][0]["items"][0];
    item["layout"]["headBodyRelation"] = item["headBodyRelation"].clone();
    assert!(serde_json::from_str::<QueryBundle>(&query.to_string()).is_err());
    query["document"]["sections"][0]["blocks"][0]["items"][0]
        .as_object_mut()
        .unwrap()
        .remove("headBodyRelation");
    assert!(serde_json::from_str::<QueryBundle>(&query.to_string()).is_err());
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
        json!({"type":"shared","wordBoundary":"joined","bodyAlignment":"indented"}),
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
            serde_json::from_str::<QueryBundle>(
                &definition_query(&shape, DefinitionBodyAlignment::Indented).to_string()
            )
            .is_err(),
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
