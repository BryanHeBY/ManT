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
    let cases = [
        include_str!("field_replay_oracle.json"),
        include_str!("word_projection_oracle.json"),
    ]
    .into_iter()
    .flat_map(|data| serde_json::from_str::<Vec<Case>>(data).unwrap());
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

#[test]
fn tbl_breakpoint_measurement_preserves_the_real_link_after_a_retreat() {
    // Fixed ASCII and UTF-8 print LEFTa<BS>b<BS>YTAIL on one row at
    // each witness width. Only YTAIL survives inside a tty link.
    let header =
        ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- example probe\n.SH DESCRIPTION\n";
    let old = LinkTarget::External(ExternalUri::parse("https://old.test/b").unwrap());
    let new = LinkTarget::External(ExternalUri::parse("https://new.test/y").unwrap());
    let cell = r"LEFT\X'tty: link https://old.test/b'\z\o'ab'\X'tty: link'\:\X'tty: link https://new.test/y'YTAIL\X'tty: link'";
    for columns in [0, 9, 10] {
        let witness = if columns == 0 {
            String::new()
        } else {
            format!("{}\n", "W".repeat(columns))
        };
        let source = format!("{header}.TS\nl.\n{cell}\n{witness}.TE\nENDTEXT\n");
        let view = DocumentView::new(&restored(&source));
        for width in [24_u16, 78] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let hits = rendered.search("LEFTYTAIL");
            let [hit] = hits.as_slice() else {
                panic!("expected the complete table word at {width}: {source}");
            };
            assert_eq!(
                painted(&buffer, hit.row, hit.start_column, hit.end_column),
                "LEFTYTAIL",
                "{source}"
            );
            let copy = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column,
                },
                focus: TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1,
                },
            });
            assert_eq!(copy, "LEFTYTAIL", "{source}");
            for col in hit.start_column..hit.end_column {
                assert_eq!(
                    rendered.link_target_at(hit.row, col),
                    (col >= hit.start_column + "LEFT".len()).then_some(&new),
                    "{source}"
                );
            }
            assert!(
                (0..rendered.row_count).all(|row| {
                    (0..usize::from(width))
                        .all(|col| rendered.link_target_at(row, col) != Some(&old))
                }),
                "{source}"
            );
            assert_eq!(rendered.search("ENDTEXT").len(), 1, "{source}");
        }
    }
}
