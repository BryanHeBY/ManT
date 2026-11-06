//! Public source admission reaches the painted/copy/activation surface intact.
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest};
use ratatui::{Terminal, backend::TestBackend};
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/source_admission.rs"]
mod admission;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "../../mant-engine/tests/support/link_evidence.rs"]
mod link_evidence;
#[path = "support/reader_actions.rs"]
mod reader_actions;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

fn assert_reader(content: &ResolvedContent, case: &admission::Case) {
    for width in [40, 78] {
        let mut app = App::new(content);
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let mut previous = None;
        for label in case.rows {
            let positions = positions(terminal.backend().buffer(), label);
            assert_eq!(positions.len(), 1, "{}/{width}: {label:?}", case.id);
            let (column, row) = positions[0];
            if let Some(previous) = previous {
                assert_eq!(row, previous + 1, "{}: code physical rows", case.id);
            }
            previous = Some(row);
            let columns = u16::try_from(label.width()).unwrap();
            for offset in 0..columns {
                click(&mut app, column + offset, row);
                let targets: Vec<_> = case.active_target.into_iter().map(str::to_owned).collect();
                assert_eq!(opened_targets(&mut app), targets, "{}: cell", case.id);
            }
            for outside in [column.saturating_sub(1), column + columns] {
                click(&mut app, outside, row);
                assert!(opened_targets(&mut app).is_empty(), "{}: edge", case.id);
            }
            select_span(
                &mut app,
                column,
                row,
                column + columns - 1,
                Some(column + columns),
            );
            assert_eq!(
                copied_selections(&mut app, |request| {
                    let CopyRequest::Selection { text } = request else {
                        panic!("visual selection");
                    };
                    text
                }),
                [*label],
                "{}: exact copy",
                case.id
            );
        }
    }
}

#[test]
fn source_admission_keeps_painted_cells_copy_and_targets_across_two_readers() {
    for case in admission::CASES {
        let original = case.load();
        let expected = link_evidence::Meaning::capture(&case.export_witness(&original));
        if let Some(target) = case.active_target {
            assert_eq!(expected.target().to_uri().as_deref(), Some(target));
        }
        let mut current = json_roundtrip::json_string_roundtrip(&original);
        for round in 0..=2 {
            case.assert_content(&current, round == 0);
            if round > 0 {
                expected.assert_preserved(&current, &format!("{}/{round}", case.id));
            }
            assert_reader(&current, case);
            if round < 2 {
                current = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(
                        &mant_codec::encode::render_markdown(&current),
                        None,
                    )
                    .unwrap(),
                );
            }
        }
    }
}
