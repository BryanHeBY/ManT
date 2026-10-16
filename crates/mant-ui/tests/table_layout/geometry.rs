use crate::table_matrix::*;
use mant_ui::DocumentView;
use serde_json::json;

#[test]
fn frame_grid_and_alignment_use_the_final_allocated_region() {
    let mut configurations = 0;
    for stroke in ["single", "double"] {
        for grid in [false, true] {
            for alignment in [None, Some("left"), Some("center"), Some("right")] {
                for parent in [0, 3] {
                    let mut borders = json!({"outer":stroke});
                    if grid {
                        borders["innerRows"] = json!(stroke);
                        borders["innerColumns"] = json!(stroke);
                    }
                    let mut table = json!({"type":"table","borders":borders,
                        "layout":{"indentColumns":parent},
                        "rows":[{"cells":[cell("ALFA"),cell("BETA")]},
                            {"kind":"horizontal-rule","cells":[]},
                            {"cells":[cell("LONGER"),cell("OTHER")]}]});
                    if let Some(alignment) = alignment {
                        table["alignment"] = json!(alignment);
                    }
                    let value = restored(&query(table));
                    let view = DocumentView::new(&value);
                    for width in [24_u16, 40, 80, 24] {
                        configurations += 1;
                        let rendered = view.render(width);
                        let rows = rows(&rendered);
                        let (left, right, top, bottom) = frame(&rows, stroke == "double");
                        let extent = right - left + 1;
                        let spare = usize::from(width) - parent - extent;
                        let origin = parent
                            + match alignment {
                                Some("center") => spare / 2,
                                Some("right") => spare,
                                _ => 0,
                            };
                        assert_eq!(
                            left, origin,
                            "{stroke}, {grid}, {alignment:?}, {parent}, {width}"
                        );
                        assert_eq!(
                            bottom - top,
                            4,
                            "coincident rule/grid must not add a second rule: {rows:?}"
                        );
                        for marker in ["ALFA", "BETA", "LONGER", "OTHER"] {
                            let hit = &rendered.search(marker)[0];
                            assert!(hit.start_column > left && hit.end_column <= right);
                        }
                        assert_markers(&value, width, &["ALFA", "BETA", "LONGER", "OTHER"]);
                        assert_buffer(&rendered, width, &["ALFA", "BETA", "LONGER", "OTHER"]);
                        assert_no_decoration_search(&rendered);
                        let cli = cli(&value, width);
                        let cli_rows = cli.lines().map(str::to_owned).collect::<Vec<_>>();
                        let (cli_left, cli_right, cli_top, cli_bottom) =
                            frame(&cli_rows, stroke == "double");
                        assert!(cli_right < usize::from(width));
                        assert_eq!(cli_bottom - cli_top, 4, "{cli}");
                        assert_eq!(
                            cli_left,
                            parent
                                + match alignment {
                                    Some("center") =>
                                        (usize::from(width) - parent - (cli_right - cli_left + 1))
                                            / 2,
                                    Some("right") =>
                                        usize::from(width) - parent - (cli_right - cli_left + 1),
                                    _ => 0,
                                }
                        );
                    }
                }
            }
        }
    }
    assert_eq!(configurations, 128);
}

#[test]
fn centered_long_prose_wraps_in_parallel_columns_before_stacking() {
    let mut middle = cell("ALPHASTART one two three four five six seven eight nine ten ALPHAEND");
    let mut last = cell("OMEGASTART eleven twelve thirteen fourteen fifteen OMEGAEND");
    middle["preferredWidthColumns"] = json!(34);
    last["preferredWidthColumns"] = json!(34);
    let value = restored(&query(json!({"type":"table","alignment":"center",
        "borders":{"outer":"single","innerColumns":"single"},
        "rows":[{"cells":[cell("MARK"),middle,last]}]})));
    let view = DocumentView::new(&value);
    for width in [40_u16, 80, 120, 24, 80] {
        let rendered = view.render(width);
        assert_buffer(&rendered, width, &["MARK"]);
        assert_no_decoration_search(&rendered);
        for marker in ["MARK", "ALPHASTART", "ALPHAEND", "OMEGASTART", "OMEGAEND"] {
            assert_eq!(
                rendered.search(marker).len(),
                1,
                "{width}: {:?}",
                rendered.text
            );
        }
        if width >= 40 {
            let mark = &rendered.search("MARK")[0];
            let alpha = &rendered.search("ALPHASTART")[0];
            let omega = &rendered.search("OMEGASTART")[0];
            assert_eq!(
                (alpha.row, omega.row),
                (mark.row, mark.row),
                "parallel columns at {width}: {:?}",
                rendered.text
            );
            assert!(mark.end_column < alpha.start_column && alpha.end_column < omega.start_column);
        }
    }
}

#[test]
fn a_spanning_owner_aligns_in_the_entire_covered_region() {
    // These source-equivalent c s / r s examples ran pinned pristine
    // ASCII/UTF-8/HTML/tree/lint before this assertion. tbl_literal() adds
    // the covered widths and spacings; tbl_html.c emits colspan=2.
    for cell_alignment in ["center", "right"] {
        for borders in [
            json!({}),
            json!({"outer":"single","innerColumns":"single"}),
            json!({"outer":"double","innerColumns":"double"}),
        ] {
            let mut owner = cell("XYZ");
            owner["columnSpan"] = json!(2);
            owner["alignment"] = json!(cell_alignment);
            let value = restored(&query(
                json!({"type":"table","borders":borders,"columnCount":2,
                "rows":[{"cells":[owner]},{"cells":[cell("AAAA"),cell("BBBB")]}]}),
            ));
            for width in [24_u16, 40, 80] {
                let rendered = DocumentView::new(&value).render(width);
                let x = &rendered.search("XYZ")[0];
                let a = &rendered.search("AAAA")[0];
                let b = &rendered.search("BBBB")[0];
                let spare = b.end_column - a.start_column - 3;
                assert_eq!(
                    x.start_column,
                    a.start_column
                        + if cell_alignment == "center" {
                            spare / 2
                        } else {
                            spare
                        }
                );
                let text = cli(&value, width);
                let lines = text.lines().collect::<Vec<_>>();
                let xr = lines.iter().find(|row| row.contains("XYZ")).unwrap();
                let lower = lines.iter().find(|row| row.contains("AAAA")).unwrap();
                let a = lower.chars().position(|c| c == 'A').unwrap();
                let end = lower.chars().position(|c| c == 'B').unwrap() + 4;
                let spare = end - a - 3;
                assert_eq!(
                    xr.chars().position(|c| c == 'X'),
                    Some(
                        a + if cell_alignment == "center" {
                            spare / 2
                        } else {
                            spare
                        }
                    ),
                    "{text}"
                );
                assert!(
                    !xr.contains('|'),
                    "no portable divider inside a span: {text}"
                );
                assert_buffer(&rendered, width, &["XYZ", "AAAA", "BBBB"]);
            }
        }
    }
}

#[test]
fn continuing_spans_suppress_crossing_lines_without_consuming_rule_rows() {
    let mut owner = cell("SPAN");
    owner["columnSpan"] = json!(2);
    owner["rowSpan"] = json!(2);
    let value = restored(&query(json!({"type":"table","columnCount":3,
        "borders":{"outer":"single","innerColumns":"single","innerRows":"single"},
        "rows":[{"cells":[owner,cell("TOP")]},
            {"kind":"horizontal-rule","cells":[]},
            {"cells":[{"blocks":[]},{"blocks":[]},cell("BOTTOM")]}]})));
    for width in [40_u16, 80] {
        let rendered = DocumentView::new(&value).render(width);
        let body = rows(&rendered);
        let span = &rendered.search("SPAN")[0];
        let top = &rendered.search("TOP")[0];
        let bottom = &rendered.search("BOTTOM")[0];
        assert_eq!(span.row, top.row);
        assert_eq!(
            bottom.row,
            top.row + 2,
            "rule and grid share one boundary: {body:?}"
        );
        for row in [top.row + 1, bottom.row] {
            let interior = body[row]
                .chars()
                .skip(span.start_column)
                .take(top.start_column - span.start_column - 2)
                .collect::<String>();
            assert!(
                !interior.contains(['─', '│']),
                "continuing owner crossed: {body:?}"
            );
        }
        let cli = cli(&value, width);
        assert_eq!(cli.matches("SPAN").count(), 1);
        let body = cli.lines().collect::<Vec<_>>();
        let top = body.iter().position(|row| row.contains("TOP")).unwrap();
        let bottom = body.iter().position(|row| row.contains("BOTTOM")).unwrap();
        assert_eq!(bottom, top + 2, "{cli}");
        let start = body[top].chars().position(|c| c == 'S').unwrap();
        let right = body[top].chars().position(|c| c == 'T').unwrap();
        for row in [top + 1, bottom] {
            let interior = body[row]
                .chars()
                .skip(start)
                .take(right - start - 2)
                .collect::<String>();
            assert!(
                !interior.contains(['─', '│']),
                "continuing owner crossed: {cli}"
            );
        }
    }
}
