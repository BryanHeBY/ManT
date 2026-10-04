//! Owner-local row layouts survive the real wire reader without entering text.

use crate::{DocumentResponse, QueryBundle};
use mant_ir::Inline;
use serde_json::{Value, json};

fn query_with_owner(owner: &str, content: &Value, layout: &Value) -> Value {
    let mut document = json!({
        "schema":"mant.document/v0.12", "producer":{"name":"test","version":"0"},
        "source":{"format":"markdown"}, "meta":{}, "sections":[]
    });
    match owner {
        "document-heading" => {
            document["heading"] = json!({"content":content,"inlineLayout":layout});
        }
        "section-heading" => {
            document["sections"] = json!([{
                "id":"section", "heading":{"content":content,"inlineLayout":layout},
                "blocks":[], "children":[]
            }]);
        }
        "paragraph" | "preformatted" => {
            document["blocks"] = json!([{
                "type":owner,"children":content,"inlineLayout":layout
            }]);
        }
        "definition-term" => {
            document["blocks"] = json!([{"type":"definition-list","items":[{
                "terms":[{"content":content,"inlineLayout":layout}],"description":[]
            }]}]);
        }
        _ => unreachable!("test owner"),
    }
    json!({"schema":"mant.query/v0.12","label":"rows","document":document})
}

const OWNERS: [&str; 5] = [
    "document-heading",
    "section-heading",
    "paragraph",
    "preformatted",
    "definition-term",
];

fn wrapped_content() -> Value {
    json!([
        {"type":"anchor","id":"visible-owner"},
        {"type":"strong","children":[
            {"type":"text","value":"A\n"},
            {"type":"emphasis","children":[{"type":"code","value":"B\n"}]}
        ]},
        {"type":"link","target":{"kind":"external","uri":"https://example.org"},
         "children":[{"type":"line-break"},{"type":"text","value":"C"}]}
    ])
}

#[test]
fn every_inline_owner_round_trips_signed_hints_and_transparent_hard_rows() {
    let content = wrapped_content();
    let layout = json!({"rowHints":[
        {"row":0,"indentColumns":2}, {"row":3,"indentColumns":-4}
    ]});
    let nodes: Vec<Inline> = serde_json::from_str(&content.to_string()).unwrap();
    assert_eq!(mant_ir::inline_plain_text(&nodes), "A\nB\n\nC");
    assert_eq!(mant_ir::logical_row_count(&nodes), 4);
    for owner in OWNERS {
        let original = query_with_owner(owner, &content, &layout);
        let decoded: QueryBundle = serde_json::from_str(&original.to_string()).unwrap();
        let wire = serde_json::to_string(&decoded).unwrap();
        let restored: QueryBundle = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), original, "{owner}");
        // A document response is independently decodable, without its bundle.
        let document: DocumentResponse =
            serde_json::from_str(&original["document"].to_string()).unwrap();
        let wire = serde_json::to_string(&document).unwrap();
        assert_eq!(
            serde_json::from_str::<DocumentResponse>(&wire).unwrap(),
            document,
            "{owner}"
        );
    }
}

#[test]
fn every_inline_owner_accepts_open_tail_positions_without_synthetic_text() {
    for (content, last_row, text) in [
        (json!([]), 0, ""),
        (json!([{"type":"anchor","id":"only-anchor"}]), 0, ""),
        (json!([{"type":"text","value":"A\n"}]), 1, "A\n"),
        (json!([{"type":"code","value":"A\n\n"}]), 2, "A\n\n"),
    ] {
        let layout = json!({"rowHints":[{"row":last_row,"indentColumns":-2}]});
        let nodes: Vec<Inline> = serde_json::from_str(&content.to_string()).unwrap();
        assert_eq!(mant_ir::inline_plain_text(&nodes), text);
        for owner in OWNERS {
            let original = query_with_owner(owner, &content, &layout);
            let decoded: QueryBundle = serde_json::from_str(&original.to_string()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), original, "{owner}");
            let invalid = query_with_owner(
                owner,
                &content,
                &json!({"rowHints":[{"row":last_row+1,"indentColumns":2}]}),
            );
            assert!(
                serde_json::from_str::<QueryBundle>(&invalid.to_string()).is_err(),
                "{owner}"
            );
        }
    }
}

#[test]
fn every_inline_owner_rejects_invalid_hint_addresses_and_closed_wire_fields() {
    let content = wrapped_content();
    for layout in [
        json!({"rowHints":[{"row":4,"indentColumns":1}]}),
        json!({"rowHints":[{"row":1,"indentColumns":1},{"row":0,"indentColumns":2}]}),
        json!({"rowHints":[{"row":1,"indentColumns":1},{"row":1,"indentColumns":2}]}),
        json!({"rowHints":[{"row":0,"indentColumns":65536}]}),
        json!({"rowHints":[{"row":0,"indentColumns":-65536}]}),
        json!({"rowHints":[{"row":-1,"indentColumns":1}]}),
        json!({"rowHints":[{"row":0,"indentColumns":null}]}),
        json!({"rowHints":[{"row":0,"indentColumns":1,"sourceLine":1}]}),
        json!({"rowHints":null}),
        json!({"indentColumns":1}),
    ] {
        for owner in OWNERS {
            let wire = query_with_owner(owner, &content, &layout).to_string();
            assert!(
                serde_json::from_str::<QueryBundle>(&wire).is_err(),
                "{owner}: {layout}"
            );
        }
    }
    for owner in OWNERS {
        let wire = query_with_owner(
            owner,
            &content,
            &json!({"rowHints":[{"row":0,"indentColumns":1}]}),
        )
        .to_string();
        let duplicate_row = wire.replacen("\"row\":0", "\"row\":0,\"row\":1", 1);
        assert!(serde_json::from_str::<QueryBundle>(&duplicate_row).is_err());
    }
}

#[test]
fn retired_break_indentation_is_rejected_inside_every_owner_and_annotation() {
    for indentation in [json!(0), json!(6), json!(-1), Value::Null] {
        let old = json!({"type":"line-break","indentColumns":indentation});
        assert!(serde_json::from_str::<Inline>(&old.to_string()).is_err());
        for nodes in [
            json!([old]),
            json!([{"type":"strong","children":[old]}]),
            json!([{"type":"emphasis","children":[old]}]),
            json!([{"type":"link","target":{"kind":"external","uri":"https://example.org"},
                    "children":[old]}]),
        ] {
            for owner in OWNERS {
                let wire = query_with_owner(owner, &nodes, &json!({})).to_string();
                assert!(
                    serde_json::from_str::<QueryBundle>(&wire).is_err(),
                    "{owner}"
                );
            }
        }
    }
    // A label owns its content and layout together; the retired parallel shape
    // cannot decode while silently discarding the owner coordinate system.
    let old_term = json!({"terms":[[{"type":"text","value":"old"}]],"description":[]});
    assert!(serde_json::from_str::<mant_ir::DefinitionItem>(&old_term.to_string()).is_err());
}
