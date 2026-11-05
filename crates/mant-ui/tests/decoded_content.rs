//! Real terminal-safe painting preserves decoded link ownership and copy cells.
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest};
use ratatui::{Terminal, backend::TestBackend};
use unicode_width::UnicodeWidthStr;
#[path = "../../mant-engine/tests/support/decoded_content.rs"]
mod decoded;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "../../mant-engine/tests/support/link_evidence.rs"]
mod link_evidence;
#[path = "support/reader_actions.rs"]
mod reader_actions;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

fn assert_reader(content: &ResolvedContent, case: &decoded::Case) {
    for width in [40, 78] {
        let mut app = App::new(content);
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let mut previous_row = None;
        for label in case.painted_rows {
            let matches = positions(terminal.backend().buffer(), label);
            assert_eq!(matches.len(), 1, "{}/{width}: {label:?}", case.id);
            let (column, row) = matches[0];
            if let Some(previous) = previous_row {
                assert_eq!(row, previous + 1, "{}: exact hard-row boundary", case.id);
            }
            previous_row = Some(row);
            let columns = u16::try_from(label.width()).unwrap();
            for offset in 0..columns {
                click(&mut app, column + offset, row);
                assert_eq!(
                    opened_targets(&mut app),
                    ["https://a.test"],
                    "{}: link cell {offset}",
                    case.id
                );
            }
            for outside in [column - 1, column + columns] {
                click(&mut app, outside, row);
                assert!(
                    opened_targets(&mut app).is_empty(),
                    "{}: inert edge",
                    case.id
                );
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
                        panic!("visual copy")
                    };
                    text
                }),
                [*label],
                "{}: safe visual cells, not raw controls",
                case.id
            );
        }
    }
}
#[test]
fn decoded_entities_keep_painted_cells_navigation_and_copy_after_two_reads() {
    for case in decoded::CASES {
        let source = format!("# Probe\n\n## Payload\n\n{}\n", case.source);
        let original = mant_loader::load_markdown_text(&source, None).unwrap();
        case.assert_first_diagnostic(&original);
        case.assert_content(&original);
        let expected = link_evidence::Meaning::capture(&original);
        assert_eq!(
            expected.target().to_uri().as_deref(),
            Some("https://a.test")
        );
        let mut content = json_roundtrip::json_string_roundtrip(&original);
        for round in 0..=2 {
            case.assert_content(&content);
            expected.assert_preserved(&content, &format!("{}/{round}", case.id));
            assert_reader(&content, case);
            if round < 2 {
                content = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(
                        &mant_codec::encode::render_markdown(&content),
                        None,
                    )
                    .unwrap(),
                );
            }
        }
    }
}
