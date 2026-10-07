//! Keyboard menu access, modal routing and title presentation contracts.

use super::*;
use ratatui::style::Modifier;

const MNEMONICS: [(char, MenuId); 6] = [
    ('m', MenuId::Manual),
    ('e', MenuId::Edit),
    ('v', MenuId::View),
    ('n', MenuId::Navigate),
    ('s', MenuId::Search),
    ('h', MenuId::Help),
];

#[test]
fn action_labels_do_not_use_ellipses_to_indicate_further_input() {
    use super::super::menu::menu_entries;

    for id in MenuId::ALL {
        for entry in menu_entries(id) {
            assert!(
                !entry.label.contains('…'),
                "{}: {}",
                id.label(),
                entry.label
            );
            assert!(
                !entry.label.ends_with("..."),
                "{}: {}",
                id.label(),
                entry.label
            );
        }
    }
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    let mut app = App::new(&navigation_bundle());
    for (id, label) in [
        (MenuId::Manual, "Open Document"),
        (MenuId::Navigate, "Open Reference"),
        (MenuId::Search, "Find in Page"),
        (MenuId::Search, "Find in Outline"),
    ] {
        assert!(menu_entries(id).iter().any(|entry| entry.label == label));
        app.open_menu(id);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let screen = terminal.backend().to_string();
        assert!(screen.contains(label));
        assert!(!screen.contains(&format!("{label}…")));
    }
}

#[test]
fn alt_letters_open_the_matching_menu_without_running_an_action() {
    for (letter, id) in MNEMONICS {
        for (character, modifiers) in [
            (letter, KeyModifiers::ALT),
            (letter.to_ascii_uppercase(), KeyModifiers::ALT),
            (
                letter.to_ascii_uppercase(),
                KeyModifiers::ALT | KeyModifiers::SHIFT,
            ),
        ] {
            let mut app = App::new(&navigation_bundle());
            app.selected = 1;
            app.session.content_scroll = 7;
            let selected = app.selected;
            let expanded = app.expanded.clone();
            assert_eq!(
                app.handle_key(KeyEvent::new(KeyCode::Char(character), modifiers)),
                UpdateOutcome::Redraw
            );
            assert_eq!(app.overlay, Overlay::Menu { id, cursor: 0 });
            assert_eq!(app.selected, selected);
            assert_eq!(app.session.content_scroll, 7);
            assert_eq!(app.expanded, expanded);
            assert!(!app.should_quit());
            assert!(app.take_open_request().is_none());
            assert!(app.take_copy_request().is_none());
        }
    }
}

#[test]
fn menu_letters_switch_locally_and_reject_unrelated_modifiers() {
    let mut app = App::new(&navigation_bundle());
    app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
    assert_eq!(
        app.overlay,
        Overlay::Menu {
            id: MenuId::Manual,
            cursor: 0
        }
    );
    for (letter, id) in MNEMONICS {
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT, KeyModifiers::ALT] {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            app.handle_key(KeyEvent::new(
                KeyCode::Char(letter.to_ascii_uppercase()),
                modifiers,
            ));
            assert_eq!(app.overlay, Overlay::Menu { id, cursor: 0 });
        }
    }
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::META,
        KeyModifiers::HYPER,
    ] {
        let key = KeyEvent::new(KeyCode::Char('v'), modifiers);
        assert_eq!(MenuId::from_mnemonic_key(key, false), None);
        app.handle_key(key);
        assert_eq!(
            app.overlay,
            Overlay::Menu {
                id: MenuId::Help,
                cursor: 0
            }
        );
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    assert_eq!(
        app.overlay,
        Overlay::Menu {
            id: MenuId::Help,
            cursor: 0
        }
    );
}

#[test]
fn f10_arrows_confirmation_and_dismissal_remain_keyboard_complete() {
    for confirm in [KeyCode::Enter, KeyCode::Char(' ')] {
        let mut app = App::new(&navigation_bundle());
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(
            app.overlay,
            Overlay::Menu {
                id: MenuId::Help,
                cursor: 0
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            app.overlay,
            Overlay::Menu {
                id: MenuId::Manual,
                cursor: 1
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(confirm, KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::DocumentFinder);
        assert!(!app.should_quit());
    }
    for dismiss in [KeyCode::Esc, KeyCode::F(10)] {
        let mut app = App::new(&navigation_bundle());
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::ALT));
        let selected = app.selected;
        let scroll = app.session.content_scroll;
        app.handle_key(KeyEvent::new(dismiss, KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.selected, selected);
        assert_eq!(app.session.content_scroll, scroll);
    }
}

#[test]
fn search_keeps_plain_letters_and_resumes_at_the_retained_cursor() {
    for opener in [
        KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT),
    ] {
        for dismiss in [KeyCode::Esc, KeyCode::F(10)] {
            let mut app = App::new(&navigation_bundle());
            let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
            for letter in "mevnsh".chars() {
                app.handle_key(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::NONE));
            }
            assert_eq!(app.search.draft, "mevnsh");
            assert_eq!(app.overlay, Overlay::None);
            app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
            let cursor = app.search.cursor;
            let mode = app.search.mode;
            app.handle_key(opener);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let screen = terminal.backend().to_string();
            assert!(screen.contains("←/→ menus"));
            assert!(!screen.contains("Find Page: mevnsh"));
            app.handle_key(KeyEvent::new(dismiss, KeyModifiers::NONE));
            assert_eq!(app.search.mode, mode);
            assert_eq!(app.search.cursor, cursor);
            assert_eq!(app.search.draft, "mevnsh");
            app.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
            assert_eq!(app.search.draft, "mevns!h");
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(
                terminal
                    .backend()
                    .to_string()
                    .contains("Find Page: mevns!h")
            );
        }
    }
}

#[test]
fn opening_a_menu_preserves_confirmed_search_results_and_navigation_position() {
    let mut app = App::new(&navigation_bundle());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "help".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.search.matches.len() >= 2);
    let search = app.search.clone();
    let selected = app.selected;
    let scroll = app.session.content_scroll;
    for (letter, id) in MNEMONICS {
        app.handle_key(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::ALT));
        assert_eq!(app.overlay, Overlay::Menu { id, cursor: 0 });
        terminal.draw(|frame| app.draw(frame)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.selected, selected);
        assert_eq!(app.session.content_scroll, scroll);
        assert_eq!(app.search.mode, search.mode);
        assert_eq!(app.search.query, search.query);
        assert_eq!(app.search.draft, search.draft);
        assert_eq!(app.search.cursor, search.cursor);
        assert_eq!(app.search.active_match, search.active_match);
        assert_eq!(app.search.matches.len(), search.matches.len());
        assert_eq!(app.search.scope_matches.len(), search.scope_matches.len());
    }
}

#[test]
fn menu_mnemonics_do_not_replace_help_or_finder_overlays() {
    let mut app = App::new(&navigation_bundle());
    for overlay in [Overlay::Help, Overlay::DocumentFinder] {
        app.overlay = overlay;
        for (letter, _) in MNEMONICS {
            app.handle_key(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::ALT));
            assert!(!matches!(app.overlay, Overlay::Menu { .. }));
        }
    }
    for letter in "mevnsh".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::NONE));
    }
    assert_eq!(app.finder.draft, "mevnsh");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    for (letter, _) in MNEMONICS {
        app.handle_key(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::None);
    }
}

#[test]
fn only_menu_initials_are_underlined_with_unchanged_geometry_and_colors() {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    let mut app = App::new(&navigation_bundle());
    for open in [None, Some(MenuId::View)] {
        app.overlay = open.map_or(Overlay::None, |id| Overlay::Menu { id, cursor: 0 });
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        for (_, id) in MNEMONICS {
            let label = id.label();
            let expected = format!(" {label} ");
            for (offset, character) in expected.chars().enumerate() {
                let cell = buffer
                    .cell((id.left() + u16::try_from(offset).unwrap(), 0))
                    .unwrap();
                assert_eq!(cell.symbol(), character.to_string());
                assert_eq!(cell.modifier.contains(Modifier::UNDERLINED), offset == 1);
                assert!(!cell.modifier.contains(Modifier::BOLD));
                assert_eq!(
                    cell.bg,
                    if open == Some(id) {
                        theme::SELECTED
                    } else {
                        theme::MENU
                    }
                );
                assert_eq!(
                    cell.fg,
                    if open == Some(id) {
                        theme::SELECTED_TEXT
                    } else {
                        theme::SUBTEXT_BRIGHT
                    }
                );
            }
        }
    }
}

#[test]
fn menu_hints_and_expanded_help_fit_the_available_terminal() {
    let mut app = App::new(&empty_bundle());
    app.open_menu(MenuId::Manual);
    let mut terminal = Terminal::new(TestBackend::new(24, 8)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("←→ ↑↓ Enter Esc"));

    app.overlay = Overlay::Help;
    let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen = terminal.backend().to_string();
    for hint in [
        "F10 / Alt+M/E/V/N/S/H",
        "switch menus while a menu is open",
        "←/→ menus · ↑/↓ items",
        "close menu and return",
        "q            quit",
        "Esc or ? closes this window",
    ] {
        assert!(screen.contains(hint), "missing help hint: {hint}");
    }

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen = terminal.backend().to_string();
    assert!(screen.contains("F10 / Alt+M/E/V/N/S/H"));
    assert!(screen.contains("Esc or ? closes this window"));
}

#[test]
fn narrow_menu_popups_remain_visible_and_mouse_hits_follow_their_position() {
    let mut terminal = Terminal::new(TestBackend::new(32, 12)).unwrap();
    let mut app = App::new(&navigation_bundle());
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("Keyboard Shortcuts")
    );
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.overlay, Overlay::Help);
}
