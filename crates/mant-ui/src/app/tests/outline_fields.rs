//! Field-local regex assertions must survive the real Markdown outline path.
use super::*;

#[test]
fn associated_heading_labels_match_as_complete_fields_without_duplicate_nodes() {
    let bundle =
        mant_loader::load_markdown_text("# Page\n\n## [cmd](target.md) extra\n\nbody\n", None)
            .unwrap();
    for (pattern, expected_field) in [
        ("cmd", "cmd extra"),
        ("^cmd$", "cmd"),
        ("^cmd extra$", "cmd extra"),
        ("\\bcmd\\b", "cmd extra"),
        ("^target$", "target"),
    ] {
        let mut app = App::new(&bundle);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
        for character in pattern.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for _ in 0..10_000 {
            let Some(at) = app.outline_search.deadline else {
                break;
            };
            app.tick(at);
        }
        assert!(app.outline_search.deadline.is_none());
        assert_eq!(app.outline_search.matches.len(), 1, "{pattern}");
        let hit = &app.outline_search.matches[0];
        assert_eq!(
            hit.fields[0].field.text.as_ref(),
            expected_field,
            "{pattern}"
        );
        assert_eq!(hit.node_index, app.selected);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(!terminal.backend().to_string().contains("No matches"));
        let highlighted = app
            .outline_rows(&app.visible_navigation_indices(), 40, usize::MAX)
            .iter()
            .flat_map(|row| &row.line.spans)
            .filter(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(
            highlighted,
            if pattern == "^target$" {
                "target"
            } else if pattern == "^cmd extra$" {
                "cmd extra"
            } else {
                "cmd"
            },
            "{pattern}"
        );
    }
}
