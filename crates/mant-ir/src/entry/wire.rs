//! Actual decoding guards, independent of generated schema snapshots.
use crate::{Block, DefinitionItem, EntryFacts, EntryKind, ListItem, SemanticEntry};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn cases() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/ir/convergence-wire.json"
    ))
    .unwrap()
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
        assert!(
            serde_json::from_value::<EntryFacts>(value)
                .unwrap()
                .forms
                .is_empty()
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
        r#"{"inlineTerm":true,"inlineTerm":false}"#,
        r#"{"spacingBeforeLines":null,"spacingBeforeLines":0}"#,
        r#"{"spacingBeforeLines":-1}"#,
        r#"{"inlineTerm":null}"#,
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
