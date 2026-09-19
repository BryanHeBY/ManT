use mant_ir::LayoutHint;

#[test]
fn definition_body_geometry_round_trips_with_closed_conditional_placement() {
    use mant_ir::{DefinitionLayout, DefinitionPlacement};
    let layout: DefinitionLayout = serde_json::from_value(serde_json::json!({
        "placement": "fit", "bodyIndentColumns": -2, "minTermGapColumns": 2,
        "termContinuationIndentColumns": -3,
        "fitConstraint": {
            "fitContentBasicUnits": 216,
            "fieldBasicUnits": 240,
            "originPhaseBasicUnits": 0,
            "cellBasicUnits": 24,
            "forcedSeparation": false
        },
        "spacingBeforeLines": 0
    }))
    .unwrap();
    assert_eq!(layout.body_indent_columns, -2);
    assert_eq!(layout.min_term_gap_columns, 2);
    assert_eq!(layout.term_continuation_indent_columns, -3);
    let constraint = layout.fit_constraint.expect("native fit constraint");
    assert_eq!(constraint.fit_content_basic_units, 216);
    assert_eq!(constraint.field_basic_units, 240);
    assert_eq!(constraint.origin_phase_basic_units, 0);
    assert_eq!(constraint.cell_basic_units.get(), 24);
    assert!(!constraint.forced_separation);
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
        serde_json::json!({"termContinuationIndentColumns": null}),
        serde_json::json!({"termContinuationIndentColumns": 2_147_483_648_i64}),
        serde_json::json!({"fitConstraint": null}),
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
        r#"{"termContinuationIndentColumns":1,"termContinuationIndentColumns":2}"#,
        r#"{"fitConstraint":{"fitContentBasicUnits":1,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":1,"forcedSeparation":false},"fitConstraint":{"fitContentBasicUnits":1,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":1,"forcedSeparation":false}}"#,
        r#"{"placement":"fit","inlineTerm":true}"#,
    ] {
        assert!(serde_json::from_str::<DefinitionLayout>(invalid).is_err());
    }
}

#[test]
fn native_fit_constraint_is_present_closed_nonnullable_and_bounded() {
    use mant_ir::DefinitionLayout;

    let valid = r#"{"placement":"fit","fitConstraint":{"fitContentBasicUnits":18446744073709551615,"fieldBasicUnits":18446744073709551615,"originPhaseBasicUnits":23,"cellBasicUnits":24,"forcedSeparation":true}}"#;
    let decoded: DefinitionLayout = serde_json::from_str(valid).unwrap();
    let constraint = decoded.fit_constraint.unwrap();
    assert_eq!(constraint.fit_content_basic_units, u64::MAX);
    assert_eq!(constraint.field_basic_units, u64::MAX);
    assert_eq!(constraint.origin_phase_basic_units, 23);
    assert!(constraint.forced_separation);

    let zero_semantics: DefinitionLayout = serde_json::from_str(
        r#"{"placement":"fit","fitConstraint":{"fitContentBasicUnits":0,"fieldBasicUnits":0,"originPhaseBasicUnits":0,"cellBasicUnits":1,"forcedSeparation":false}}"#,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(zero_semantics).unwrap(),
        serde_json::json!({
            "placement": "fit",
            "fitConstraint": {
                "fitContentBasicUnits": 0,
                "fieldBasicUnits": 0,
                "originPhaseBasicUnits": 0,
                "cellBasicUnits": 1,
                "forcedSeparation": false
            }
        })
    );

    let fields = [
        ("fitContentBasicUnits", "1"),
        ("fieldBasicUnits", "2"),
        ("originPhaseBasicUnits", "0"),
        ("cellBasicUnits", "24"),
        ("forcedSeparation", "false"),
    ];
    for missing in 0..fields.len() {
        let body = fields
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != missing)
            .map(|(_, (name, value))| format!(r#""{name}":{value}"#))
            .collect::<Vec<_>>()
            .join(",");
        let wire = format!(r#"{{"placement":"fit","fitConstraint":{{{body}}}}}"#);
        assert!(
            serde_json::from_str::<DefinitionLayout>(&wire).is_err(),
            "{wire}"
        );
    }

    for wire in [
        r#"{"fitConstraint":{"fitContentBasicUnits":1,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":24,"forcedSeparation":false}}"#,
        r#"{"placement":"stacked","fitConstraint":{"fitContentBasicUnits":1,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":24,"forcedSeparation":false}}"#,
        r#"{"placement":"run-in","fitConstraint":{"fitContentBasicUnits":1,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":24,"forcedSeparation":false}}"#,
        r#"{"fitConstraint":{"fitContentBasicUnits":1,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":0,"forcedSeparation":false}}"#,
        r#"{"fitConstraint":{"fitContentBasicUnits":1,"fieldBasicUnits":2,"originPhaseBasicUnits":24,"cellBasicUnits":24,"forcedSeparation":false}}"#,
        r#"{"fitConstraint":{"fitContentBasicUnits":-1,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":24,"forcedSeparation":false}}"#,
        r#"{"fitConstraint":{"fitContentBasicUnits":18446744073709551616,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":24,"forcedSeparation":false}}"#,
        r#"{"fitConstraint":{"fitContentBasicUnits":1.5,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":24,"forcedSeparation":false}}"#,
        r#"{"fitConstraint":{"fitContentBasicUnits":1,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":24,"forcedSeparation":false,"unknown":0}}"#,
        r#"{"fitConstraint":{"fitContentBasicUnits":1,"fitContentBasicUnits":2,"fieldBasicUnits":2,"originPhaseBasicUnits":0,"cellBasicUnits":24,"forcedSeparation":false}}"#,
    ] {
        assert!(
            serde_json::from_str::<DefinitionLayout>(wire).is_err(),
            "{wire}"
        );
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
