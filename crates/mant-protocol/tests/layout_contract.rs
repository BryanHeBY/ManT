use mant_ir::LayoutHint;

#[test]
fn definition_body_geometry_round_trips_independently_of_run_in_policy() {
    use mant_ir::DefinitionLayout;
    let layout: DefinitionLayout = serde_json::from_value(serde_json::json!({
        "inlineTerm": true, "bodyIndentColumns": -2, "minTermGapColumns": 2,
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
    ] {
        assert!(serde_json::from_value::<DefinitionLayout>(invalid).is_err());
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
