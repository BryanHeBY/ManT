//! Source-neutral live-search scheduling and cancellation, with real buffers.
use super::*;

fn page(text: &str) -> ResolvedContent {
    mant_loader::load_markdown_text(&format!("# Sample\n\n{text}\n"), None).unwrap()
}

fn type_query(app: &mut App, query: &str) {
    if !app.search_is_open() {
        if app.search_target == super::super::search::SearchTarget::Outline {
            app.open_outline_search();
        } else {
            app.open_search();
        }
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for character in query.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
}

fn finish(app: &mut App) {
    for _ in 0..10_000 {
        let Some(deadline) = app.search.deadline.or(app.outline_search.deadline) else {
            return;
        };
        app.tick(deadline);
    }
    panic!("finite search did not finish");
}

#[test]
fn preview_is_debounced_highlights_without_confirmation_and_reuses_layout() {
    let mut app = App::new(&page("a searchable phrase and another searchable phrase"));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let width = app.geometry.content.width;
    let layout = std::sync::Arc::clone(&app.session.rendered_cache[&width]);
    app.open_search();
    let location = app.session.content_scroll;
    let selected = app.selected;
    type_query(&mut app, "searchable phrase");
    let deadline = app.search.deadline.unwrap();
    app.tick(deadline.checked_sub(Duration::from_millis(1)).unwrap());
    assert_eq!(app.search.matches, Vec::new());
    app.tick(deadline);
    assert_eq!(app.search.matches, layout.search("searchable phrase"));
    assert_eq!(app.search.matches.len(), 2);
    assert!(
        app.search.query.is_empty(),
        "preview is not a confirmed query"
    );
    assert_eq!(app.session.content_scroll, location);
    assert_eq!(app.selected, selected);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &layout,
        &app.session.rendered_cache[&width]
    ));
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.bg == theme::SEARCH_MATCH)
    );
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.search.query, "searchable phrase");
    assert_eq!(app.session.content_scroll, app.search.matches[0].row);
}

#[test]
fn a_long_single_record_yields_and_old_queries_cannot_publish_after_edits() {
    let mut app = App::new(&page(&format!("{}needle İstanbul", "x".repeat(100_000))));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "needle");
    let old_deadline = app.search.deadline.unwrap();
    app.tick(old_deadline);
    assert!(app.search.job.is_some());
    assert_eq!(app.search.matches, Vec::new());
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen = terminal.backend().to_string();
    assert!(screen.contains("Searching") || screen.contains("searching"));
    assert!(!screen.contains("No matches"));
    type_query(&mut app, "İstanbul");
    let new_deadline = app.search.deadline.unwrap();
    app.tick(old_deadline);
    assert_eq!(app.search.matches, Vec::new());
    app.tick(new_deadline);
    assert!(app.search.job.is_some());
    finish(&mut app);
    assert_eq!(
        app.search.matches,
        app.session.rendered_cache[&app.geometry.content.width].search("İstanbul")
    );
    assert_eq!(app.search.matches.len(), 1);
    assert_eq!(
        app.session.content_scroll, 0,
        "finishing preview never jumps"
    );
    type_query(&mut app, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.tick(Instant::now() + Duration::from_secs(5));
    assert!(app.search.job.is_none());
    assert!(app.search.deadline.is_none());
    assert_eq!(app.search.matches, Vec::new());
}

#[test]
fn navigation_waits_for_later_matches_instead_of_wrapping_partial_results() {
    let mut app = App::new(&page(&format!("needle {} needle", "x".repeat(100_000))));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.search.scope_matches.len(), 1);
    assert!(app.search.job.is_some());
    let first = app.session.content_scroll;
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.session.content_scroll, first);
    assert_eq!(app.search.active_match, 0);
    finish(&mut app);
    assert_eq!(app.search.active_match, 1);
    assert!(app.session.content_scroll > first);
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.search.active_match, 1, "default search does not wrap");
}

#[test]
fn repeated_hits_have_a_disclosed_bound_and_query_input_is_utf8_bounded() {
    let mut app = App::new(&page(&"a ".repeat(5000)));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "a");
    finish(&mut app);
    assert_eq!(app.search.scope_matches.len(), 4096);
    assert!(app.search.limited);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("partial"));
    type_query(&mut app, &"界".repeat(2000));
    assert!(app.search.draft.len() <= crate::document::MAX_QUERY_BYTES);
    assert!(app.search.draft.is_char_boundary(app.search.cursor));
    type_query(&mut app, "");
    assert!(app.search.job.is_none());
    assert_eq!(app.search.matches, Vec::new());
}

#[test]
fn reflow_discards_stale_cell_ranges_and_retains_the_active_source_match() {
    let mut app = App::new(&page(
        "alpha searchable phrase beta searchable phrase gamma",
    ));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "searchable phrase");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.search.active_match, 1);
    let mut narrow = Terminal::new(TestBackend::new(28, 20)).unwrap();
    narrow.draw(|frame| app.draw(frame)).unwrap();
    assert_eq!(app.search.matches, Vec::new());
    finish(&mut app);
    let expected =
        app.session.rendered_cache[&app.geometry.content.width].search("searchable phrase");
    assert_eq!(app.search.matches, expected);
    assert_eq!(app.search.active_match, 1);
    narrow.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        narrow
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.bg == theme::SEARCH_ACTIVE)
    );
}

#[test]
fn outline_preview_keeps_folds_and_location_until_explicit_result_navigation() {
    let source = format!(
        "# Sample\n\n## Parent\n\n### {}needle\n\nbody\n",
        "x".repeat(20_000)
    );
    let mut app = App::new(&mant_loader::load_markdown_text(&source, None).unwrap());
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.expanded.clear();
    let selected = app.selected;
    let location = app.session.content_scroll;
    app.open_outline_search();
    type_query(&mut app, "needle");
    app.tick(app.outline_search.deadline.unwrap());
    assert!(app.outline_search.job.is_some());
    finish(&mut app);
    assert_eq!(app.outline_search.matches.len(), 1);
    assert_eq!(app.selected, selected);
    assert_eq!(app.session.content_scroll, location);
    assert!(app.expanded.is_empty());
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_ne!(app.selected, selected);
    assert!(
        app.expanded.is_empty(),
        "temporary reveal does not rewrite user folds"
    );
    assert!(!app.outline_search.revealed.is_empty());
    type_query(&mut app, "absent");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    finish(&mut app);
    assert!(app.outline_search.job.is_none());
}

#[test]
fn changing_document_retires_pending_search_and_preserves_reading_focus() {
    let mut app = App::new(&page(&"x".repeat(30_000)));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "needle");
    let deadline = app.search.deadline.unwrap();
    app.replace_document(
        std::sync::Arc::new(page("needle")),
        super::super::DocumentChangeReason::Open,
    );
    app.tick(deadline + Duration::from_secs(1));
    assert!(app.search.job.is_none());
    assert!(app.search.scope_matches.is_empty());
    assert_eq!(app.focus, super::super::focus::ReaderFocus::Content);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "needle");
    finish(&mut app);
    assert_eq!(
        app.search.matches.len(),
        1,
        "single-page search follows the new snapshot"
    );
}

#[test]
fn cancelling_preview_retains_confirmed_query_and_active_hit_in_both_scopes() {
    let mut app = App::new(&page(
        "## needle one\n\nneedle body\n\n## needle two\n\nneedle body",
    ));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.search.active_match, 1);
    type_query(&mut app, "absent");
    finish(&mut app);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        app.search.deadline.is_none(),
        "drawing closed search must not resume it"
    );
    assert_eq!(app.search.query, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
    finish(&mut app);
    assert_eq!(
        app.search.active_match, 0,
        "resume from the confirmed second hit"
    );

    app.open_outline_search();
    type_query(&mut app, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.outline_search.active_match, 1);
    type_query(&mut app, "absent");
    finish(&mut app);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.outline_search.input.query, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
    finish(&mut app);
    assert_eq!(app.outline_search.active_match, 0);
}

#[test]
fn multi_document_preview_never_activates_a_hit_and_current_extras_stay_bounded() {
    use std::sync::Arc;
    let first = Arc::new(page("ordinary text"));
    let second = Arc::new(page("needle"));
    let mut app = App::from_shared(super::super::ReaderOptions {
        current: Arc::clone(&first),
        scope: vec![first, second],
        catalog: DocumentCatalog::default(),
    });
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let original = Arc::clone(&app.session.current_bundle);
    app.open_search();
    type_query(&mut app, "needle");
    finish(&mut app);
    assert_eq!(app.search.scope_matches.len(), 1);
    assert_eq!(app.search.matches.len(), 0);
    assert!(Arc::ptr_eq(&original, &app.session.current_bundle));
    assert!(app.take_open_request().is_none());
    assert_eq!(app.navigation.history_lengths().0, 0);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!Arc::ptr_eq(&original, &app.session.current_bundle));
    assert_eq!(app.navigation.history_lengths().0, 1);
    for _ in 0..10 {
        app.replace_document(
            Arc::new(page("current needle")),
            super::super::DocumentChangeReason::Open,
        );
        assert_eq!(app.scope_documents.len(), 3);
    }
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "current needle");
    finish(&mut app);
    assert_eq!(app.search.matches.len(), 1);
}

#[test]
fn passive_result_updates_coalesce_but_completion_and_navigation_redraw_immediately() {
    use super::super::search::SearchRepaint;
    let mut repaint = SearchRepaint::default();
    let now = Instant::now();
    assert!(repaint.update(now, true, false, false));
    assert!(!repaint.update(now + Duration::from_millis(8), true, false, false));
    assert!(!repaint.update(now + Duration::from_millis(49), false, false, false));
    assert!(repaint.update(now + Duration::from_millis(50), false, false, false));
    assert!(repaint.update(now + Duration::from_millis(51), false, false, true));
    assert!(repaint.update(now + Duration::from_millis(52), false, true, false));
}

#[test]
fn reopening_a_suspended_draft_replaces_results_used_by_closed_confirmed_navigation() {
    let mut app = App::new(&page("needle needle"));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    type_query(&mut app, "absent");
    app.open_outline_search();
    app.close_outline_search();
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    finish(&mut app);
    assert_eq!(app.search.scope_matches.len(), 2);
    app.open_search();
    assert_eq!(app.search.draft, "absent");
    assert_eq!(app.search.query, "needle");
    finish(&mut app);
    assert_eq!(app.search.matches, Vec::new());
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.search.query, "absent");
    assert_eq!(app.search.matches, Vec::new());
}

#[test]
fn a_modal_surface_cancels_deferred_result_navigation_but_not_matching() {
    let mut app = App::new(&page(&format!("{}needle", "x".repeat(100_000))));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.search.job.is_some());
    app.open_menu(MenuId::Manual);
    let scroll = app.session.content_scroll;
    finish(&mut app);
    assert_eq!(app.search.scope_matches.len(), 1);
    assert_eq!(app.session.content_scroll, scroll);
    assert!(matches!(app.overlay, Overlay::Menu { .. }));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.session.content_scroll, scroll);
}

#[test]
fn active_scope_menu_navigation_confirms_preview_like_result_arrows() {
    let mut app = App::new(&page("needle absent absent"));
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    type_query(&mut app, "needle");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    type_query(&mut app, "absent");
    finish(&mut app);
    app.activate_menu_action(MenuAction::FindNext);
    assert_eq!(app.search.query, "absent");
    assert!(!app.search.is_editing());
    assert_eq!(app.search.active_match, 0);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(
        app.search.active_match, 1,
        "default search stops at the last match"
    );
}
