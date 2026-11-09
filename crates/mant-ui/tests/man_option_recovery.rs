//! Recovered native option operands in the real terminal viewport.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ui::{App, CopyRequest};
use ratatui::{Terminal, backend::TestBackend};

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

#[test]
fn skipped_option_operand_cannot_change_the_following_link_or_copy_range() {
    // This exact source's pristine tree keeps a third, bare \z operand and
    // post_OP diagnoses it, but pre_OP never executes it. The pinned UTF-8
    // output retains both brackets and the full following link label.
    let matrix: serde_json::Value = serde_json::from_str(include_str!(
        "../../mant-engine/tests/roff_lowering/upstream_inline/man_option_arity.json"
    ))
    .unwrap();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "escape-z/link")
        .unwrap();
    let input = case["source"].as_str().unwrap();
    let content = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_roff_bytes(input.as_bytes()).unwrap(),
    );
    for width in [40, 78] {
        let mut app = App::new(&content);
        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(width + 4, 24)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let visible = positions(buffer, "BEFORE [A B] AFTER");
        assert_eq!(visible.len(), 1, "width {width}: {buffer:?}");
        let label = positions(buffer, "AFTER");
        assert_eq!(label.len(), 1);
        let (column, row) = label[0];
        let end = column + 4;
        click(&mut app, column - 1, row);
        assert_eq!(opened_targets(&mut app), [] as [&str; 0]);
        for edge in [column, end] {
            click(&mut app, edge, row);
            assert_eq!(opened_targets(&mut app), ["https://x.io"]);
        }
        select_span(&mut app, column, row, end, None);
        assert_eq!(
            copied_selections(&mut app, |request| match request {
                CopyRequest::Selection { text } => text,
                _ => panic!("actual pointer selection"),
            }),
            ["AFTER"]
        );
    }
}
