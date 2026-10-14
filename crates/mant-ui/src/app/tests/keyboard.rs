//! Reading focus is independent of the selected outline location and overlays.
use super::super::focus::ReaderFocus;
use super::*;
use crate::layout::MIN_SIDEBAR_WIDTH;

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn starts_in_content_and_tab_switches_without_jumping_the_reader() {
    let mut app = App::new(&navigation_bundle());
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert_eq!(app.focus, ReaderFocus::Content);
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("Content · j/k scroll")
    );
    assert!(!terminal.backend().to_string().contains("▶ OUTLINE"));
    let selected = app.selected;
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.session.content_scroll, 1);
    assert_eq!(app.selected, selected);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, ReaderFocus::Outline);
    assert_eq!(app.session.content_scroll, 1);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("▶ OUTLINE"));
    key(&mut app, KeyCode::Char('j'));
    assert_ne!(app.selected, selected);
    let location = app.session.content_scroll;
    key(&mut app, KeyCode::BackTab);
    assert_eq!(app.focus, ReaderFocus::Content);
    assert_eq!(app.session.content_scroll, location);
    app.tick(Instant::now() + NAVIGATION_SYNC_IDLE);
    assert_eq!(app.focus, ReaderFocus::Content);
}

#[test]
fn edges_and_page_steps_use_the_focused_region_and_actual_viewport() {
    let mut app = App::new(&navigation_bundle());
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let height = usize::from(app.geometry.content.height);
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert_eq!(app.session.content_scroll, height / 2);
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(app.session.content_scroll, 0);
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.session.content_scroll, height - 1);
    key(&mut app, KeyCode::Char('g'));
    assert_eq!(app.session.content_scroll, 0);
    key(&mut app, KeyCode::Char('G'));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(app.session.content_scroll > 0);
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::End);
    assert_eq!(Some(&app.selected), app.visible_navigation_indices().last());
    key(&mut app, KeyCode::Home);
    assert_eq!(
        Some(&app.selected),
        app.visible_navigation_indices().first()
    );
}

#[test]
fn hidden_or_narrow_sidebar_has_no_fake_focus_target_and_overlays_own_keys() {
    let mut app = App::new(&navigation_bundle());
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Char('B'));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert_eq!(app.focus, ReaderFocus::Content);
    assert!(!terminal.backend().to_string().contains("Tab: Outline"));
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, ReaderFocus::Content);
    app.open_search();
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Char('j'));
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert!(
        app.search.draft.is_empty(),
        "Ctrl+u edits search, not half-page scrolling"
    );
    assert_eq!(app.focus, ReaderFocus::Content);
    key(&mut app, KeyCode::Esc);
    app.open_menu(MenuId::Manual);
    let selected = app.selected;
    key(&mut app, KeyCode::Down);
    assert_eq!(app.selected, selected);
    key(&mut app, KeyCode::Esc);
    app.overlay = Overlay::Help;
    key(&mut app, KeyCode::End);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("live highlights"));
    app.overlay = Overlay::None;
    app.show_sidebar = true;
    let mut narrow = Terminal::new(TestBackend::new(20, 8)).unwrap();
    narrow.draw(|frame| app.draw(frame)).unwrap();
    assert_eq!(app.focus, ReaderFocus::Content);
    assert!(!narrow.backend().to_string().contains("Tab: Outline"));
}

#[test]
fn footer_hints_choose_whole_phrases_and_never_overwrite_position_or_notices() {
    use unicode_width::UnicodeWidthStr;
    let mut app = App::new(&navigation_bundle());
    for width in [1, 8, 20, 36, 80, 120, 180] {
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let hint = app.reading_hint(usize::from(width));
        assert!(hint.width() <= usize::from(width));
        assert!(!hint.contains("Tab:") || hint.contains("Tab: Outline"));
        if width >= 36 {
            let screen = terminal.backend().to_string();
            assert!(screen.contains("Content"));
            assert!(screen.contains("1/3"));
        }
    }
    app.report_notice("important failure".into());
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("important failure"));
    assert!(!terminal.backend().to_string().contains("j/k scroll"));
}

#[test]
fn region_titles_and_margins_focus_without_activating_nodes_or_links() {
    let mut app = App::new(&navigation_bundle());
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let selected = app.selected;
    let scroll = app.session.content_scroll;
    for (region, focus) in [
        (app.geometry.outline_region, ReaderFocus::Outline),
        (app.geometry.content_region, ReaderFocus::Content),
    ] {
        let outcome = app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: region.x,
            row: region.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(outcome, UpdateOutcome::Redraw);
        assert_eq!(app.focus, focus);
        assert_eq!(app.selected, selected);
        assert_eq!(app.session.content_scroll, scroll);
        assert!(app.take_open_request().is_none());
    }
}

#[test]
fn less_page_aliases_preserve_outline_folds_and_half_page_keys_agree() {
    let mut app = App::new(&navigation_bundle());
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let height = usize::from(app.geometry.content.height);
    let folds = app.expanded.clone();
    for code in [KeyCode::Char(' '), KeyCode::Char('f'), KeyCode::PageDown] {
        app.session.content_scroll = 0;
        key(&mut app, code);
        assert_eq!(app.session.content_scroll, height - 1);
        assert_eq!(app.expanded, folds);
        assert!(app.show_sidebar);
        key(&mut app, KeyCode::Char('b'));
        assert_eq!(app.session.content_scroll, 0);
        assert!(app.show_sidebar);
    }
    for modifiers in [KeyModifiers::NONE, KeyModifiers::CONTROL] {
        app.session.content_scroll = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), modifiers));
        assert_eq!(app.session.content_scroll, height / 2);
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), modifiers));
        assert_eq!(app.session.content_scroll, 0);
    }
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.expanded, folds,
        "Content Enter must not activate a node"
    );
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Char(' '));
    assert_ne!(app.expanded, folds);
    assert_eq!(app.session.content_scroll, 0);
    key(&mut app, KeyCode::Char('b'));
    assert!(
        app.show_sidebar,
        "lowercase b is not a sidebar toggle in Outline either"
    );
}

#[test]
fn uppercase_sidebar_binding_uses_character_not_shift_or_caps_lock_provenance() {
    use crossterm::event::KeyEventState;
    let mut app = App::new(&navigation_bundle());
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    for (modifiers, state) in [
        (KeyModifiers::NONE, KeyEventState::NONE),
        (KeyModifiers::SHIFT, KeyEventState::NONE),
        (KeyModifiers::NONE, KeyEventState::CAPS_LOCK),
    ] {
        let mut event = KeyEvent::new(KeyCode::Char('B'), modifiers);
        event.state = state;
        app.handle_key(event);
        assert!(!app.show_sidebar);
        key(&mut app, KeyCode::Char('B'));
        assert!(app.show_sidebar);
    }
    app.session.content_scroll = 10;
    let mut event = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::SHIFT);
    event.state = KeyEventState::CAPS_LOCK;
    app.handle_key(event);
    assert!(
        app.show_sidebar,
        "a lowercase character remains lowercase with Shift+Caps Lock"
    );
    assert_eq!(app.session.content_scroll, 0);
    app.open_search();
    for code in ['b', 'B', 'f', ' '] {
        key(&mut app, KeyCode::Char(code));
    }
    assert_eq!(app.search.draft, "bBf ");
    assert!(app.show_sidebar);
    key(&mut app, KeyCode::Esc);
    app.open_menu(MenuId::View);
    key(&mut app, KeyCode::Char('B'));
    assert!(
        app.show_sidebar,
        "modal menu input does not toggle the sidebar"
    );
    app.activate_menu_action(MenuAction::ToggleSidebar);
    assert!(!app.show_sidebar, "View remains an explicit alternative");
}

#[test]
fn resize_corrects_focus_before_paint_and_rejects_a_hidden_tab_target() {
    use crossterm::event::Event;
    let mut app = App::new(&navigation_bundle());
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, ReaderFocus::Outline);
    let selected = app.selected;
    let hidden_width =
        MIN_SIDEBAR_WIDTH + SIDEBAR_SPLITTER_WIDTH + crate::layout::MIN_CONTENT_WIDTH;
    app.handle_event(&Event::Resize(hidden_width, 24));
    assert_eq!(
        app.focus,
        ReaderFocus::Content,
        "no draw required for normalization"
    );
    assert!(
        app.geometry.navigation.width > 0,
        "hit geometry still belongs to the last frame"
    );
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, ReaderFocus::Content);
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.session.content_scroll, 1);
    assert_eq!(app.selected, selected);
    app.handle_event(&Event::Resize(hidden_width + 1, 24));
    assert!(
        app.outline_is_available(),
        "same visibility threshold as painting"
    );
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(app.focus, ReaderFocus::Content, "Ctrl+Tab is not plain Tab");
}

#[test]
fn direct_frame_fallback_does_not_mutate_requested_focus() {
    let mut app = App::new(&navigation_bundle());
    let mut wide = Terminal::new(TestBackend::new(120, 24)).unwrap();
    wide.draw(|frame| app.draw(frame)).unwrap();
    key(&mut app, KeyCode::Tab);
    let mut narrow = Terminal::new(TestBackend::new(20, 24)).unwrap();
    narrow.draw(|frame| app.draw(frame)).unwrap();
    assert_eq!(
        app.focus,
        ReaderFocus::Outline,
        "painting never writes requested focus"
    );
    assert_eq!(app.reading_focus(), ReaderFocus::Content);
    assert_eq!(app.focus_label(), "Content");
    assert!(!narrow.backend().to_string().contains("Tab: Outline"));
    assert!(!narrow.backend().to_string().contains("▶ OUTLINE"));
    let selected = app.selected;
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(
        app.focus,
        ReaderFocus::Content,
        "input commits the effective fallback"
    );
    assert_eq!(app.selected, selected);
    wide.draw(|frame| app.draw(frame)).unwrap();
    assert_eq!(
        app.focus,
        ReaderFocus::Content,
        "widening does not steal focus after reading"
    );
}
