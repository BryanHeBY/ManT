//! Actual decoding guards, independent of generated schema snapshots.
use crate::{Block, DefinitionItem, EntryFacts, EntryKind, ListItem, SemanticEntry};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn cases() -> Value {
    serde_json::from_str(include_str!("convergence-wire.json")).unwrap()
}

fn definition() -> Value {
    cases()["definition"]["new"].clone()
}

fn reject_fields<T: DeserializeOwned>(value: &Value, fields: &Value) {
    for field in fields.as_array().unwrap() {
        let mut invalid = value.clone();
        invalid[field.as_str().unwrap()] = Value::Null;
        assert!(
            serde_json::from_value::<T>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}

fn roundtrip<T: DeserializeOwned + Serialize>(value: Value) -> Value {
    let decoded: T = serde_json::from_value(value).unwrap();
    let canonical = serde_json::to_value(decoded).unwrap();
    let _: T = serde_json::from_value(canonical.clone()).unwrap();
    canonical
}

#[test]
fn owners_reject_old_and_unknown_fields_including_mixed_shapes() {
    let definition = definition();
    assert!(
        serde_json::from_value::<DefinitionItem>(cases()["definition"]["old"].clone()).is_err()
    );
    reject_fields::<DefinitionItem>(&definition, &cases()["definition"]["rejectFields"]);
    reject_fields::<ListItem>(
        &cases()["listItem"]["new"],
        &cases()["listItem"]["rejectFields"],
    );
    for entry in [None, Some(Value::Null), Some(definition["entry"].clone())] {
        let mut list = json!({"blocks": [], "source": null});
        if let Some(entry) = entry {
            list["entry"] = entry;
        }
        let canonical = roundtrip::<ListItem>(list);
        assert!(!canonical.as_object().unwrap().contains_key("source"));
        assert!(canonical.get("entry").is_none_or(Value::is_object));
    }
    let mut block = json!({"type": "definition-list", "items": [definition]});
    let _ = roundtrip::<Block>(block.clone());
    block["items"][0]["identity"] = Value::Null;
    assert!(serde_json::from_value::<Block>(block).is_err());
    assert!(
        serde_json::from_str::<ListItem>(r#"{"blocks":[],"entry":null,"entry":null}"#).is_err()
    );
    assert!(
        serde_json::from_str::<DefinitionItem>(
            r#"{"terms":[],"description":[],"source":null,"source":null}"#
        )
        .is_err()
    );
}

#[test]
fn facts_are_closed_but_content_references_are_semantically_validated() {
    let facts = definition()["entry"].clone();
    reject_fields::<EntryFacts>(&facts, &cases()["facts"]["rejectFields"]);
    for invalid in [Value::Null, json!({}), json!("implicit")] {
        let mut value = facts.clone();
        value["forms"] = invalid;
        assert!(serde_json::from_value::<EntryFacts>(value).is_err());
    }
    for forms in [None, Some(json!([]))] {
        let mut value = facts.clone();
        value.as_object_mut().unwrap().remove("forms");
        if let Some(forms) = forms {
            value["forms"] = forms;
        }
        assert_eq!(
            serde_json::from_value::<EntryFacts>(value)
                .unwrap()
                .forms
                .len(),
            0
        );
    }
    let mut bad_reference = facts.clone();
    bad_reference["forms"][0]["parts"][0]["root"]["index"] = json!(999);
    let mut owner: DefinitionItem = serde_json::from_value(definition()).unwrap();
    owner.entry = Some(serde_json::from_value(bad_reference).unwrap());
    assert!(crate::EntryOwner::Definition(&owner).forms().is_none());
    for kind in [
        json!("option"),
        json!({"kind":"command","unknown":null}),
        json!({"kind":"parameter","parameterKind":"wrong"}),
    ] {
        assert!(serde_json::from_value::<EntryKind>(kind).is_err());
    }
    let raw = serde_json::to_string(&facts).unwrap();
    let duplicate = raw.replacen('{', "{\"case\":\"sensitive\",", 1);
    assert!(serde_json::from_str::<EntryFacts>(&duplicate).is_err());
}

#[test]
fn derived_names_are_not_a_second_alias_vocabulary() {
    let entry = json!({"id":"example", "kind":{"kind":"command"},
        "case":"sensitive", "names":["example"], "forms":["example"],
        "aliasGroups":[["example"]], "aliasOf":"other"});
    reject_fields::<SemanticEntry>(&entry, &cases()["semanticEntry"]["rejectFields"]);
    let canonical = roundtrip::<SemanticEntry>(entry);
    assert_eq!(canonical["names"], json!(["example"]));
    assert_eq!(canonical["aliasOf"], "other");
    assert!(serde_json::from_str::<SemanticEntry>(r#"{"id":"x","kind":{"kind":"term"},"case":"sensitive","forms":[],"names":[],"names":[]}"#).is_err());
}

#[test]
fn definition_layout_keeps_inherited_and_explicit_zero_spacing_distinct() {
    use crate::DefinitionLayout;
    for value in cases()["layout"]["accept"].as_array().unwrap() {
        let layout: DefinitionLayout = serde_json::from_value(value.clone()).unwrap();
        let canonical = serde_json::to_value(layout).unwrap();
        assert_eq!(
            canonical.get("spacingBeforeLines"),
            value.get("spacingBeforeLines").filter(|v| !v.is_null())
        );
        let mut item = definition();
        item["layout"] = value.clone();
        let output = roundtrip::<DefinitionItem>(item);
        assert_eq!(output.get("layout").is_none(), layout.is_empty());
    }
    for value in cases()["layout"]["reject"].as_array().unwrap() {
        assert!(serde_json::from_value::<DefinitionLayout>(value.clone()).is_err());
        let mut item = definition();
        item["layout"] = value.clone();
        assert!(serde_json::from_value::<DefinitionItem>(item).is_err());
    }
    for invalid in [
        r#"{"headBodyRelation":"run-in","headBodyRelation":"separate"}"#,
        r#"{"spacingBeforeLines":null,"spacingBeforeLines":0}"#,
        r#"{"spacingBeforeLines":-1}"#,
        r#"{"headBodyRelation":null}"#,
    ] {
        assert!(
            serde_json::from_str::<DefinitionLayout>(invalid).is_err(),
            "{invalid}"
        );
    }
    let inherited: DefinitionItem =
        serde_json::from_value(json!({"terms":[],"description":[]})).unwrap();
    assert_eq!(inherited.layout, DefinitionLayout::default());
}

#[test]
fn only_ordered_lists_accept_start_in_actual_decoders() {
    use crate::ListKind;
    for value in cases()["listKind"]["accept"].as_array().unwrap() {
        let canonical = roundtrip::<ListKind>(value.clone());
        assert!(canonical.get("start").is_none_or(|start| !start.is_null()));
        let block = json!({"type":"list", "kind":value, "items":[]});
        let _ = roundtrip::<Block>(block.clone());
        let mut legacy = block;
        legacy["start"] = Value::Null;
        assert!(serde_json::from_value::<Block>(legacy).is_err());
    }
    for invalid in cases()["listKind"]["reject"].as_array().unwrap() {
        assert!(
            serde_json::from_value::<ListKind>(invalid.clone()).is_err(),
            "{invalid}"
        );
        assert!(
            serde_json::from_value::<Block>(json!({"type":"list","kind":invalid,"items":[]}))
                .is_err()
        );
    }
    for raw in cases()["listKind"]["rejectRaw"].as_array().unwrap() {
        assert!(
            serde_json::from_str::<ListKind>(raw.as_str().unwrap()).is_err(),
            "{raw}"
        );
    }
    for start in [None, Some(0), Some(7), Some(u64::MAX)] {
        let kind = ListKind::Ordered { start };
        for offset in [0, 1, 3, usize::MAX] {
            let expected = start
                .unwrap_or(1)
                .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
            assert_eq!(kind.ordinal(offset), Some(expected));
            assert_eq!(
                kind.for_excerpt(offset),
                ListKind::Ordered {
                    start: Some(expected)
                }
            );
            assert_eq!(kind, ListKind::Ordered { start });
        }
    }
    for kind in [ListKind::Bullet, ListKind::Dash, ListKind::Plain] {
        assert_eq!(kind.ordinal(0), None);
        assert_eq!(kind.for_excerpt(8), kind);
    }
}

#[test]
fn definition_relations_round_trip_independent_word_and_alignment_facts() {
    use crate::{
        DefinitionBodyAlignment, DefinitionLayout, DefinitionWordBoundary, HeadBodyRelation,
    };
    for word_boundary in [
        DefinitionWordBoundary::Joined,
        DefinitionWordBoundary::Separated,
    ] {
        for body_alignment in [
            DefinitionBodyAlignment::AfterTerm,
            DefinitionBodyAlignment::Indented,
        ] {
            let layout = DefinitionLayout {
                head_body_relation: HeadBodyRelation::Shared {
                    word_boundary,
                    body_alignment,
                },
                body_indent_columns: -7,
                min_term_gap_columns: 2,
                spacing_before_lines: Some(0),
            };
            let wire = serde_json::to_string(&layout).unwrap();
            assert_eq!(
                serde_json::from_str::<DefinitionLayout>(&wire).unwrap(),
                layout
            );
            let value: Value = serde_json::from_str(&wire).unwrap();
            assert_eq!(value["headBodyRelation"]["type"], "shared");
            assert!(!wire.contains("inlineTerm"));
            assert!(!wire.contains("flush-at-body"));
        }
    }
    for relation in [
        json!("separate"),
        json!("run-in"),
        json!("joined-no-space"),
        json!("flush-at-body"),
        json!({"type":"shared"}),
        json!({"type":"shared","wordBoundary":"joined"}),
        json!({"type":"shared","wordBoundary":"unknown","bodyAlignment":"indented"}),
        json!({"type":"shared","wordBoundary":"joined","bodyAlignment":"unknown"}),
        json!({"type":"shared","wordBoundary":"joined","bodyAlignment":"indented","unknown":0}),
        json!({"type":"separate","wordBoundary":"joined"}),
    ] {
        let wire = json!({"headBodyRelation":relation}).to_string();
        assert!(
            serde_json::from_str::<DefinitionLayout>(&wire).is_err(),
            "{wire}"
        );
    }
    for invalid in [
        json!({"headBodyRelation":{"type":"shared","wordBoundary":"joined","bodyAlignment":"indented"},"inlineTerm":true}),
        json!({"headBodyRelation":{"type":"shared","wordBoundary":"joined","bodyAlignment":"indented"},"minTermGapColumns":65536}),
    ] {
        assert!(serde_json::from_str::<DefinitionLayout>(&invalid.to_string()).is_err());
    }
}

#[test]
fn shared_definition_gap_preserves_word_boundaries_under_origin_translation() {
    use crate::{
        DefinitionBodyAlignment, DefinitionLayout, DefinitionWordBoundary, HeadBodyRelation,
    };
    for parent in [-11, 0, 9] {
        for word_boundary in [
            DefinitionWordBoundary::Joined,
            DefinitionWordBoundary::Separated,
        ] {
            for body_alignment in [
                DefinitionBodyAlignment::AfterTerm,
                DefinitionBodyAlignment::Indented,
            ] {
                let layout = DefinitionLayout {
                    head_body_relation: HeadBodyRelation::Shared {
                        word_boundary,
                        body_alignment,
                    },
                    body_indent_columns: 12,
                    min_term_gap_columns: 1,
                    ..Default::default()
                };
                let gap = crate::geometry::definition_body_gap(&layout, parent, 3, parent + 12);
                let expected = match (word_boundary, body_alignment) {
                    (DefinitionWordBoundary::Joined, _) => 0,
                    (DefinitionWordBoundary::Separated, DefinitionBodyAlignment::AfterTerm) => 1,
                    (DefinitionWordBoundary::Separated, DefinitionBodyAlignment::Indented) => {
                        crate::geometry::padding(parent + 12)
                            .saturating_sub(crate::geometry::padding(parent) + 3)
                            .max(1)
                    }
                };
                assert_eq!(gap, expected, "{parent}: {layout:?}");
            }
        }
    }
}
