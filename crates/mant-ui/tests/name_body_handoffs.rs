//! The ordinary It/Nm handoff reaches actual reader cells and their owners.
use mant_ir::ResolvedContent;
use mant_ui::DocumentView;
use ratatui::style::Modifier;

#[path = "../../mant-engine/tests/support/case_failures.rs"]
mod case_failures;
#[path = "../../mant-engine/tests/support/name_body_handoffs.rs"]
mod fixture;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

use case_failures::verify_cases;
use json_roundtrip::json_string_roundtrip;
use terminal_cells::{assert_row, paint};

#[path = "support/reader_actions.rs"]
mod reader_actions;

fn core_rows(case: &fixture::Case, round: usize) -> Vec<String> {
    if round == 0 {
        case.oracle
            .rows
            .iter()
            .map(|row| row.replace('\u{a0}', " "))
            .collect()
    } else if case.id == "core-inset" {
        vec!["• key probe".into(), "  int".into()]
    } else {
        vec!["• key probe int".into()]
    }
}

fn core_origin(case: &fixture::Case, width: usize, round: usize) -> usize {
    // The source declares width20n + It padding2n; Nm adds width(probe)+1=6.
    // Existing readable_origins reserves sixteen content cells at width40:
    // (first3, continuation3+22+6=31) shifts by31-(40-16)=7, hence first0.
    // Width78 keeps first3. This viewport policy never changes source gold,
    // internal word cells, native fonts or the logical owner coordinates.
    if round == 0 && matches!(case.id.as_str(), "core-tag" | "core-hang") {
        let content = (width / 2).min(16);
        let reduction = 31usize.saturating_sub(width.saturating_sub(content));
        3usize.saturating_sub(reduction)
    } else {
        3
    }
}

fn assert_core(content: &ResolvedContent, case: &fixture::Case, round: usize) {
    for width in [40, 78] {
        let rendered = DocumentView::new(content).render(width);
        let buffer = paint(&rendered, width);
        let start = rendered.anchor_row("synopsis").unwrap() + 1;
        let expected = core_rows(case, round);
        let origin = core_origin(case, usize::from(width), round);
        assert_eq!(
            rendered.anchor_row("next"),
            Some(start + expected.len() + 1),
            "{}/{width}/round{round}: exact physical rows",
            case.id
        );
        for (offset, row) in expected.iter().enumerate() {
            assert_row(
                &buffer,
                start + offset,
                origin,
                row,
                &format!("{}/{width}/round{round}", case.id),
            );
            if round == 0 {
                for (column, font) in case.oracle.styles[offset].iter().enumerate() {
                    let cell = &buffer[(
                        (column + origin).try_into().unwrap(),
                        (start + offset).try_into().unwrap(),
                    )];
                    for (flag, modifier) in [(1, Modifier::BOLD), (2, Modifier::ITALIC)] {
                        assert_eq!(
                            cell.modifier.contains(modifier),
                            font & flag != 0,
                            "{}/{width}: exact native font {offset}/{column}",
                            case.id
                        );
                    }
                }
            }
        }
        assert_eq!(
            rendered.search("probe int").len(),
            usize::from(case.id != "core-inset")
        );
        assert_eq!(rendered.search("probeint"), []);
        assert_eq!(rendered.search("keyprobe"), []);
    }
}

#[test]
fn ordinary_item_names_keep_words_hard_rows_fonts_and_two_real_readbacks() {
    // The typed fixture binds five pristine profiles before these assertions.
    // Native tag/hang HEAD prints stay on the outer row; inset's unflushed
    // key+fixed word overflows the Nm HEAD field and closes that real row.
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| case.cohort == "core"),
        |case| &case.id,
        |case| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let mut content = json_string_roundtrip(&original);
            for round in 0..=2 {
                assert_core(&content, case, round);
                if round < 2 {
                    let markdown = mant_codec::encode::render_markdown(&content);
                    content = mant_loader::load_markdown_text(&markdown, None).unwrap();
                }
            }
        },
    );
}

#[test]
fn the_shared_name_row_keeps_its_original_link_copy_and_search_owner() {
    use mant_ui::{App, CopyRequest};
    use ratatui::{Terminal, backend::TestBackend};
    use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

    let case = fixture::cases()
        .into_iter()
        .find(|case| case.id == "body-tag-external")
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
        select_span(&mut app, column, row, column + 4, None);
        assert_eq!(
            copied_selections(&mut app, |request| {
                let CopyRequest::Selection { text } = request else {
                    panic!("body selection")
                };
                text
            }),
            ["LABEL"]
        );
    }
}
