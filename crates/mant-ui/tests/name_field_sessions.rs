//! Real cell geometry, fonts and actions after the complete local Nm session.
use mant_ir::ResolvedContent;
use mant_ui::DocumentView;
use ratatui::style::Modifier;

#[path = "../../mant-engine/tests/support/case_failures.rs"]
mod case_failures;
#[path = "../../mant-engine/tests/support/name_field_sessions.rs"]
mod fixture;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

use case_failures::verify_cases;
use json_roundtrip::json_string_roundtrip;
use terminal_cells::{assert_row, paint};

fn assert_cells(content: &ResolvedContent, width: u16, case: &fixture::Case, round: usize) {
    let rendered = DocumentView::new(content).render(width);
    let buffer = paint(&rendered, width);
    let start = rendered.anchor_row("synopsis").unwrap() + 1;
    let expected = case.expected();
    let mut presented_rows = expected.rows.clone();
    if case.id == "risk-enclosure" && round == 0 {
        assert_eq!(expected.rows, ["[probe", "      int", "]"]);
        // The unframed cell's natural width is 6 offset + 3 glyphs = 9.
        // Both 40/78 viewports admit that whole cell after the shared 3-cell
        // margin; no table rail adds padding. The documented readability
        // policy reserves half of this narrow cell (4), so presentation
        // reduces the six-column logical origin to 9 - 4 = 5. Markdown's
        // fence later carries six authored SP, not a compressible row hint.
        let natural_width = 6_usize + 3;
        assert!(usize::from(width) >= natural_width + 3);
        let content_reserve = (natural_width / 2).min(16);
        let origin = 6.min(natural_width - content_reserve);
        presented_rows[1] = format!("{}int", " ".repeat(origin));
    }
    assert_eq!(
        rendered.anchor_row("next"),
        Some(start + expected.rows.len() + 1),
        "{}/{width}/round{round}: exact hard rows",
        case.id
    );
    for (offset, row) in presented_rows.iter().enumerate() {
        assert_row(
            &buffer,
            start + offset,
            3,
            row,
            &format!("{}/{width}/round{round}", case.id),
        );
        if round == 0 && case.cohort == "core" {
            for (column, font) in expected.styles[offset].iter().enumerate() {
                let cell = &buffer[(
                    (3 + column).try_into().unwrap(),
                    (start + offset).try_into().unwrap(),
                )];
                for (flag, modifier) in [(1, Modifier::BOLD), (2, Modifier::ITALIC)] {
                    assert_eq!(
                        cell.modifier.contains(modifier),
                        font & flag != 0,
                        "{}/{width}: native font {offset}/{column}",
                        case.id
                    );
                }
            }
        }
    }
}

#[test]
fn clean_independent_nm_sessions_keep_words_fonts_and_hard_rows_in_actual_cells() {
    // Every source has independent pristine receipts before this assertion.
    // The whole tbl UNSUPP is not called a native-supported table. Its chosen
    // bounded enhancement follows the clean independent section session.
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| case.independent.is_some()),
        |case| &case.id,
        |case| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let mut content = json_string_roundtrip(&original);
            for round in 0..=2 {
                for width in [40, 78] {
                    assert_cells(&content, width, case, round);
                }
                if round < 2 {
                    let markdown = mant_codec::encode::render_markdown(&content);
                    content = mant_loader::load_markdown_text(&markdown, None).unwrap();
                }
            }
        },
    );
}

#[test]
fn adjacent_and_following_cells_keep_searchable_whole_words_at_both_viewports() {
    verify_cases(
        fixture::cell_controls(),
        |case| &case.id,
        |case| {
            let source = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let content = json_string_roundtrip(&source);
            let document = content.document.as_ref().unwrap();
            let synopsis = document
                .sections
                .iter()
                .find(|section| section.heading.plain_text() == "SYNOPSIS")
                .unwrap();
            let [mant_ir::Block::Table { rows, .. }] = synopsis.blocks.as_slice() else {
                panic!("table owner")
            };
            assert_eq!(rows.len(), if case.adjacent { 1 } else { 2 });
            for width in [40, 78] {
                let rendered = DocumentView::new(&content).render(width);
                for expected in &case.independent {
                    assert_eq!(expected.rows.len(), 1);
                    assert_eq!(
                        rendered.search(&expected.rows[0]).len(),
                        1,
                        "{}/{width}",
                        case.id
                    );
                }
            }
        },
    );
}

#[path = "support/reader_actions.rs"]
mod reader_actions;

#[test]
fn nm_body_links_keep_original_cell_search_copy_and_click_owners() {
    use mant_ui::{App, CopyRequest};
    use ratatui::{Terminal, backend::TestBackend};
    use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

    let case = fixture::cases()
        .into_iter()
        .find(|case| case.id == "risk-body-external")
        .unwrap();
    let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&original);
    for width in [40, 78] {
        let rendered = DocumentView::new(&content).render(width);
        assert_eq!(rendered.search("probe LABEL").len(), 1);
        assert_eq!(rendered.search("probeLABEL"), []);
        let mut app = App::new(&content);
        let mut terminal = Terminal::new(TestBackend::new(width, 48)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let found = positions(terminal.backend().buffer(), "LABEL");
        assert_eq!(found.len(), 1);
        let (column, row) = found[0];
        for offset in 0..5 {
            click(&mut app, column + offset, row);
            assert_eq!(opened_targets(&mut app), ["https://example.test"]);
        }
        for boundary in [column - 1, column + 5] {
            click(&mut app, boundary, row);
            assert_eq!(opened_targets(&mut app), [] as [String; 0]);
        }
        select_span(&mut app, column, row, column + 4, None);
        assert_eq!(
            copied_selections(&mut app, |request| {
                let CopyRequest::Selection { text } = request else {
                    panic!("visual body selection")
                };
                text
            }),
            ["LABEL"]
        );
    }
}
