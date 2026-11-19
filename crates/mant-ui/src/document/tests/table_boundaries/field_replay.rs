use super::*;

#[derive(serde::Deserialize)]
struct Case {
    source: String,
    probe: String,
    want: usize,
    link: bool,
}

#[test]
fn visible_slots_agree_across_paint_copy_search_and_link_hits() {
    // ASCII and UTF-8 from the fixed CVS agree for these exact sources.
    // Include empty and superseded labels; their target alone cannot prove
    // that any displayed position belongs to the link.
    let cases: Vec<Case> = serde_json::from_str(include_str!("field_replay_oracle.json")).unwrap();
    let target = LinkTarget::External(ExternalUri::parse("https://example.test/r").unwrap());
    for case in cases {
        let view = DocumentView::new(&restored(&case.source));
        for width in [24, 78] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let shown: usize = (0..rendered.row_count)
                .map(|row| {
                    painted(&buffer, row, 0, width.into())
                        .matches(&case.probe)
                        .count()
                })
                .sum();
            let copy = rendered.selected_text(RenderedSelection {
                anchor: TextPosition { row: 0, column: 0 },
                focus: TextPosition {
                    row: rendered.row_count - 1,
                    column: usize::from(width) - 1,
                },
            });
            let hits = rendered.search(&case.probe);
            assert_eq!(
                [shown, copy.matches(&case.probe).count(), hits.len()],
                [case.want; 3],
                "{} at {width}",
                case.source
            );
            for hit in hits {
                for col in hit.start_column..hit.end_column {
                    assert_eq!(
                        rendered.link_target_at(hit.row, col),
                        case.link.then_some(&target),
                        "{} at {width}",
                        case.source
                    );
                }
            }
        }
    }
}
