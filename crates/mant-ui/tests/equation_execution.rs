//! Executed equation boundaries reach the actual document and reader cells.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ir::ResolvedContent;
use mant_ui::{App, DocumentView};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};
use serde::Deserialize;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Matrix {
    reference_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    axes: [String; 6],
    source: String,
    body: String,
    damaged_math: bool,
}

fn cases() -> Vec<Case> {
    let matrix: Matrix = serde_json::from_str(include_str!(
        "../../mant-engine/tests/roff_lowering/equation_execution/matrix.json"
    ))
    .unwrap();
    assert_eq!(
        matrix.reference_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    let selected: Vec<_> = matrix
        .cases
        .into_iter()
        .filter(|case| {
            let [_, _, prefix, payload, origin, suffix] = case.axes.each_ref();
            [
                ("hard-break", "one-atom", "new-line", "same-line"),
                ("zero-X", "one-atom", "new-line", "same-line"),
                ("plain", "space-atom", "same-line", "same-line"),
                ("plain", "two-space-atom", "same-line", "same-line"),
                ("zero-bare", "one-atom", "new-line", "same-line"),
                ("zero-X", "empty-atom", "new-line", "same-line"),
                ("zero-X", "space-atom", "same-line", "same-line"),
                ("zero-X", "two-space-atom", "same-line", "new-line"),
            ]
            .contains(&(
                prefix.as_str(),
                payload.as_str(),
                origin.as_str(),
                suffix.as_str(),
            ))
        })
        .collect();
    assert_eq!(selected.len(), 32);
    assert_eq!(selected.iter().filter(|case| case.damaged_math).count(), 2);
    selected
}

fn load(case: &Case) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    json_roundtrip::json_string_roundtrip(&content)
}

#[test]
fn equation_word_rows_and_authored_blanks_paint_after_json_at_40_and_78() {
    // These exact matrix sources first ran all five pinned native profiles.
    // eqn_term.c::eqn_box calls term_word for every atom, including an empty
    // or blank atom. term.c settles the incoming \p and BACKBEFORE states
    // before that word. The recorded body keeps every accepted author blank.
    for case in cases() {
        let content = load(&case);
        for width in [40, 78] {
            let rendered = DocumentView::new(&content).render(width);
            let buffer = terminal_cells::paint(&rendered, width);
            let start = rendered.anchor_row("description").unwrap() + 1;
            let expected: Vec<_> = case.body.split('\n').collect();
            assert_eq!(
                rendered.row_count,
                start + expected.len(),
                "{}/{width}: exact hard row count",
                case.id
            );
            for (offset, row) in expected.iter().enumerate() {
                // Three columns are the top-level section's UI origin. No
                // source-leading, internal or trailing blank is trimmed.
                terminal_cells::assert_row(
                    &buffer,
                    start + offset,
                    3,
                    row,
                    &format!("{}/{width}", case.id),
                );
            }
        }
    }
}

#[test]
fn executed_equation_rows_reach_the_app_document_viewport() {
    let selected = cases()
        .into_iter()
        .filter(|case| {
            [
                "man--filled--hard-break--one-atom--new-line--same-line",
                "mdoc--literal--zero-X--one-atom--new-line--same-line",
                "man--filled--plain--two-space-atom--same-line--same-line",
                "mdoc--filled--zero-bare--one-atom--new-line--same-line",
                "man--filled--zero-X--space-atom--same-line--same-line",
            ]
            .contains(&case.id.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 5);
    for case in selected {
        let content = load(&case);
        for width in [40, 78] {
            let mut app = App::new(&content);
            app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
            // The reader reserves one outer cell on each side and a two-cell
            // scrollbar gutter. Its document therefore receives this width.
            let mut terminal = Terminal::new(TestBackend::new(width + 4, 24)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            let headings = (1..buffer.area.height - 1)
                .filter(|&row| {
                    let label: String = (1..=11)
                        .map(|column| buffer[(column, row)].symbol())
                        .collect();
                    label == "DESCRIPTION"
                })
                .collect::<Vec<_>>();
            assert_eq!(headings.len(), 1, "{}: actual content heading", case.id);
            let start = usize::from(headings[0]) + 1;
            let mut document = Buffer::empty(Rect::new(0, 0, width, buffer.area.height));
            for row in 0..buffer.area.height {
                for column in 0..width {
                    document[(column, row)] = buffer[(column + 1, row)].clone();
                }
            }
            for (offset, row) in case.body.split('\n').enumerate() {
                terminal_cells::assert_row(
                    &document,
                    start + offset,
                    3,
                    row,
                    &format!("{}/{width}: App", case.id),
                );
            }
        }
    }
}
