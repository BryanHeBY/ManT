//! Table CR safety is observable in real cells, search, links and visual copy.

use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend};
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "../../mant-engine/tests/support/table_content_roles.rs"]
mod roles;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

fn assert_reader(content: &ResolvedContent, case: &roles::Case, linked: bool) {
    for width in [40, 78] {
        let rendered = DocumentView::new(content).render(width);
        let mut app = App::new(content);
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let mut previous_row = None;
        for label in case.rows {
            let found = positions(terminal.backend().buffer(), label);
            assert_eq!(found.len(), 1, "{}/{width}: {label:?}", case.id);
            let (column, row) = found[0];
            if let Some(previous) = previous_row {
                assert_eq!(row, previous + 1, "{}: no CR-created hard row", case.id);
            }
            previous_row = Some(row);
            let matches = rendered.search(label);
            assert_eq!(matches.len(), 1, "{}: safe terminal search", case.id);
            let columns = u16::try_from(label.width()).unwrap();
            for offset in 0..columns {
                click(&mut app, column + offset, row);
                assert_eq!(
                    opened_targets(&mut app),
                    if linked { vec![roles::TARGET] } else { vec![] },
                    "{}: portable fence is inert, original source link is not",
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
                copied_selections(&mut app, |request| match request {
                    CopyRequest::Selection { text } => text,
                    _ => panic!("visual selection"),
                }),
                [*label],
                "{}: safe copy includes replacement without invented LF",
                case.id
            );
        }
    }
}

#[test]
fn table_text_cr_is_not_a_hard_line_in_terminal_or_portable_consumers() {
    for case in roles::CASES {
        for linked in [false, true] {
            let mut content = json_roundtrip::json_string_roundtrip(&case.load(linked));
            for round in 0..=2 {
                if round > 0 {
                    case.assert_portable(&content);
                }
                assert_reader(&content, case, linked && round == 0);
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
}
