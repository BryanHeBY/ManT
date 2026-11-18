//! Check current column coordinates and blank rows in the real widget Buffer.

use super::*;

const HEADER: &str =
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
const FINISH: &str = ".El\n.Sh NEXT\n.No SKIP\n";
const TAGS: [&str; 2] = ["", ".Tg Mixed.Target\n"];

fn load(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into()
}

fn paint(view: &DocumentView, width: u16) -> (RenderedDocument, Vec<String>) {
    let rendered = view.render(width);
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    let rows = (0..area.height)
        .map(|row| {
            (0..width)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
        })
        .collect();
    (rendered, rows)
}

#[test]
fn filled_display_keeps_the_middle_col_and_right_col_on_one_painted_row() {
    // The engine's fixed CVS matrix covers these exact roff sources.
    for tag in TAGS {
        for width in [24, 78, 24] {
            let mut origin = None;
            for (parent, shift) in [("", 0), (" -offset 3n", 3)] {
                let source = format!(
                    "{HEADER}.Bl -column a b c{parent}\n.It F Ta \\&\n{tag}.Bd -filled\nMID\n.Ed\n.No \\& Ta Z\n{FINISH}"
                );
                let query = load(&source);
                let view = DocumentView::new(&query);
                let (rendered, rows) = paint(&view, width);
                let hits = ["F", "MID", "Z"].map(|word| {
                    rendered
                        .search(word)
                        .into_iter()
                        .find(|hit| {
                            rows[hit.row]
                                .split_ascii_whitespace()
                                .any(|token| token == word)
                        })
                        .unwrap()
                });
                let table_row = hits[0].row;
                let origin = *origin.get_or_insert(hits[0].start_column);
                for ((word, delta, expected_col), hit) in
                    [("F", 0, shift), ("MID", 1, shift + 5), ("Z", 1, shift + 10)]
                        .into_iter()
                        .zip(&hits)
                {
                    let expected_col = origin + expected_col;
                    assert_eq!(hit.row, table_row + delta, "{source}: width {width}");
                    assert_eq!(hit.start_column, expected_col, "{source}: width {width}");
                    assert_eq!(
                        &rows[hit.row][expected_col..expected_col + word.len()],
                        word
                    );
                }
                let anchor = rendered.anchor_row("Mixed.Target");
                assert_eq!(anchor, (!tag.is_empty()).then_some(table_row + 1));
            }
        }
    }
}

#[test]
fn last_display_block_paints_its_pre_and_post_gaps_independently() {
    // Verify the two independent line intervals in the final widget, not
    // just the number of rows or the continued presence of MID.
    for (compact, before_mid) in [("", 2), (" -compact", 1)] {
        for tag in TAGS {
            let source = format!(
                "{HEADER}.Bl -column a b c\n.It F Ta M Ta \\&\n{tag}.Bd -literal{compact}\nMID\n.Ed\n.It K Ta L Ta M\n{FINISH}"
            );
            let query = load(&source);
            let view = DocumentView::new(&query);
            for width in [24, 78] {
                let (rendered, rows) = paint(&view, width);
                let f = rows
                    .iter()
                    .position(|row| row.trim_start().starts_with("F "))
                    .unwrap();
                let DisplayHit { row: mid, column } = exact_hit(&rendered, &rows, "MID");
                let next = rows
                    .iter()
                    .position(|row| row.trim_start().starts_with("K "))
                    .unwrap();
                assert_eq!(mid - f, before_mid, "{source}: width {width}");
                assert_eq!(next - mid, 2, "{source}: width {width}");
                assert_eq!(
                    column,
                    rows[f].find('F').unwrap() + 10,
                    "{source}: width {width}"
                );
                assert!(rows[f + 1..mid].iter().all(|row| row.trim().is_empty()));
                assert_eq!(rows[mid + 1].trim(), "");
                assert_eq!(
                    rendered.anchor_row("Mixed.Target"),
                    (!tag.is_empty()).then_some(mid)
                );
            }
        }
    }
}

#[test]
fn first_display_paints_layout_rows_with_no_previous_cell() {
    // The engine matrix checks these exact inputs against pristine CVS.
    for (compact, before, gaps) in [
        (" -compact", "", 0),
        ("", "", 1),
        (" -compact", ".Tg X.X\n.sp 1\n", 1),
        (" -compact", ".sp 2\n", 2),
    ] {
        let source = format!(
            "{HEADER}.Bl -column a b c\n.It \\&\n{before}.Bd -literal{compact}\nMID\n.Ed\n.No \\& Ta MIDEND Ta LAST\n.It K Ta L Ta N\n{FINISH}"
        );
        let query = load(&source);
        let view = DocumentView::new(&query);
        for width in [24, 78] {
            let (rendered, rows) = paint(&view, width);
            let heading = exact_hit(&rendered, &rows, "DESCRIPTION").row;
            let mid = rows
                .iter()
                .position(|row| row.split_ascii_whitespace().next() == Some("MID"))
                .unwrap();
            assert_eq!(mid - heading - 1, gaps, "{source}: width {width}");
            assert!(
                rows[heading + 1..mid]
                    .iter()
                    .all(|row| row.trim().is_empty())
            );
            assert_eq!(
                rendered.anchor_row("X.X"),
                before.contains(".Tg").then_some(mid)
            );
        }
    }
}

struct DisplayHit {
    row: usize,
    column: usize,
}

fn exact_hit(rendered: &RenderedDocument, rows: &[String], text: &str) -> DisplayHit {
    let matches = rendered.search(text);
    let [hit] = matches.as_slice() else {
        panic!("expected one result for {text:?}");
    };
    assert_eq!(&rows[hit.row][hit.start_column..hit.end_column], text);
    DisplayHit {
        row: hit.row,
        column: hit.start_column,
    }
}
