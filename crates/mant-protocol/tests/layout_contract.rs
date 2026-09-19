use mant_ir::LayoutHint;

#[test]
fn definition_body_geometry_round_trips_with_closed_conditional_placement() {
    use mant_ir::{DefinitionLayout, DefinitionPlacement};
    let layout: DefinitionLayout = serde_json::from_value(serde_json::json!({
        "placement": "fit", "bodyIndentColumns": -2, "minTermGapColumns": 2,
        "spacingBeforeLines": 0
    }))
    .unwrap();
    assert_eq!(layout.body_indent_columns, -2);
    assert_eq!(layout.min_term_gap_columns, 2);
    assert_eq!(layout.placement, DefinitionPlacement::Fit);
    assert_eq!(
        serde_json::from_value::<DefinitionLayout>(serde_json::to_value(layout).unwrap()).unwrap(),
        layout
    );
    assert_eq!(
        serde_json::to_value(DefinitionLayout::default()).unwrap(),
        serde_json::json!({})
    );
    for (wire, placement, canonical) in [
        ("{}", DefinitionPlacement::Stacked, serde_json::json!({})),
        (
            r#"{"placement":"stacked"}"#,
            DefinitionPlacement::Stacked,
            serde_json::json!({}),
        ),
        (
            r#"{"placement":"run-in"}"#,
            DefinitionPlacement::RunIn,
            serde_json::json!({"placement":"run-in"}),
        ),
        (
            r#"{"placement":"fit"}"#,
            DefinitionPlacement::Fit,
            serde_json::json!({"placement":"fit"}),
        ),
    ] {
        let decoded: DefinitionLayout = serde_json::from_str(wire).unwrap();
        assert_eq!(decoded.placement, placement);
        assert_eq!(serde_json::to_value(decoded).unwrap(), canonical);
    }
    for invalid in [
        serde_json::json!({"bodyIndentColumns": null}),
        serde_json::json!({"minTermGapColumns": -1}),
        serde_json::json!({"sourceWidth": "7n"}),
        serde_json::json!({"inlineTerm": true}),
        serde_json::json!({"placement": null}),
        serde_json::json!({"placement": "runIn"}),
        serde_json::json!({"placement": "unknown"}),
        serde_json::json!({"placement": 1}),
    ] {
        assert!(serde_json::from_value::<DefinitionLayout>(invalid).is_err());
    }
    for invalid in [
        r#"{"placement":"fit","placement":"run-in"}"#,
        r#"{"placement":"fit","inlineTerm":true}"#,
    ] {
        assert!(serde_json::from_str::<DefinitionLayout>(invalid).is_err());
    }
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
