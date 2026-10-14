use mant_ir::LayoutHint;

#[test]
fn all_inline_owners_preserve_row_presentation_in_the_current_golden_wire() {
    use mant_ir::{Block, TextAlignment};
    let original = include_str!("../../../tests/contracts/row-presentation-v0.13.json");
    let query: mant_protocol::QueryBundle = serde_json::from_str(original).unwrap();
    let wire = serde_json::to_string(&query).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    assert_eq!(restored, query);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&wire).unwrap(),
        serde_json::from_str::<serde_json::Value>(original).unwrap()
    );
    let document = query.document.unwrap();
    let Block::Paragraph {
        inline_layout,
        children,
        ..
    } = &document.blocks[0]
    else {
        panic!("paragraph fixture");
    };
    assert_eq!(inline_layout.row_layout(0).alignment, TextAlignment::Center);
    assert_eq!(inline_layout.row_layout(0).first_line_indent_columns, 4);
    assert_eq!(mant_ir::inline_plain_text(children), "Paragraph\n尾行");
    let Block::Preformatted {
        inline_layout,
        children,
        ..
    } = &document.blocks[1]
    else {
        panic!("literal fixture");
    };
    assert_eq!(inline_layout.row_layout(0).alignment, TextAlignment::Right);
    assert_eq!(inline_layout.row_layout(0).indent_columns, -2);
    assert_eq!(mant_ir::inline_plain_text(children), "Literal\n尾行");
    let Block::DefinitionList { items, .. } = &document.blocks[2] else {
        panic!("definition fixture");
    };
    let term = &items[0].terms[0];
    assert_eq!(
        term.inline_layout.row_layout(0).alignment,
        TextAlignment::Right
    );
    assert_eq!(mant_ir::inline_plain_text(term.as_slice()), "Term\n尾行");
    for (heading, text) in [
        (document.heading.as_ref().unwrap(), "Title\n尾行"),
        (&document.sections[0].heading, "Heading\n尾行"),
    ] {
        assert_eq!(
            heading.inline_layout.row_layout(0).alignment,
            TextAlignment::Center
        );
        assert_eq!(
            heading
                .inline_layout
                .row_layout(0)
                .first_line_indent_columns,
            4
        );
        assert_eq!(heading.plain_text(), text);
    }
}

#[test]
fn definition_body_geometry_round_trips_independently_of_run_in_policy() {
    use mant_ir::DefinitionLayout;
    let layout: DefinitionLayout = serde_json::from_value(serde_json::json!({
        "bodyAlignment": "after-term",
        "bodyIndentColumns": -2, "minTermGapColumns": 2,
        "spacingBeforeLines": 0
    }))
    .unwrap();
    assert_eq!(layout.body_indent_columns, -2);
    assert_eq!(layout.min_term_gap_columns, 2);
    assert_eq!(
        serde_json::from_value::<DefinitionLayout>(serde_json::to_value(layout).unwrap()).unwrap(),
        layout
    );
    assert_eq!(
        serde_json::to_value(DefinitionLayout::default()).unwrap(),
        serde_json::json!({})
    );
    for invalid in [
        serde_json::json!({"bodyIndentColumns": null}),
        serde_json::json!({"minTermGapColumns": -1}),
        serde_json::json!({"sourceWidth": "7n"}),
        serde_json::json!({"inlineTerm": true}),
        serde_json::json!({"headBodyRelation": "unknown"}),
    ] {
        assert!(serde_json::from_value::<DefinitionLayout>(invalid).is_err());
    }
}

#[test]
fn definition_rows_round_trip_through_actual_current_query_json() {
    use mant_ir::{DefinitionBodyAlignment, HeadBodyRelation};
    for (relation, alignment) in [
        (
            HeadBodyRelation::Separate,
            DefinitionBodyAlignment::Indented,
        ),
        (
            HeadBodyRelation::Separate,
            DefinitionBodyAlignment::AfterTerm,
        ),
        (
            HeadBodyRelation::joined(),
            DefinitionBodyAlignment::AfterTerm,
        ),
        (
            HeadBodyRelation::joined(),
            DefinitionBodyAlignment::Indented,
        ),
        (
            HeadBodyRelation::separated(),
            DefinitionBodyAlignment::AfterTerm,
        ),
        (
            HeadBodyRelation::separated(),
            DefinitionBodyAlignment::Indented,
        ),
    ] {
        let input = serde_json::json!({
            "schema":"mant.query/v0.13", "label":"rows",
            "document":{
                "schema":"mant.document/v0.13",
                "producer":{"name":"test","version":"0"},
                "source":{"format":"mdoc"}, "meta":{}, "sections":[],
                "blocks":[{"type":"definition-list","items":[{
                    "headBodyRelation":serde_json::to_value(relation).unwrap(),
                    "layout":{"bodyAlignment":alignment},
                    "terms":[{"content":[{"type":"strong","children":[
                        {"type":"text","value":"Alpha"},
                        {"type":"line-break"},
                        {"type":"link","target":{"kind":"external","uri":"https://example.org"},
                         "children":[{"type":"text","value":"Beta"}]}
                    ]}],"inlineLayout":{"rowHints":[{"row":1,"indentColumns":6}]}}],
                    "description":[]
                }]}]
            }
        });
        let query: mant_protocol::QueryBundle =
            serde_json::from_str(&input.to_string()).expect("current query contract");
        let serialized = serde_json::to_string(&query).expect("query JSON");
        let restored: mant_protocol::QueryBundle =
            serde_json::from_str(&serialized).expect("real JSON round trip");
        let serialized: serde_json::Value = serde_json::to_value(restored).unwrap();
        assert_eq!(serialized["schema"], "mant.query/v0.13");
        let item = &serialized["document"]["blocks"][0]["items"][0];
        let restored_relation = item
            .get("headBodyRelation")
            .map_or(HeadBodyRelation::Separate, |shape| {
                serde_json::from_value(shape.clone()).unwrap()
            });
        assert_eq!(restored_relation, relation);
        let restored_alignment = item["layout"]
            .get("bodyAlignment")
            .map_or(DefinitionBodyAlignment::Indented, |shape| {
                serde_json::from_value(shape.clone()).unwrap()
            });
        assert_eq!(restored_alignment, alignment);
        assert!(item["layout"].get("headBodyRelation").is_none());
        assert!(item["layout"].get("inlineTerm").is_none());
        assert_eq!(
            item["terms"][0]["inlineLayout"]["rowHints"][0],
            serde_json::json!({"row":1,"indentColumns":6})
        );
        assert_eq!(
            item["terms"][0]["content"][0]["children"][1],
            serde_json::json!({"type":"line-break"})
        );
    }
    for input in [
        r#"{"type":"line-break","indentColumns":0}"#,
        r#"{"type":"line-break","indentColumns":6}"#,
        r#"{"type":"line-break","indentColumns":null}"#,
        r#"{"type":"line-break","indentColumns":-1}"#,
        r#"{"type":"line-break","indentColumns":65536}"#,
        r#"{"type":"line-break","indentColumns":1.5}"#,
        r#"{"type":"line-break","offset":6}"#,
    ] {
        assert!(
            serde_json::from_str::<mant_ir::Inline>(input).is_err(),
            "{input}"
        );
    }
    assert_eq!(
        serde_json::to_string(&mant_ir::Inline::line_break()).unwrap(),
        r#"{"type":"line-break"}"#
    );
}

#[test]
fn layout_origins_are_signed_and_the_wire_object_is_closed() {
    let layout: LayoutHint = serde_json::from_str(r#"{"indentColumns":-5}"#).unwrap();
    assert_eq!(layout.indent_columns, -5);
    assert_eq!(
        serde_json::to_value(layout).unwrap(),
        serde_json::json!({"indentColumns":-5})
    );
    for invalid in [
        r#"{"absoluteIndent":5}"#,
        r#"{"indentColumns":null}"#,
        r#"{"indentColumns":2147483648}"#,
        r#"{"indentColumns":1.5}"#,
        "null",
    ] {
        assert!(
            serde_json::from_str::<LayoutHint>(invalid).is_err(),
            "{invalid}"
        );
    }
    assert_eq!(
        serde_json::from_str::<LayoutHint>("{}").unwrap(),
        LayoutHint::default()
    );
}

#[test]
fn paragraph_continuation_geometry_is_not_omitted() {
    let layout = LayoutHint {
        continuation_indent_columns: 12,
        ..Default::default()
    };
    assert!(!layout.is_empty());
    assert_eq!(
        serde_json::to_value(layout).unwrap(),
        serde_json::json!({"continuationIndentColumns": 12})
    );
    assert_eq!(
        serde_json::from_str::<LayoutHint>(r#"{"continuationIndentColumns":-3}"#)
            .unwrap()
            .continuation_indent_columns,
        -3
    );
}
