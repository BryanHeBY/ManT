//! Search-menu settings share the active editor's keyboard context.
use super::super::search::SearchTarget;
use super::*;

fn open(app: &mut App, target: SearchTarget) {
    match target {
        SearchTarget::Page => app.open_search(),
        SearchTarget::Outline => app.open_outline_search(),
    }
}

fn input_snapshot(app: &App) -> (String, String) {
    (
        format!("{:?}", *app.search),
        format!("{:?}", app.outline_search.input),
    )
}

fn prepared_reader(current: SearchTarget, last: SearchTarget, mode: SearchMode) -> App {
    let mut app = App::new(&navigation_bundle());
    for target in [SearchTarget::Page, SearchTarget::Outline] {
        open(&mut app, target);
        for ch in if target == SearchTarget::Page {
            "help"
        } else {
            "OPTIONS"
        }
        .chars()
        {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }
    open(&mut app, current);
    for ch in "界draft".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    app.active_search_input_mut().direction = -1;
    if mode == SearchMode::Closed {
        app.close_search();
    } else {
        app.active_search_input_mut().mode = mode;
    }
    app.last_search_target = last;
    app
}

#[test]
fn menu_settings_match_shortcuts_on_both_search_surfaces() {
    for current in [SearchTarget::Page, SearchTarget::Outline] {
        for last in [SearchTarget::Page, SearchTarget::Outline] {
            for mode in [
                SearchMode::Closed,
                SearchMode::Open { editing: true },
                SearchMode::Open { editing: false },
            ] {
                for (action, shortcut) in [
                    (
                        MenuAction::SearchCase,
                        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT),
                    ),
                    (
                        MenuAction::SearchSyntax,
                        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
                    ),
                    (
                        MenuAction::SearchWrap,
                        KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT),
                    ),
                ] {
                    let mut menu = prepared_reader(current, last, mode);
                    let mut keyboard = prepared_reader(current, last, mode);
                    let target = if mode == SearchMode::Closed {
                        last
                    } else {
                        current
                    };
                    let before = input_snapshot(&menu);
                    if mode == SearchMode::Closed {
                        open(&mut keyboard, last);
                        let input = keyboard.active_search_input_mut();
                        input.draft.clone_from(&input.query);
                        input.cursor = input.draft.len();
                    }
                    keyboard.handle_key(shortcut);
                    menu.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));
                    let cursor = super::super::menu::menu_entries(MenuId::Search)
                        .iter()
                        .position(|entry| entry.action == action)
                        .unwrap();
                    for _ in 0..cursor {
                        menu.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
                    }
                    menu.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    assert_eq!(
                        menu.search_target, target,
                        "{current:?}/{last:?}/{mode:?}/{action:?}"
                    );
                    assert_eq!(menu.last_search_target, last);
                    assert_eq!(input_snapshot(&menu), input_snapshot(&keyboard));
                    let after = input_snapshot(&menu);
                    if target == SearchTarget::Page {
                        assert_eq!(after.1, before.1);
                    } else {
                        assert_eq!(after.0, before.0);
                    }
                    if mode == SearchMode::Closed {
                        assert_eq!(
                            menu.active_search_input().draft,
                            menu.active_search_input().query
                        );
                    } else {
                        assert_eq!(menu.active_search_input().draft, "界draft");
                        assert_eq!(menu.active_search_input().cursor, "界draf".len());
                    }
                }
            }
        }
    }
}
