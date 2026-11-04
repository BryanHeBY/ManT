//! Inspect actual painted cells after JSON and two Markdown readbacks.
use mant_ir::ResolvedContent;
use mant_ui::DocumentView;

#[path = "../../mant-engine/tests/support/case_failures.rs"]
mod case_failures;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "../../mant-engine/tests/support/oracle_receipt.rs"]
mod oracle_receipt;
#[path = "../../mant-engine/tests/roff_lowering/word_spacing/cases.rs"]
mod recorded;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

use case_failures::verify_cases;
use json_roundtrip::json_string_roundtrip;
use terminal_cells::{assert_row, paint};

fn assert_cells(content: &ResolvedContent, width: u16, case: &recorded::Case, expected: &[String]) {
    let rendered = DocumentView::new(content).render(width);
    let buffer = paint(&rendered, width);
    let rows: Vec<String> = (0..buffer.area.height)
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
    assert_eq!(
        end - start + 1,
        expected.len(),
        "{}/{width}: {rows:?}",
        case.id
    );
    for (offset, expected) in expected.iter().enumerate() {
        assert_row(
            &buffer,
            start + offset,
            origin,
            expected,
            &format!("{}/{width}", case.id),
        );
    }
}

#[test]
fn executed_empty_words_do_not_gain_a_leading_cell_after_sm_on() {
    // The fixture's five pristine profiles precede these assertions. Native
    // BACKAFTER from bare \z consumes S, whereas \z\c preserves SECOND;
    // NOSPACE and NONEWLINE must not be replaced with a glyph predicate.
    verify_cases(
        recorded::cases(),
        |case| &case.id,
        |case| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let mut content = json_string_roundtrip(&original);
            for round in 0..=2 {
                if round > 0 && case.cohort == "risk" {
                    break;
                }
                for width in [40, 78] {
                    let expected = if round == 0 {
                        case.rows.clone()
                    } else {
                        case.markdown_rows()
                    };
                    assert_cells(&content, width, case, &expected);
                }
                if round < 2 {
                    let markdown = mant_codec::encode::render_markdown(&content);
                    content = mant_loader::load_markdown_text(&markdown, None).unwrap();
                }
            }
        },
    );
}

#[path = "support/reader_actions.rs"]
mod reader_actions;

#[test]
fn incoming_separator_does_not_enter_link_style_copy_or_activation_ranges() {
    use mant_ui::{App, CopyRequest};
    use ratatui::{Terminal, backend::TestBackend, style::Modifier};
    use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

    for (identifier, label) in [
        ("core-empty-on-link", "SECOND"),
        ("core-bold-toggle-link", "SECOND"),
        ("core-bare-z-on-link", "ECOND"),
        ("flush-on-empty-on-link", "SECOND"),
    ] {
        let case = recorded::cases()
            .into_iter()
            .find(|case| case.id == identifier)
            .unwrap();
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let mut content = json_string_roundtrip(&original);
        for round in 0..=2 {
            for width in [40, 78] {
                let mut app = App::new(&content);
                let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let &(column, row) = positions(terminal.backend().buffer(), label)
                    .iter()
                    .max_by_key(|(column, _)| *column)
                    .unwrap();
                let label_width = u16::try_from(label.len()).unwrap();
                for offset in 0..label_width {
                    // termp_lk_pre pushes UNDER for the label independently
                    // of the preceding empty operand's font (1881-1901).
                    assert!(
                        terminal.backend().buffer()[(column + offset, row)]
                            .modifier
                            .contains(Modifier::ITALIC),
                        "{identifier}/{width}"
                    );
                    click(&mut app, column + offset, row);
                    assert_eq!(opened_targets(&mut app), ["https://example.test"]);
                }
                for boundary in [column - 1, column + label_width] {
                    click(&mut app, boundary, row);
                    assert!(opened_targets(&mut app).is_empty(), "{identifier}/{width}");
                }
                select_span(&mut app, column, row, column + label_width - 1, None);
                assert_eq!(
                    copied_selections(&mut app, |request| {
                        let CopyRequest::Selection { text } = request else {
                            panic!("visual copy")
                        };
                        text
                    }),
                    [label]
                );
            }
            if round < 2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = mant_loader::load_markdown_text(&markdown, None).unwrap();
            }
        }
    }
}
