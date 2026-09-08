use mant_ir::LayoutHint;

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
