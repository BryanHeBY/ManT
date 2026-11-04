//! Actual terminal cells retain Sm-on word boundaries and zero-width cursor rows.
use mant_ir::ResolvedContent;
use mant_ui::DocumentView;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
use serde_json::Value;

fn cases() -> Vec<Value> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../mant-engine/tests/roff_lowering/spacing_execution/cases.json"
    ))
    .unwrap();
    fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["cohort"] != "pairwise")
        .cloned()
        .collect()
}

fn json_roundtrip(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = serde_json::to_vec(&mant_protocol::QueryBundle::from(&content)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_slice(&wire).unwrap();
    decoded.into()
}

fn assert_cells(content: &ResolvedContent, width: u16, case: &Value) {
    let rendered = DocumentView::new(content).render(width);
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text).render(area, &mut buffer);
    let rows: Vec<String> = (0..area.height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect();
    let start = rows
        .iter()
        .position(|row| row.trim_matches(' ') == "ORIGIN")
        .unwrap();
    let end = rows
        .iter()
        .position(|row| row.trim_matches(' ') == "TAIL")
        .unwrap();
    let origin = rows[start].find("ORIGIN").unwrap();
    let expected = case["rows"].as_array().unwrap();
    assert_eq!(
        end - start + 1,
        expected.len(),
        "{} width {width}: {rows:?}",
        case["id"]
    );
    for (actual, expected) in rows[start..=end].iter().zip(expected) {
        let expected = expected.as_str().unwrap();
        let prefix = format!("{}{expected}", " ".repeat(origin));
        assert!(
            actual.starts_with(&prefix),
            "{} width {width}: {actual:?} != {prefix:?}",
            case["id"]
        );
        // Only untouched cells after the complete authored row are padding.
        // No leading/internal space or empty native row is normalized away.
        assert!(
            actual[prefix.len()..].chars().all(|ch| ch == ' '),
            "{} width {width}: {actual:?}",
            case["id"]
        );
    }
}

#[test]
fn sm_modes_ns_continuations_and_native_cursor_reach_buffers_40_and_78() {
    // All shared exact sources ran pinned CVS ASCII/UTF-8/HTML/tree/lint first.
    // In particular an NBRZW cell and an empty word have different p->col,
    // and Sm-on clears NOSPACE without consuming \c's NONEWLINE state.
    let cases = cases();
    assert_eq!(cases.len(), 179);
    for case in cases {
        let mut content = json_roundtrip(case["source"].as_str().unwrap());
        for round in 0..=2 {
            for width in [40, 78] {
                assert_cells(&content, width, &case);
            }
            if round < 2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = mant_loader::load_markdown_text(&markdown, None).unwrap();
            }
        }
    }
}

#[path = "support/reader_actions.rs"]
mod reader_actions;

#[test]
fn sm_on_does_not_move_link_label_style_copy_or_activation_onto_its_separator() {
    use mant_ui::{App, CopyRequest};
    use ratatui::{Terminal, backend::TestBackend, style::Modifier};
    use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

    for id in [
        "core-filled-on-on-ns-nbrzw-link",
        "core-filled-off-on-c-visible-link",
        "core-filled-off-toggle-ns-visible-link",
        "control-off-off-filled-visible-link",
        "control-off-off-filled-nbrzw-link",
        "control-off-off-filled-blank-link",
    ] {
        let case = cases().into_iter().find(|case| case["id"] == id).unwrap();
        let mut content = json_roundtrip(case["source"].as_str().unwrap());
        for round in 0..=2 {
            for width in [40, 78] {
                let mut app = App::new(&content);
                let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let labels = positions(terminal.backend().buffer(), "SECOND");
                let &(column, row) = labels
                    .iter()
                    .max_by_key(|(column, _)| *column)
                    .expect("actual visible linked label");
                for offset in 0..6 {
                    assert!(
                        terminal.backend().buffer()[(column + offset, row)]
                            .modifier
                            .contains(Modifier::ITALIC),
                        "{id}/{width}"
                    );
                    click(&mut app, column + offset, row);
                    assert_eq!(
                        opened_targets(&mut app),
                        ["https://example.test"],
                        "{id}/{width}"
                    );
                }
                for boundary in [column - 1, column + 6] {
                    click(&mut app, boundary, row);
                    assert!(
                        opened_targets(&mut app).is_empty(),
                        "{id}/{width}: adjacent cell {boundary}"
                    );
                }
                select_span(&mut app, column, row, column + 5, None);
                assert_eq!(
                    copied_selections(&mut app, |request| {
                        let CopyRequest::Selection { text } = request else {
                            panic!("visual copy")
                        };
                        text
                    }),
                    ["SECOND"]
                );
            }
            if round < 2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = mant_loader::load_markdown_text(&markdown, None).unwrap();
            }
        }
    }
}
