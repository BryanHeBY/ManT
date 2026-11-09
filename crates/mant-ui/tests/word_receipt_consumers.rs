//! Stable source owners and accepted rows reach the reader's actual cells.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

const WORD: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_receipts/accepted-word.1");
const HARD_ROW: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_receipts/late-hard-row.1");
const LINK: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_receipts/accepted-unicode-link.1");
const LABEL: &str = "界e\u{301}字";

fn only_position(buffer: &Buffer, text: &str) -> (u16, u16) {
    let hits = positions(buffer, text);
    assert_eq!(hits.len(), 1, "{text:?}: {hits:?}");
    hits[0]
}

fn paint(content: &ResolvedContent, width: u16) -> (App, Terminal<TestBackend>) {
    let mut app = App::new(content);
    app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(width + 4, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    (app, terminal)
}

fn copy(app: &mut App, at: (u16, u16), expected: &str) {
    let (column, row) = at;
    let last = column + u16::try_from(expected.width()).unwrap() - 1;
    select_span(app, column, row, last, None);
    assert_eq!(
        copied_selections(app, |request| match request {
            CopyRequest::Selection { text } => text,
            _ => panic!("expected reader selection"),
        }),
        [expected]
    );
}

fn assert_rows(content: &ResolvedContent, width: u16, expected: &[&str], word: &str) {
    let (mut app, terminal) = paint(content, width);
    let buffer = terminal.backend().buffer();
    let (origin, first) = only_position(buffer, expected[0]);
    for (offset, expected) in expected.iter().enumerate() {
        let at = only_position(buffer, expected);
        assert_eq!(at, (origin, first + u16::try_from(offset).unwrap()));
        let visible_end = origin + u16::try_from(expected.width()).unwrap();
        // The last two cells are outside the body. Every cell after the
        // expected row must be empty, including any unexpected suffix.
        assert!(
            (visible_end..width + 2).all(|x| buffer[(x, at.1)].symbol() == " "),
            "{expected:?}: unexpected right-hand cells at width {width}"
        );
    }
    let (column, row) = only_position(buffer, word);
    let rendered = DocumentView::new(content).render(width);
    let hits = rendered.search(word);
    assert_eq!(hits.len(), 1);
    let heading = rendered.anchor_row("description").unwrap();
    assert_eq!(
        hits[0].row - heading,
        usize::from(row - only_position(buffer, "DESCRIPTION").1)
    );
    assert_eq!(hits[0].end_column - hits[0].start_column, word.width());
    if word == LABEL {
        // The first blank belongs to the earlier empty word. The second is
        // the equivalent separator in the original styled link label.
        assert_eq!(column - origin, 8);
        for (cell, expected) in [
            (column - 2, None),
            (column - 1, Some("https://x.io")),
            (column, Some("https://x.io")),
            (column + u16::try_from(word.width()).unwrap(), None),
        ] {
            click(&mut app, cell, row);
            assert_eq!(
                opened_targets(&mut app),
                expected.into_iter().collect::<Vec<_>>()
            );
        }
    }
    copy(&mut app, (column, row), word);
}

#[test]
fn accepted_source_rows_survive_json_at_two_actual_reader_widths() {
    // Exact .1 sources ran against all five pristine profiles. The two
    // columns of lead space come from distinct term_word() writes. In the
    // delayed case term_flushln() ends ALPHA before X is published.
    for (source, expected, word) in [
        (WORD, &["ALPHAX  BETA", "OMEGA"][..], "BETA"),
        (HARD_ROW, &["ALPHA", "XBETA", "OMEGA"][..], "XBETA"),
        (
            LINK,
            &["ALPHAX  界e\u{301}字: https://x.io", "OMEGA"][..],
            LABEL,
        ),
    ] {
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
        );
        for width in [40, 78] {
            assert_rows(&content, width, expected, word);
        }
    }
}
