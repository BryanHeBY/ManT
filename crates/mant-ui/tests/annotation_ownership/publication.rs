//! Exercise real pointer hits and selection after source annotation publication.
use super::*;
use serde_json::Value;

fn source(id: &str) -> Vec<u8> {
    include_str!("../../../mant-engine/tests/roff_lowering/annotation_publication/cases.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|case| case["id"] == id)
        .unwrap()["source"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec()
}

#[test]
fn source_label_excludes_leading_motion_in_resized_reader() {
    // man_term.c runs the zero-width prefix and native horizontal movement
    // before LABEL. The pristine HTML anchor starts at the L.
    let id = "N2/UR/first/zero/2n/same";
    let content =
        json_roundtrip::json_string_roundtrip(&mant_loader::load_roff_bytes(&source(id)).unwrap());
    let mut app = App::new(&content);
    key(&mut app, KeyCode::Char('B'));
    let mut terminal = Terminal::new(TestBackend::new(78, 40)).unwrap();
    for width in [78, 40, 16, 12, 78] {
        terminal.backend_mut().resize(width, 40);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        key(&mut app, KeyCode::Home);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let cells = wrapped_positions(buffer, "LABEL")
            .pop()
            .expect("visible label");
        let (first, row) = cells[0];
        assert!(first > 0, "{id} {width}");
        reader_actions::click(&mut app, first - 1, row);
        assert!(
            reader_actions::opened_targets(&mut app).is_empty(),
            "{id} {width}"
        );
        for &(column, row) in &[cells[0], *cells.last().unwrap()] {
            reader_actions::click(&mut app, column, row);
            assert_eq!(
                reader_actions::opened_targets(&mut app),
                ["https://one.test"]
            );
        }
        check_copy(&mut app, &cells, "LABEL", id);
    }
}

#[test]
fn example_end_break_keeps_both_rows_clickable_under_the_same_target() {
    // man_term.c::pre_literal executes the .EE break. The CVS terminal starts
    // LASTMARK on the next line and HTML retains both words in the anchor.
    let id = "N4/UR/filled/EE/text";
    let content =
        json_roundtrip::json_string_roundtrip(&mant_loader::load_roff_bytes(&source(id)).unwrap());
    let mut app = App::new(&content);
    key(&mut app, KeyCode::Char('B'));
    let mut terminal = Terminal::new(TestBackend::new(78, 40)).unwrap();
    for width in [78, 40, 16, 12, 78] {
        terminal.backend_mut().resize(width, 40);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        key(&mut app, KeyCode::Home);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let first = wrapped_positions(buffer, "FIRSTMARK")
            .pop()
            .expect("first label row");
        let last = wrapped_positions(buffer, "LASTMARK")
            .pop()
            .expect("second label row");
        assert_eq!(last[0].1, first.last().unwrap().1 + 1, "{id} {width}");
        for (cells, spelling) in [(first, "FIRSTMARK"), (last, "LASTMARK")] {
            let (column, row) = cells[0];
            reader_actions::click(&mut app, column, row);
            assert_eq!(
                reader_actions::opened_targets(&mut app),
                ["https://one.test"]
            );
            check_copy(&mut app, &cells, spelling, id);
        }
    }
}
