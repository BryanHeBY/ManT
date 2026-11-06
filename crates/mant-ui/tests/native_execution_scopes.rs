//! Source-bound field lifetimes survive JSON, viewport layout and real cells.

use mant_ui::{App, DocumentView};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

fn cases() -> Vec<Value> {
    let value: Value = serde_json::from_str(include_str!(
        "../../mant-engine/tests/roff_lowering/execution_scopes/cases.json"
    ))
    .unwrap();
    value["cases"].as_array().unwrap().clone()
}

fn painted_positions(buffer: &ratatui::buffer::Buffer, label: &str) -> Vec<(u16, u16)> {
    let mut found = Vec::new();
    for row in 0..buffer.area.height {
        let text: String = (0..buffer.area.width)
            .map(|column| buffer[(column, row)].symbol())
            .collect();
        found.extend(
            text.match_indices(label)
                .map(|(column, _)| (u16::try_from(text[..column].width()).unwrap(), row)),
        );
    }
    found
}

#[test]
fn native_item_and_display_glyphs_keep_their_source_relative_cells() {
    // Five pinned profiles precede these source-bound expected glyph cells.
    // mdoc_term.c::termp_it_pre/post and print_mdoc_node restore geometry,
    // whereas output owners, marker presentation and viewport wraps do not.
    let mut checked = 0;
    for case in cases() {
        if !case["id"].as_str().unwrap().starts_with("m3-") {
            continue;
        }
        let Some(glyphs) = case["expected"]["glyphs"].as_object() else {
            continue;
        };
        checked += 1;
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        for width in [40, 78] {
            let rendered = DocumentView::new(&content).render(width);
            let painted = terminal_cells::paint(&rendered, width);
            for (label, column) in glyphs {
                let found = painted_positions(&painted, label);
                let source_origin = u16::try_from(column.as_i64().unwrap() + 3).unwrap();
                // The documented viewport contract reserves sixteen reading
                // cells, without altering the logical source-row origin.
                let expected = source_origin.min(width - 16);
                assert_eq!(found.len(), 1, "{}/{width}: {label}", case["id"]);
                assert_eq!(found[0].0, expected, "{}/{width}: {label}", case["id"]);
                assert_eq!(rendered.search(label).len(), 1);
            }
        }
        let mut current = content;
        for _ in 0..2 {
            current = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(
                    &mant_codec::encode::render_markdown(&current),
                    None,
                )
                .unwrap(),
            );
            let rendered = DocumentView::new(&current).render(78);
            for label in glyphs.keys() {
                assert_eq!(rendered.search(label).len(), 1, "{}: {label}", case["id"]);
            }
        }
    }
    assert_eq!(checked, 20, "all recorded native core geometries");
}

#[test]
fn synopsis_width_does_not_split_following_prose_in_actual_reader_cells() {
    let mut checked = 0;
    for case in cases() {
        if !case["id"].as_str().unwrap().starts_with("sy_") {
            continue;
        }
        checked += 1;
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        for width in [40, 78] {
            let rendered = DocumentView::new(&content).render(width);
            let painted = terminal_cells::paint(&rendered, width);
            assert_eq!(
                painted_positions(&painted, "alpha beta gamma delta").len(),
                1,
                "{}/{width}: whole source prose in one real row",
                case["id"]
            );
        }
    }
    assert_eq!(checked, 96);
}

fn copied_visual_selections(app: &mut App) -> Vec<String> {
    reader_actions::copied_selections(app, |request| {
        let mant_ui::CopyRequest::Selection { text } = request else {
            panic!("visual selection copy request")
        };
        text
    })
}

#[test]
fn narrow_display_can_be_revealed_without_changing_its_copy_source() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "m3-tag-enum-d1")
        .unwrap();
    let content = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
    );
    let mut app = App::new(&content);
    let key = |app: &mut App, code| {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    };
    key(&mut app, KeyCode::Char('B'));
    // Hidden-sidebar App reserves two margins plus a two-cell scrollbar
    // gutter. Terminal16 therefore exercises an actual twelve-column reader.
    let mut terminal = Terminal::new(TestBackend::new(16, 8)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    key(&mut app, KeyCode::Char('G'));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        ["DISPLA", "Y"]
            .iter()
            .all(
                |fragment| reader_actions::positions(terminal.backend().buffer(), fragment)
                    .is_empty()
            ),
        "all source fragments are initially outside the body viewport"
    );
    // Start from the bottom, so use the public backward search. Page search
    // deliberately does not wrap by default.
    key(&mut app, KeyCode::Char('?'));
    for character in "DISPLAY".chars() {
        key(&mut app, KeyCode::Char(character));
    }
    key(&mut app, KeyCode::Enter);
    let mut now = std::time::Instant::now();
    for _ in 0..16 {
        let Some(delay) = app.next_wakeup(now) else {
            break;
        };
        now += delay;
        app.tick(now);
    }
    terminal.draw(|frame| app.draw(frame)).unwrap();
    // At twelve columns, the reading policy reserves half the viewport
    // for content: source origin21 clamps to6, leaving six glyph cells.
    // App's outer margin adds one cell. Check the full authored word through
    // both actual visual rows, rather than demanding a single-row substring.
    let first = reader_actions::positions(terminal.backend().buffer(), "DISPLA");
    let last = reader_actions::positions(terminal.backend().buffer(), "Y");
    assert_eq!(
        (first.len(), last.len()),
        (1, 1),
        "confirmed backward search reveals both narrow fragments: {}",
        terminal.backend()
    );
    let (column, row) = first[0];
    let (end_column, end_row) = last[0];
    assert_eq!((column, end_column, end_row), (7, 7, row + 1));
    reader_actions::click(&mut app, column, row);
    assert_eq!(
        reader_actions::opened_targets(&mut app),
        Vec::<String>::new()
    );
    for (kind, column, row) in [
        (MouseEventKind::Down(MouseButton::Left), column, row),
        (MouseEventKind::Drag(MouseButton::Left), end_column, end_row),
        (MouseEventKind::Up(MouseButton::Left), end_column, end_row),
    ] {
        reader_actions::pointer(&mut app, kind, column, row);
    }
    assert_eq!(
        copied_visual_selections(&mut app),
        ["DISPLA\n      Y"],
        "visual copy retains its established row/structural-padding policy"
    );
    assert_eq!(
        DocumentView::new(&content)
            .render(12)
            .search("DISPLAY")
            .len(),
        1
    );
    // Resize this actual App, preserving its source-bound search location.
    // No independent wide view can stand in for reflow/reveal navigation.
    terminal.backend_mut().resize(82, 40);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert_eq!(
        reader_actions::positions(terminal.backend().buffer(), "DISPLAY").len(),
        1,
        "the same search location remains reachable after resize"
    );
    // Keep the second drag away from viewport-edge autoscroll. The resize
    // check above already observed the retained source-bound location.
    key(&mut app, KeyCode::Home);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let wide = reader_actions::positions(terminal.backend().buffer(), "DISPLAY");
    assert_eq!(wide.len(), 1);
    let (column, row) = wide[0];
    reader_actions::select_span(&mut app, column, row, column + 6, None);
    assert_eq!(copied_visual_selections(&mut app), ["DISPLAY"]);
}
