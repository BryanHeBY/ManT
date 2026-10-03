//! Actual viewport, copy and landing for both parameterized HP/IP owners.
use super::*;

#[test]
fn parameter_declarations_preserve_cells_and_both_authoritative_landing_rows() {
    // Package-local mirror of the exact oracle-first engine sources; the
    // fixture contract gate verifies bytes. Native pre_HP/pre_IP layout is
    // retained when semantic ownership is added or detached.
    let fixture: Value = serde_json::from_str(include_str!("parameter_cases.json")).unwrap();
    let mut cases = fixture["cases"].as_array().unwrap().clone();
    cases.push(fixture["multipleOwners"].clone());
    for case in cases
        .iter()
        .filter(|case| case["source"].as_str().unwrap().contains("\n.HP\n"))
    {
        let original =
            mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let content: ResolvedContent = decoded.into();
        assert_eq!(content, original);
        let detached = baseline(&content);
        let view = DocumentView::new(&content);
        let prior = DocumentView::new(&detached);
        let names = if case["ownerNames"].is_array() {
            case["ownerNames"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|names| names.as_array().unwrap())
                .collect::<Vec<_>>()
        } else {
            case["names"].as_array().unwrap().iter().collect()
        };
        let first = view.render(20).text;
        for width in [20, 40, 78, 120, 20] {
            let rendered = view.render(width);
            assert_native_cells(
                &cells(&rendered, width),
                &cells(&prior.render(width), width),
                &case["id"],
                width,
            );
            for name in &names {
                let name = name.as_str().unwrap();
                let found = rendered.search(name);
                assert_ne!(found.len(), 0, "{}: {name} width {width}", case["id"]);
                let hit = &found[0];
                let (row, end) = hit
                    .additional_fragments
                    .last()
                    .map_or((hit.row, hit.end_column), |last| {
                        (last.row, last.end_column)
                    });
                let selection = RenderedSelection {
                    anchor: TextPosition {
                        row: hit.row,
                        column: hit.start_column,
                    },
                    focus: TextPosition {
                        row,
                        column: end - 1,
                    },
                };
                let copied = rendered.selected_text(selection);
                assert_eq!(copied, prior.render(width).selected_text(selection));
                // Selection includes actual hanging origins on soft-wrapped
                // physical rows. Verify those cells exactly above; remove
                // only each row's origin to recover the bound name.
                assert_eq!(
                    copied.split('\n').map(str::trim_start).collect::<String>(),
                    name
                );
            }
            if width == 120 && !case["ownerNames"].is_array() && !names.is_empty() {
                let mut native = case.clone();
                native["ownerNames"] = serde_json::json!([case["names"]]);
                check_native_rows(&native, &content, &rendered);
            }
            if case["ownerNames"].is_array() {
                let mut rows = Vec::new();
                for block in &content.document.as_ref().unwrap().sections[0].blocks {
                    if let Block::List { items, .. } = block {
                        for owner in items.iter().filter(|owner| owner.entry.is_some()) {
                            let row = rendered
                                .anchor_row(owner.entry.as_ref().unwrap().id.as_str())
                                .unwrap();
                            rows.push(row);
                            let name = owner.entry.as_ref().unwrap().names[0].as_str();
                            assert_eq!(rendered.search(name)[0].row, row);
                        }
                    }
                }
                assert_eq!(rows.len(), 2);
                assert!(rows[0] < rows[1]);
            }
        }
        assert_eq!(view.render(20).text, first);
    }
}
