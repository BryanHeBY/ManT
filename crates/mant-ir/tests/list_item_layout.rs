use mant_ir::{ListItem, ListItemLayout};

#[test]
fn list_item_layout_distinguishes_inheritance_from_explicit_spacing() {
    for input in [r#"{"blocks":[]}"#, r#"{"blocks":[],"layout":{}}"#] {
        let item: ListItem = serde_json::from_str(input).unwrap();
        assert_eq!(item.layout, ListItemLayout::default());
        assert!(serde_json::to_value(item).unwrap().get("layout").is_none());
    }
    for rows in [0, 1, 2, 4096] {
        let item: ListItem = serde_json::from_value(serde_json::json!({
            "blocks": [], "layout": {"spacingBeforeLines": rows}
        }))
        .unwrap();
        assert_eq!(item.layout.spacing_before_lines, Some(rows));
        assert_eq!(
            serde_json::to_value(item).unwrap()["layout"]["spacingBeforeLines"],
            rows
        );
    }
    for layout in [serde_json::Value::Null, serde_json::json!({"unknown": 1})] {
        assert!(
            serde_json::from_value::<ListItem>(serde_json::json!({
                "blocks": [], "layout": layout
            }))
            .is_err()
        );
    }
}
