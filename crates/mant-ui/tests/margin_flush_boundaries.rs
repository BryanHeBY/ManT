//! A native flush must not add a cell to reader search or clipboard text.

use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend};

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "../../mant-engine/tests/support/margin_flush_cases.rs"]
mod recorded;

#[test]
fn margin_flush_word_seams_reach_painted_cells_search_and_copy() {
    for case in recorded::CASES {
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let mut content = json_roundtrip::json_string_roundtrip(&original);
        for round in 0..=2 {
            for width in [40, 78] {
                let view = DocumentView::new(&content).render(width);
                assert_eq!(view.search(case.row).len(), 1, "{}/{round}", case.id);
                assert!(view.search(case.rejected_row).is_empty(), "{}", case.id);
                let mut app = App::new(&content);
                let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let buffer = terminal.backend().buffer();
                let hits = reader_actions::positions(buffer, case.row);
                assert_eq!(hits.len(), 1, "{}/{width}/{round}", case.id);
                let (column, row) = hits[0];
                let origin = reader_actions::positions(buffer, "ORIGIN");
                let tail = reader_actions::positions(buffer, "TAIL");
                assert_eq!(origin, [(column, row - 1)], "{}", case.id);
                assert_eq!(tail, [(column, row + 1)], "{}", case.id);
                assert_eq!(
                    reader_actions::positions(buffer, case.rejected_row),
                    Vec::<(u16, u16)>::new()
                );
                let length = u16::try_from(case.row.len()).unwrap();
                reader_actions::click(&mut app, column, row);
                assert_eq!(
                    reader_actions::opened_targets(&mut app),
                    Vec::<String>::new()
                );
                reader_actions::select_span(&mut app, column, row, column + length - 1, None);
                assert_eq!(
                    reader_actions::copied_selections(&mut app, |request| {
                        let CopyRequest::Selection { text } = request else {
                            panic!("visual copy")
                        };
                        text
                    }),
                    [case.row],
                    "{}/{width}/{round}",
                    case.id
                );
            }
            if round < 2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
            }
        }
    }
}
