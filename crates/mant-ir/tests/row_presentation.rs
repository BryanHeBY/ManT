//! Source-neutral row placement never changes an owner's authoritative text.

use mant_ir::{
    Inline, InlineContentRef, InlineLayout, RowLayoutHint, TextAlignment, inline_plain_text,
};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

#[test]
fn first_visual_displacement_preserves_the_structural_hanging_origin() {
    for (first, continued, relative, first_only, expected) in [
        (2, 6, 1, 4, (7, 7)),
        (2, 6, -3, 4, (3, 3)),
        (2, 6, 1, -4, (-1, 7)),
        (-4, 2, 3, 2, (1, 5)),
    ] {
        let hint = RowLayoutHint {
            row: 0,
            indent_columns: relative,
            first_line_indent_columns: first_only,
            alignment: TextAlignment::Center,
        };
        let origins = hint.resolve(first, continued);
        assert_eq!(
            (origins.first_visual_origin, origins.continuation_origin),
            expected
        );
    }
    let hint = RowLayoutHint {
        first_line_indent_columns: 1,
        ..Default::default()
    };
    assert_eq!(hint.resolve(i32::MAX, 0).first_visual_origin, i32::MAX);
}

#[test]
fn alignment_uses_the_viewport_without_creating_cells_for_empty_rows() {
    for (available, occupied, center, right) in [
        (20, 4, 8, 16),
        (19, 4, 7, 15),
        (4, 20, 0, 0),
        (0, 4, 0, 0),
        (20, 0, 0, 0),
    ] {
        assert_eq!(TextAlignment::Left.padding(available, occupied), 0);
        assert_eq!(TextAlignment::Center.padding(available, occupied), center);
        assert_eq!(TextAlignment::Right.padding(available, occupied), right);
    }
}

#[test]
fn presentation_only_hints_survive_actual_json_and_default_fields_are_omitted() {
    for (field, wire) in [
        (
            "alignment",
            r#"{"rowHints":[{"row":0,"indentColumns":0,"alignment":"center"}]}"#,
        ),
        (
            "firstLineIndentColumns",
            r#"{"rowHints":[{"row":0,"indentColumns":0,"firstLineIndentColumns":4}]}"#,
        ),
    ] {
        let layout: InlineLayout = serde_json::from_str(wire).unwrap();
        assert!(!layout.is_empty(), "{field}");
        layout.validate(&[]).unwrap();
        let encoded = serde_json::to_string(&layout).unwrap();
        assert_eq!(
            serde_json::from_str::<InlineLayout>(&encoded).unwrap(),
            layout
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
            serde_json::from_str::<serde_json::Value>(wire).unwrap()
        );
    }
    let legacy: RowLayoutHint = serde_json::from_str(r#"{"row":0,"indentColumns":4}"#).unwrap();
    assert_eq!(legacy.first_line_indent_columns, 0);
    assert_eq!(legacy.alignment, TextAlignment::Left);
    let explicit: InlineLayout = serde_json::from_str(
        r#"{"rowHints":[{"row":0,"indentColumns":0,"firstLineIndentColumns":0,"alignment":"left"}]}"#,
    )
    .unwrap();
    assert!(explicit.is_empty());
    assert_eq!(serde_json::to_string(&explicit).unwrap(), "{}");
}

#[test]
fn scalar_slicing_retains_both_origins_and_alignment_in_the_new_owner() {
    let content = vec![Inline::Emphasis {
        children: vec![text("é名\n"), text("中\nZ")],
    }];
    let hint = RowLayoutHint {
        row: 1,
        indent_columns: -2,
        first_line_indent_columns: 4,
        alignment: TextAlignment::Right,
    };
    let layout = InlineLayout {
        row_hints: vec![hint],
    };
    let view = InlineContentRef {
        content: &content,
        layout: &layout,
    };
    let sliced = view.sliced_layout(3..5).unwrap();
    assert_eq!(sliced.row_layout(0), RowLayoutHint { row: 0, ..hint });
    assert!(sliced.row_layout(1).is_empty());
    assert_eq!(inline_plain_text(&content), "é名\n中\nZ");
}

#[test]
fn empty_and_open_rows_accept_presentation_without_manufacturing_text() {
    for (content, row, expected) in [
        (vec![], 0, ""),
        (vec![Inline::anchor("target")], 0, ""),
        (vec![text("A")], 0, "A"),
        (vec![text("A\n")], 1, "A\n"),
        (vec![text("A\n\n")], 2, "A\n\n"),
    ] {
        let layout = InlineLayout {
            row_hints: vec![RowLayoutHint {
                row,
                indent_columns: -2,
                first_line_indent_columns: 4,
                alignment: TextAlignment::Center,
            }],
        };
        layout.validate(&content).unwrap();
        assert_eq!(inline_plain_text(&content), expected);
        assert_eq!(
            layout.row_layout(usize::try_from(row).unwrap()).alignment,
            TextAlignment::Center
        );
    }
}
