//! Source rows and their typed labels reach the real terminal viewport.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

const TAB: &str = include_str!("../../mant-engine/tests/roff_lowering/source_line_consumers/tab.1");
const BO_CONTINUED: &str = include_str!(
    "../../mant-engine/tests/roff_lowering/source_line_consumers/bo-continued-space.1"
);
const INSET: &str =
    include_str!("../../mant-engine/tests/roff_lowering/source_line_consumers/inset.1");
const LINK_TAB: &str =
    include_str!("../../mant-engine/tests/roff_lowering/source_line_consumers/link-tab.1");
const LINK_SPACE: &str =
    include_str!("../../mant-engine/tests/roff_lowering/source_line_consumers/link-space.1");

fn load(source: &str) -> ResolvedContent {
    json_roundtrip::json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap())
}

fn paint(content: &ResolvedContent, width: u16) -> (App, Terminal<TestBackend>) {
    let mut app = App::new(content);
    app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
    // Two outer cells and the scrollbar gutter are outside the document.
    let mut terminal = Terminal::new(TestBackend::new(width + 4, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    (app, terminal)
}

fn position(buffer: &Buffer, marker: &str) -> (u16, u16) {
    let found = positions(buffer, marker);
    assert_eq!(found.len(), 1, "one painted {marker}: {buffer:?}");
    found[0]
}

#[test]
fn actual_viewport_keeps_source_break_inside_enclosure_and_definition_head() {
    // These complete fixtures ran against pristine ASCII/UTF-8/HTML/tree/lint
    // first. The terminal's TEXT visitor distinguishes SP from TAB after the
    // Bo generated prefix. The inset HEAD break precedes its BODY handoff.
    for width in [40, 78] {
        let (_, terminal) = paint(&load(TAB), width);
        let buffer = terminal.backend().buffer();
        let (before, middle, after) = (
            position(buffer, "BEFORE"),
            position(buffer, "MIDDLE"),
            position(buffer, "AFTER"),
        );
        assert_eq!((before.1, middle.1), (middle.1, after.1), "width {width}");
        assert!(before.0 < middle.0 && middle.0 < after.0);

        let (_, terminal) = paint(&load(BO_CONTINUED), width);
        let buffer = terminal.backend().buffer();
        let (column, row) = position(buffer, "BEFOREMARK[");
        let (middle, middle_row) = position(buffer, "MIDDLEMARK] AFTERMARK");
        assert_eq!((middle, middle_row), (column + 1, row + 1));
        assert_eq!(buffer[(column, middle_row)].symbol(), " ");

        let (_, terminal) = paint(&load(INSET), width);
        let buffer = terminal.backend().buffer();
        let (column, row) = position(buffer, "BEFOREMARK");
        let (middle, middle_row) = position(buffer, "MIDDLEMARK AFTERMARK");
        assert_eq!((middle, middle_row), (column + 1, row + 1));
        assert_eq!(buffer[(column, middle_row)].symbol(), " ");
        let (body, body_row) = position(buffer, "BODYMARK");
        assert_eq!(body_row, middle_row);
        assert!(body > middle + u16::try_from("MIDDLEMARK AFTERMARK".len()).unwrap());
    }
}

#[test]
fn hard_line_and_filled_tab_keep_the_link_click_and_copy_ranges() {
    // man_term.c visits the two TEXT children within UR BODY. The source
    // ASCII SP adds one hard row inside the link. post_UR's separate URI
    // word is visible but does not own either label's hit range.
    for (source, offset) in [(LINK_TAB, 0), (LINK_SPACE, 1)] {
        let content = load(source);
        for width in [40, 78] {
            let (mut app, terminal) = paint(&content, width);
            let buffer = terminal.backend().buffer();
            let alpha = position(buffer, "ALPHA");
            let beta = position(buffer, "BETA");
            assert_eq!(beta.1, alpha.1 + offset);
            assert_eq!(position(buffer, "POST").1, beta.1 + 1);

            for (word, (column, row)) in [("ALPHA", alpha), ("BETA", beta)] {
                let last = column + u16::try_from(word.len()).unwrap() - 1;
                for boundary in [column, last] {
                    click(&mut app, boundary, row);
                    assert_eq!(opened_targets(&mut app), ["https://x.io"]);
                }
                select_span(&mut app, column, row, last, None);
                assert_eq!(
                    copied_selections(&mut app, |request| match request {
                        CopyRequest::Selection { text } => text,
                        _ => panic!("actual pointer selection"),
                    }),
                    [word]
                );
            }
            for unlinked in ["PRE", "https://x.io", "POST"] {
                let (column, row) = position(buffer, unlinked);
                click(&mut app, column, row);
                assert!(opened_targets(&mut app).is_empty(), "{unlinked}");
            }
        }
    }
}
