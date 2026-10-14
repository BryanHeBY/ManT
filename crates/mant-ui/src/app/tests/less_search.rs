//! Less-like controls over source-neutral Markdown snapshots.
use super::*;
use mant_protocol::{SearchCase, SearchSyntax};

fn reader(text: &str) -> (App, Terminal<TestBackend>) {
    let document = mant_loader::load_markdown_text(text, None).unwrap();
    let mut app = App::new(&document);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    (app, terminal)
}
fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        key(app, KeyCode::Char(ch));
    }
}
fn finish(app: &mut App) {
    for _ in 0..20_000 {
        match app.search.deadline.or(app.outline_search.deadline) {
            Some(at) => {
                app.tick(at);
            }
            None => return,
        }
    }
    panic!("search failed to complete");
}

#[test]
fn sensitive_regex_defaults_and_literal_switch_are_visible() {
    let (mut app, mut terminal) = reader("# Page\n\nFoo foo FOOD [foo]");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "foo");
    finish(&mut app);
    assert_eq!(app.search.matches.len(), 2);
    assert_eq!(app.search.options.syntax, SearchSyntax::Regex);
    assert_eq!(app.search.options.case, SearchCase::Sensitive);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("Regex · Case"));
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT));
    finish(&mut app);
    assert_eq!(app.search.matches.len(), 4);
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_text(&mut app, "[foo]");
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    finish(&mut app);
    assert_eq!(app.search.matches.len(), 1);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("Literal · Ignore case")
    );
}

#[test]
fn invalid_regex_is_not_a_miss_or_a_literal_fallback_and_cannot_confirm() {
    let (mut app, mut terminal) = reader("# Page\n\n[foo]");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "[");
    assert!(app.search.error.is_some());
    key(&mut app, KeyCode::Enter);
    assert!(app.search_is_open());
    assert_eq!(app.search.query, "");
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("Invalid regex"));
    assert!(!terminal.backend().to_string().contains("No matches"));
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    finish(&mut app);
    assert!(app.search.error.is_none());
    assert_eq!(app.search.matches.len(), 1);
    key(&mut app, KeyCode::Enter);
    assert!(!app.search_is_open());
}

#[test]
fn smart_case_and_explicit_regex_flags_use_the_same_engine() {
    for (pattern, expected) in [("foo", 3), ("Foo", 1), ("(?i)Foo", 3), ("(?-i)foo", 1)] {
        let (mut app, _) = reader("# Page\n\nfoo Foo FOO");
        key(&mut app, KeyCode::Char('/'));
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT));
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT));
        type_text(&mut app, pattern);
        finish(&mut app);
        assert_eq!(app.search.matches.len(), expected, "{pattern}");
    }
}

#[test]
fn enter_returns_to_reading_keeps_highlights_and_empty_enter_reuses_query() {
    let (mut app, mut terminal) = reader("# Page\n\nneedle\n\nneedle\n\nbody");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    assert!(!app.search_is_open());
    assert!(app.search.highlighted);
    let row = app.session.content_scroll;
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.session.content_scroll, row + 1);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.bg == theme::SEARCH_MATCH || cell.bg == theme::SEARCH_ACTIVE)
    );
    key(&mut app, KeyCode::Char('/'));
    assert_eq!(app.search.draft, "");
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    assert_eq!(app.search.query, "needle");
    assert!(!app.search_is_open());
    key(&mut app, KeyCode::Esc);
    assert!(!app.search.highlighted);
    assert_eq!(app.search.matches, Vec::new());
    key(&mut app, KeyCode::Char('N'));
    finish(&mut app);
    assert!(app.search.highlighted);
}

#[test]
fn forward_and_backward_confirmation_start_at_the_reading_position() {
    for opener in ['/', '?'] {
        let (mut app, _) = reader("# Page\n\nneedle one\n\nneedle two\n\nneedle three\n\nend");
        let hits = app.session.rendered_cache[&app.geometry.content.width].search("needle");
        app.session.content_scroll = hits[1].row + 1;
        key(&mut app, KeyCode::Char(opener));
        type_text(&mut app, "needle");
        let origin = app.session.content_scroll;
        finish(&mut app);
        assert_eq!(app.session.content_scroll, origin);
        key(&mut app, KeyCode::Enter);
        finish(&mut app);
        assert_eq!(app.search.active_match, if opener == '/' { 2 } else { 1 });
        key(&mut app, KeyCode::Char('n'));
        finish(&mut app);
        assert_eq!(app.search.active_match, if opener == '/' { 2 } else { 0 });
        if opener == '?' {
            key(&mut app, KeyCode::Char('N'));
            assert_eq!(app.search.active_match, 1);
        }
    }
}

#[test]
fn wrap_is_opt_in_and_never_wraps_a_truncated_inventory() {
    let (mut app, _) = reader("# Page\n\nneedle needle");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    key(&mut app, KeyCode::Char('N'));
    assert_eq!(app.search.active_match, 0);
    assert!(app.notice.as_ref().unwrap().contains("No more"));
    app.search.wrap = true;
    key(&mut app, KeyCode::Char('N'));
    assert_eq!(app.search.active_match, 1);
    assert!(app.notice.as_ref().unwrap().contains("wrapped"));
    app.search.limited = true;
    key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.search.active_match, 1);
    assert!(app.notice.as_ref().unwrap().contains("partial"));
}

#[test]
fn cancel_restores_confirmed_case_syntax_and_direction() {
    let (mut app, _) = reader("# Page\n\nFoo foo");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "Foo");
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    key(&mut app, KeyCode::Char('?'));
    type_text(&mut app, "foo");
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT));
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    finish(&mut app);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.search.query, "Foo");
    assert_eq!(app.search.options.case, SearchCase::Sensitive);
    assert_eq!(app.search.options.syntax, SearchSyntax::Regex);
    assert_eq!(app.search.repeat_direction(), 1);
    key(&mut app, KeyCode::Char('n'));
    finish(&mut app);
    assert_eq!(app.search.scope_matches.len(), 1);
}

#[test]
fn logical_anchors_word_boundaries_and_regex_ranges_survive_reflow() {
    let (mut app, mut terminal) =
        reader("# Page\n\n--git-dir=path and --work-tree=other; notword needleword\n\nneedle");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, r"^--(git-dir|work-tree)=\w+|\bneedle\b");
    finish(&mut app);
    assert_eq!(app.search.matches.len(), 2);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char('n'));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let mut narrow = Terminal::new(TestBackend::new(35, 24)).unwrap();
    narrow.draw(|frame| app.draw(frame)).unwrap();
    finish(&mut app);
    assert_eq!(app.search.scope_matches.len(), 2);
    assert_eq!(app.search.active_match, 1);
}

#[test]
fn outline_uses_the_same_options_and_confirms_relative_to_selected_node() {
    let (mut app, _) = reader("# Page\n\n## Needle one\n\n## needle two\n\n## needle three");
    app.selected = 1;
    app.open_outline_search();
    type_text(&mut app, "needle");
    finish(&mut app);
    assert_eq!(app.outline_search.matches.len(), 2);
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    assert!(!app.search_is_open());
    assert_eq!(app.selected, app.outline_search.matches[0].node_index);
    key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.outline_search.active_match, 1);
    key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.outline_search.active_match, 1);
}

#[test]
fn regex_inside_a_large_record_yields_and_can_be_replaced_without_a_late_jump() {
    let (mut app, _) = reader(&format!("# Page\n\na{}z\n\nneedle", "x".repeat(100_000)));
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "a.*z");
    app.tick(app.search.deadline.unwrap());
    assert!(app.search.job.is_some());
    assert_eq!(app.search.matches, Vec::new());
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_text(&mut app, "needle");
    finish(&mut app);
    assert_eq!(app.search.matches.len(), 1);
    assert_eq!(app.session.content_scroll, 0);
}

#[test]
fn repeated_search_tracks_manual_reading_moves_not_an_old_result_cursor() {
    let (mut app, _) = reader("# Page\n\nneedle one\n\nneedle two\n\nneedle three");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    app.session.content_scroll = app.search.scope_matches[1].rendered.row + 1;
    key(&mut app, KeyCode::Char('n'));
    finish(&mut app);
    assert_eq!(app.search.active_match, 2);
    app.open_outline_search();
    type_text(&mut app, "Page");
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    assert_eq!(app.outline_search.input.query, "Page");
}

#[test]
fn menus_change_the_retained_query_without_discarding_text() {
    let (mut app, _) = reader("# Page\n\nFoo foo");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "Foo");
    key(&mut app, KeyCode::Enter);
    app.activate_menu_action(MenuAction::SearchCase);
    assert_eq!(app.search.draft, "Foo");
    finish(&mut app);
    assert_eq!(app.search.matches.len(), 2);
    app.activate_menu_action(MenuAction::SearchSyntax);
    assert_eq!(app.search.draft, "Foo");
    assert_eq!(app.search.options.syntax, SearchSyntax::Literal);
}

#[test]
fn compact_search_prompt_leaves_room_for_input_and_reports_regex_errors() {
    let (mut app, _) = reader("# Page\n\nbody");
    key(&mut app, KeyCode::Char('?'));
    type_text(&mut app, "[");
    for width in [40, 20, 8] {
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let screen = terminal.backend().to_string();
        assert!(screen.contains('?'));
        if width >= 20 {
            assert!(screen.contains("invalid regex"));
        }
        assert!(app.search_is_open());
    }
}

#[test]
fn empty_repeat_validates_the_last_pattern_before_committing_new_modes() {
    let (mut app, _) = reader("# Page\n\n[");
    key(&mut app, KeyCode::Char('/'));
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    type_text(&mut app, "[");
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    key(&mut app, KeyCode::Char('/'));
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert_eq!(app.search.draft, "");
    key(&mut app, KeyCode::Enter);
    assert!(app.search_is_open());
    assert!(app.search.error.is_some());
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.search.options.syntax, SearchSyntax::Literal);
    assert_eq!(app.search.query, "[");
}

#[test]
fn escape_clears_resumed_highlights_after_visiting_another_search_scope() {
    let (mut app, _) = reader("# Page\n\nneedle needle");
    key(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    finish(&mut app);
    app.open_outline_search();
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('n'));
    finish(&mut app);
    assert!(app.search.highlighted);
    key(&mut app, KeyCode::Esc);
    assert!(!app.search.highlighted);
    assert_eq!(app.search.matches, Vec::new());
    assert_eq!(app.search.query, "needle");
}
