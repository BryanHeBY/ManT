//! Grid boundaries belong to original data rows, independently of stacking.
use crate::table_matrix::*;
use mant_ui::DocumentView;
use serde_json::{Value, json};

fn separators(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| {
            line.contains(['├', '╠', '╟', '╞'])
                || (!line.is_empty()
                    && line
                        .chars()
                        .all(|ch| matches!(ch, '─' | '═' | '-' | '=' | '┼' | '╬' | '╪' | '╫')))
        })
        .collect()
}

fn table(stroke: &str, outer: bool, middle: Option<Value>, span: usize) -> Value {
    let mut first = vec![cell("AAAA"), cell("BBBB")];
    let mut second = vec![cell("CCCC"), cell("DDDD")];
    for column in 0..span {
        first[column]["rowSpan"] = json!(2);
        second[column] = json!({"blocks":[]});
    }
    let mut rows = vec![json!({"cells":first})];
    if let Some(middle) = middle {
        rows.push(middle);
    }
    rows.push(json!({"cells":second}));
    let mut borders = json!({"innerRows":stroke,"innerColumns":stroke});
    if outer {
        borders["outer"] = json!(stroke);
    }
    json!({"type":"table","alignment":"left","borders":borders,"rows":rows})
}

#[test]
fn source_grid_boundaries_survive_all_viewport_fallbacks_once() {
    let mut configurations = 0;
    for stroke in ["single", "double"] {
        for outer in [false, true] {
            for width in [2_u16, 7, 8, 9, 14, 15, 40] {
                for (middle, span, count) in [
                    (None, 0, 1),
                    (Some(json!({"kind":"horizontal-rule","cells":[]})), 0, 1),
                    (
                        Some(json!({"kind":"double-horizontal-rule","cells":[]})),
                        0,
                        1,
                    ),
                    (
                        Some(
                            json!({"cells":[{"blocks":[{"type":"paragraph","children":[{"type":"anchor","id":"navigation"}]}]}]}),
                        ),
                        0,
                        1,
                    ),
                    (Some(json!({"cells":[]})), 0, 2),
                    (None, 1, 1),
                    (None, 2, 0),
                    (Some(json!({"cells":[]})), 2, 2),
                ] {
                    let value = restored(&query(table(stroke, outer, middle, span)));
                    let rendered = DocumentView::new(&value).render(width);
                    let ui = rows(&rendered);
                    let text = cli(&value, width);
                    let cli = text.lines().map(str::to_owned).collect::<Vec<_>>();
                    assert_eq!(
                        separators(&cli).len(),
                        count,
                        "CLI {stroke}/{outer}/{width}/span={span}: {cli:?}"
                    );
                    assert_eq!(
                        separators(&ui).len(),
                        count,
                        "TUI {stroke}/{outer}/{width}/span={span}: {ui:?}"
                    );
                    assert_no_decoration_search(&rendered);
                    for marker in ["AAAA", "BBBB"] {
                        assert_eq!(rendered.search(marker).len(), 1);
                    }
                    if span == 0 {
                        for marker in ["CCCC", "DDDD"] {
                            assert_eq!(rendered.search(marker).len(), 1);
                        }
                    }
                    configurations += 1;
                }
            }
        }
    }
    assert_eq!(configurations, 224);
}

#[test]
fn empty_logical_column_ranges_do_not_imply_a_continuing_span() {
    let value = query(
        json!({"type":"table","borders":{"outer":"single","innerRows":"single"},
        "rows":[{"cells":[]},{"cells":[]}]}),
    );
    for width in [2_u16, 9, 40] {
        let text = cli(&value, width);
        let cli = text.lines().map(str::to_owned).collect::<Vec<_>>();
        let ui = rows(&DocumentView::new(&value).render(width));
        assert_eq!(separators(&cli).len(), 1, "CLI {width}: {cli:?}");
        assert_eq!(separators(&ui).len(), 1, "TUI {width}: {ui:?}");
    }
}

#[test]
fn stacked_grid_lines_separate_data_rows_instead_of_cells_or_wrapped_rows() {
    let value = query(table("single", true, None, 0));
    let expected = [
        "┌──────┐",
        "│ AAAA │",
        "│ BBBB │",
        "├──────┤",
        "│ CCCC │",
        "│ DDDD │",
        "└──────┘",
    ];
    for lines in [
        cli(&value, 9)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        rows(&DocumentView::new(&value).render(9)),
    ] {
        let (_, _, top, bottom) = frame(&lines, false);
        assert_eq!(&lines[top..=bottom], expected);
    }
    let mut wrapped = cell("long word here");
    wrapped["preferredWidthColumns"] = json!(3);
    let value = query(
        json!({"type":"table","borders":{"outer":"single","innerRows":"single","innerColumns":"single"},
        "rows":[{"cells":[wrapped,cell("BBBB")]},{"cells":[cell("CCCC"),cell("DDDD")]}]}),
    );
    let text = cli(&value, 9);
    let lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(separators(&lines), ["├──────┤"]);
    let divider = lines.iter().position(|line| line.contains('├')).unwrap();
    for word in ["long", "word", "here", "BBBB"] {
        assert!(lines[..divider].iter().any(|line| line.contains(word)));
    }
    assert!(
        lines[divider + 1..]
            .iter()
            .any(|line| line.contains("CCCC"))
    );
}
