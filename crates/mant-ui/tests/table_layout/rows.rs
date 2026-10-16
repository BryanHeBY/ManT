use crate::table_matrix::*;
use mant_ui::DocumentView;
use serde_json::{Value, json};

#[test]
fn framed_fallback_keeps_empty_cell_receipts_distinct() {
    let literal = |children: Value| json!({"type":"preformatted","children":children});
    let cases = [
        ("absent", json!([]), 1),
        (
            "empty-paragraph",
            json!([{"type":"paragraph","children":[]}]),
            1,
        ),
        ("zero-literal", json!([literal(json!([]))]), 1),
        (
            "authored-empty-literal",
            json!([literal(json!([{"type":"text","value":""}]))]),
            2,
        ),
        (
            "author-space",
            json!([literal(json!([{"type":"text","value":" "}]))]),
            2,
        ),
        (
            "vertical-one",
            json!([{"type":"vertical-space","lines":1}]),
            2,
        ),
        (
            "vertical-two",
            json!([{"type":"vertical-space","lines":2}]),
            3,
        ),
    ];
    for (name, blocks, distance) in cases {
        for stroke in [None, Some("single"), Some("double")] {
            for fallback in ["columns", "gap", "viewport"] {
                let borders = stroke.map_or_else(|| json!({}), |stroke| json!({"outer":stroke}));
                let mut table = json!({"type":"table","borders":borders,
                    "rows":[{"cells":[cell("FIRST"),{"blocks":blocks.clone()},cell("SECOND")]}]});
                if fallback == "columns" {
                    table["columnCount"] = json!(257);
                }
                if fallback == "gap" {
                    table["columnPreferences"] = json!({"gapColumns":65535});
                }
                let width = if fallback == "viewport" { 12 } else { 40 };
                let value = restored(&query(table));
                let rendered = DocumentView::new(&value).render(width);
                let first = &rendered.search("FIRST")[0];
                let second = &rendered.search("SECOND")[0];
                assert_eq!(
                    second.row - first.row,
                    distance,
                    "{name}, {stroke:?}, {fallback}: {:?}",
                    rendered.text
                );
                let text = cli(&value, width);
                let lines = text.lines().collect::<Vec<_>>();
                let first = lines
                    .iter()
                    .position(|line| line.contains("FIRST"))
                    .unwrap();
                let second = lines
                    .iter()
                    .position(|line| line.contains("SECOND"))
                    .unwrap();
                assert_eq!(
                    second - first,
                    distance,
                    "{name}, {stroke:?}, {fallback}: {text:?}"
                );
                assert_buffer(&rendered, width, &["FIRST", "SECOND"]);
                assert_no_decoration_search(&rendered);
            }
        }
    }
}

#[test]
fn global_frame_does_not_make_a_navigation_carrier_an_extra_data_row() {
    for stroke in ["single", "double"] {
        let data = json!({"cells":[cell("FIRST"),cell("SECOND")]});
        let table = |rows: Value| {
            query(
                json!({"type":"table","borders":{"outer":stroke,"innerColumns":stroke},"columnCount":2,"rows":rows}),
            )
        };
        let original = table(json!([data.clone()]));
        let with_navigation = restored(&table(json!([
            {"cells":[{"blocks":[{"type":"paragraph","children":[{"type":"anchor","id":"nav-destination"}]}]}]},data
        ])));
        for width in [24, 80, 24] {
            let plain = DocumentView::new(&original).render(width);
            let marked = DocumentView::new(&with_navigation).render(width);
            assert_eq!(rows(&plain), rows(&marked));
            assert_eq!(
                marked.anchor_row("nav-destination"),
                Some(marked.search("FIRST")[0].row)
            );
            assert_eq!(cli(&original, width), cli(&with_navigation, width));
        }
    }
}

#[test]
fn logical_span_limits_never_amplify_placeholder_cells() {
    for columns in [256_u32, 257, 65535, 65536, u32::MAX] {
        let mut owner = cell("SPANOWNER");
        owner["columnSpan"] = json!(columns);
        let value = restored(&query(json!({"type":"table","columnCount":columns,
            "borders":{"outer":"single","innerColumns":"single"},
            "rows":[{"cells":[owner]}]})));
        let rendered = DocumentView::new(&value).render(40);
        assert_eq!(rendered.search("SPANOWNER").len(), 1);
        assert!(
            rendered.row_count < 16,
            "span count manufactured rows: {columns}"
        );
        let text = cli(&value, 40);
        assert_eq!(text.matches("SPANOWNER").count(), 1);
        assert!(
            text.len() < 4096,
            "span count manufactured output: {columns}"
        );
    }
}
