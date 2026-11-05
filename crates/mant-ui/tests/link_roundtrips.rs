//! Original link scalars and metadata survive encoding and actual reader input.

use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest};
use ratatui::{Terminal, backend::TestBackend};
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "../../mant-engine/tests/support/link_evidence.rs"]
mod link_evidence;
#[path = "../../mant-engine/tests/support/link_meaning.rs"]
mod link_meaning;
#[path = "support/reader_actions.rs"]
mod reader_actions;
use json_roundtrip::json_string_roundtrip;
use link_meaning::{Meaning, SOURCES};
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

fn assert_reader(content: &ResolvedContent, label: &str, target: &str) {
    for width in [40, 78] {
        let mut app = App::new(content);
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let matches = positions(terminal.backend().buffer(), label);
        assert_eq!(
            matches.len(),
            1,
            "{width}: exact visible link label {label:?}"
        );
        let (column, row) = matches[0];
        let columns = u16::try_from(label.width()).unwrap();
        for offset in 0..columns {
            click(&mut app, column + offset, row);
            assert_eq!(
                opened_targets(&mut app),
                [target],
                "{width}: label cell {offset}"
            );
        }
        for outside in [column - 1, column + columns] {
            click(&mut app, outside, row);
            assert!(
                opened_targets(&mut app).is_empty(),
                "{width}: outside label"
            );
        }
        select_span(&mut app, column, row, column + columns - 1, None);
        assert_eq!(
            copied_selections(&mut app, |request| {
                let CopyRequest::Selection { text } = request else {
                    panic!("actual visual copy")
                };
                text
            }),
            [label]
        );
    }
}

#[test]
fn metadata_never_changes_painted_label_copy_or_click_ranges_after_two_reads() {
    for (index, link) in SOURCES.iter().enumerate().filter(|(index, _)| *index != 9) {
        let source = format!("# Probe\n\n## Payload\n\n{link}\n\n## Next\n\nEND\n");
        let original = mant_loader::load_markdown_text(&source, None).unwrap();
        let expected = Meaning::capture(&original);
        let target = expected.target().to_uri().unwrap();
        let label = if index == 0 {
            "! a "
        } else if index == 8 {
            "a*b"
        } else {
            "LinkWord"
        };
        let mut content = json_string_roundtrip(&original);
        for round in 0..=2 {
            expected.assert_preserved(&content, &format!("case {index}/round {round}"));
            assert_reader(&content, label, &target);
            if round < 2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
            }
        }
    }
}
