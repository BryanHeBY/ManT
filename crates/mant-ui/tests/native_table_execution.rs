//! Table exit witnesses reach actual cells and the same App after reflow.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend};
#[path = "../../mant-engine/tests/support/table_execution.rs"]
mod cases;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn settle(app: &mut App) {
    let mut now = std::time::Instant::now();
    for _ in 0..16 {
        let Some(delay) = app.next_wakeup(now) else {
            break;
        };
        now += delay;
        app.tick(now);
    }
}

#[test]
fn complete_native_table_witnesses_keep_real_cells_search_and_navigation_edges() {
    for case in &cases::cases() {
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        for width in [40, 78] {
            let rendered = DocumentView::new(&content).render(width);
            let buffer = terminal_cells::paint(&rendered, width);
            let text = (0..buffer.area.height)
                .map(|row| {
                    (0..width)
                        .map(|col| buffer[(col, row)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            cases::assert_follower(&text, case);
            let hits = rendered.search(&case.expected_label);
            assert_eq!(hits.len(), 1, "{} {width}", case.id);
            let hit = &hits[0];
            let row = (0..width)
                .map(|col| buffer[(col, u16::try_from(hit.row).unwrap())].symbol())
                .collect::<String>();
            assert_eq!(
                cases::positions(&row, &case.expected_label),
                [hit.start_column]
            );
            assert_eq!(hit.end_column - hit.start_column, case.expected_label.len());
        }
    }
}

#[test]
fn table_exit_links_and_opposite_controls_keep_copy_reveal_and_same_app_resize() {
    let selected = cases::cases()
        .into_iter()
        .filter(|case| {
            case.id.starts_with("T1-")
                && (case.id.contains("Q01") || case.id.contains("Q03"))
                && (case.id.ends_with("-link") || case.id.ends_with("-nofill"))
        })
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 8);
    for case in selected {
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        let target = case.id.ends_with("-link").then_some("https://example.test");
        let mut app = App::new(&content);
        key(&mut app, KeyCode::Char('B'));
        let mut terminal = Terminal::new(TestBackend::new(44, 5)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        key(&mut app, KeyCode::Char('G'));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert_eq!(
            reader_actions::positions(terminal.backend().buffer(), &case.expected_label),
            Vec::<(u16, u16)>::new(),
            "search starts with witness offscreen"
        );
        key(&mut app, KeyCode::Char('?'));
        for ch in case.expected_label.chars() {
            key(&mut app, KeyCode::Char(ch));
        }
        key(&mut app, KeyCode::Enter);
        settle(&mut app);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert_eq!(
            reader_actions::positions(terminal.backend().buffer(), &case.expected_label).len(),
            1,
            "confirmed search reveals in original 44x5 viewport"
        );
        for width in [44, 12, 16, 82] {
            terminal.backend_mut().resize(width, 30);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let positions =
                reader_actions::positions(terminal.backend().buffer(), &case.expected_label);
            assert_eq!(
                positions.len(),
                1,
                "{} {width}: {}",
                case.id,
                terminal.backend()
            );
            // Observe retained reveal above, then put the copy away from
            // viewport-edge autoscroll (the existing visual selection policy).
            key(&mut app, KeyCode::Home);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let positions =
                reader_actions::positions(terminal.backend().buffer(), &case.expected_label);
            assert_eq!(positions.len(), 1);
            let (col, row) = positions[0];
            let length = u16::try_from(case.expected_label.len()).unwrap();
            for i in 0..length {
                reader_actions::click(&mut app, col + i, row);
                assert_eq!(
                    reader_actions::opened_targets(&mut app),
                    target.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                    "{} {width}",
                    case.id
                );
            }
            for outside in [col.saturating_sub(1), col + length] {
                reader_actions::click(&mut app, outside, row);
                assert_eq!(
                    reader_actions::opened_targets(&mut app),
                    Vec::<String>::new()
                );
            }
            reader_actions::select_span(&mut app, col, row, col + length - 1, Some(col + length));
            assert_eq!(
                reader_actions::copied_selections(&mut app, |request| {
                    let CopyRequest::Selection { text } = request else {
                        panic!("selection")
                    };
                    text
                }),
                std::slice::from_ref(&case.expected_label),
                "{} {width}",
                case.id
            );
        }
    }
}
