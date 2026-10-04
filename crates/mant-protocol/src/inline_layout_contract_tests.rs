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

fn assert_object_baseline(original: &Value) {
    let wire = original.to_string();
    let query: QueryBundle =
        serde_json::from_str(&wire).unwrap_or_else(|error| panic!("{error}: {wire}"));
    assert_eq!(serde_json::to_value(query).unwrap(), *original);
    let document_wire = original["document"].to_string();
    let document: DocumentResponse = serde_json::from_str(&document_wire)
        .unwrap_or_else(|error| panic!("{error}: {document_wire}"));
    assert_eq!(
        serde_json::to_value(document).unwrap(),
        original["document"]
    );
}

fn assert_query_and_document_reject(original: &Value) {
    let wire = original.to_string();
    assert!(
        serde_json::from_str::<QueryBundle>(&wire).is_err(),
        "{wire}"
    );
    let document_wire = original["document"].to_string();
    assert!(
        serde_json::from_str::<DocumentResponse>(&document_wire).is_err(),
        "{document_wire}"
    );
}

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
fn inline_layout_requires_an_object_in_each_actual_owner_wire() {
    let content = wrapped_content();
    let layout = json!({"rowHints":[{"row":0,"indentColumns":2},{"row":3,"indentColumns":-4}]});
    assert_eq!(
        serde_json::from_str::<mant_ir::InlineLayout>("{}").unwrap(),
        mant_ir::InlineLayout::default()
    );
    assert!(serde_json::from_str::<mant_ir::InlineLayout>(&layout.to_string()).is_ok());
    for owner in OWNERS {
        assert_object_baseline(&query_with_owner(owner, &content, &layout));
        for sequence in [json!([]), json!([[]]), json!([layout["rowHints"]])] {
            assert!(
                serde_json::from_str::<mant_ir::InlineLayout>(&sequence.to_string()).is_err(),
                "accepted positional inline layout: {sequence}"
            );
            assert_query_and_document_reject(&query_with_owner(owner, &content, &sequence));
        }
    }
}

#[test]
fn definition_term_requires_an_object_in_actual_query_and_document_wire() {
    let content = wrapped_content();
    let layout = json!({"rowHints":[{"row":3,"indentColumns":-4}]});
    let baseline = query_with_owner("definition-term", &content, &layout);
    assert_object_baseline(&baseline);
    assert_eq!(
        serde_json::from_str::<mant_ir::DefinitionTerm>(r#"{"content":[],"inlineLayout":{}}"#)
            .unwrap(),
        mant_ir::DefinitionTerm::default()
    );
    let term = &baseline["document"]["blocks"][0]["items"][0]["terms"][0];
    assert!(serde_json::from_str::<mant_ir::DefinitionTerm>(&term.to_string()).is_ok());
    for sequence in [
        json!([]),
        json!([[]]),
        json!([[], {}]),
        json!([content, layout]),
        json!([{"content":[]}]),
    ] {
        assert!(
            serde_json::from_str::<mant_ir::DefinitionTerm>(&sequence.to_string()).is_err(),
            "accepted positional definition term: {sequence}"
        );
        let mut invalid = baseline.clone();
        invalid["document"]["blocks"][0]["items"][0]["terms"][0] = sequence;
        assert_query_and_document_reject(&invalid);
    }
}

#[test]
fn row_hint_requires_an_object_inside_each_actual_owner_wire() {
    let content = wrapped_content();
    let hint = json!({"row":3,"indentColumns":-4});
    assert!(serde_json::from_str::<mant_ir::RowLayoutHint>(&hint.to_string()).is_ok());
    for owner in OWNERS {
        assert_object_baseline(&query_with_owner(
            owner,
            &content,
            &json!({"rowHints":[hint]}),
        ));
        for sequence in [json!([3, -4]), json!([0, 2]), json!([]), json!([3])] {
            assert!(
                serde_json::from_str::<mant_ir::RowLayoutHint>(&sequence.to_string()).is_err(),
                "accepted positional row hint: {sequence}"
            );
            assert_query_and_document_reject(&query_with_owner(
                owner,
                &content,
                &json!({"rowHints":[sequence]}),
            ));
        }
    }
}

#[test]
fn object_decoding_preserves_explicit_zero_budget_and_owner_row_checks() {
    let content = json!([{"type":"text","value":"\n".repeat(mant_ir::MAX_INLINE_ROW_HINTS)}]);
    let mut hints = (0..mant_ir::MAX_INLINE_ROW_HINTS)
        .map(|row| json!({"row":row,"indentColumns":0}))
        .collect::<Vec<_>>();
    let layout = json!({"rowHints":hints});
    let decoded: mant_ir::InlineLayout = serde_json::from_str(&layout.to_string()).unwrap();
    assert_eq!(decoded.row_hints.len(), mant_ir::MAX_INLINE_ROW_HINTS);
    for owner in OWNERS {
        let query = query_with_owner(owner, &content, &layout);
        let wire = query.to_string();
        assert!(serde_json::from_str::<QueryBundle>(&wire).is_ok(), "{wire}");
        let document_wire = query["document"].to_string();
        assert!(
            serde_json::from_str::<DocumentResponse>(&document_wire).is_ok(),
            "{document_wire}"
        );
    }
    hints.push(json!({"row":mant_ir::MAX_INLINE_ROW_HINTS,"indentColumns":0}));
    let over_budget = json!({"rowHints":hints});
    assert!(serde_json::from_str::<mant_ir::InlineLayout>(&over_budget.to_string()).is_err());
    for owner in OWNERS {
        assert_query_and_document_reject(&query_with_owner(owner, &content, &over_budget));
        for invalid in [
            json!({"rowHints":[{"row":4,"indentColumns":0}]}),
            json!({"rowHints":[{"row":0,"indentColumns":0},{"row":0,"indentColumns":0}]}),
            json!({"rowHints":[{"row":1,"indentColumns":0},{"row":0,"indentColumns":0}]}),
        ] {
            assert_query_and_document_reject(&query_with_owner(
                owner,
                &wrapped_content(),
                &invalid,
            ));
        }
    }
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
            assert_query_and_document_reject(&query_with_owner(owner, &content, &layout));
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
