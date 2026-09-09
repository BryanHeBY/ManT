//! Existing regressions grouped by search behavior; expected values remain independent.
use super::*;

#[test]
fn confirmed_search_reports_no_matches_in_the_bottom_bar() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "missing".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    assert!(terminal.backend().to_string().contains("No matches"));
}

#[test]
fn search_input_edits_at_unicode_character_boundaries() {
    let mut app = App::new(&navigation_bundle());
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "ab界".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    assert_eq!(app.search.draft, "ac界");
    assert_eq!(app.search.cursor, 2);
}

#[test]
fn clicking_the_search_field_moves_its_unicode_aware_cursor() {
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "ab界".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    terminal.draw(|frame| app.draw(frame)).expect("draw search");

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.geometry.status.x + 8,
        row: app.geometry.status.y,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));

    assert_eq!(app.search.draft, "aXb界");
    assert_eq!(app.search.cursor, 2);
}

#[test]
fn arrows_cycle_confirmed_search_results_without_requerying() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "help".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.search.matches.len() >= 2);

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.search.active_match, 1);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.search.active_match, 0);

    app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
    assert_eq!(app.search.active_match, app.search.matches.len() - 1);
}

#[test]
fn confirmed_search_moves_across_a_pre_resolved_document_scope() {
    let scoped = |name: &str, text: &str| ResolvedContent {
        address: Some(DocumentAddress::Markdown {
            path: name.to_owned(),
            origin: MarkdownOrigin::Documents,
        }),
        label: name.to_owned(),
        document: Some(Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta {
                title: Some(name.to_owned()),
                ..DocumentMeta::default()
            },
            heading: None,
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: vec![AstBlock::Paragraph {
                children: vec![Inline::Text {
                    value: text.to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            sections: Vec::new(),
        }),
        tldr: None,
    };
    let alpha = scoped("alpha", "ordinary first document");
    let beta = scoped("beta", "unique recursive match");
    let mut app =
        App::with_catalog_and_scope(&alpha, DocumentCatalog::default(), &[alpha.clone(), beta]);
    let backend = TestBackend::new(90, 16);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| app.draw(frame)).expect("draw scope");

    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "recursive".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.search.scope_matches.len(), 1);
    assert_eq!(
        app.navigation.address().cloned(),
        Some(DocumentAddress::Markdown {
            path: "beta".to_owned(),
            origin: MarkdownOrigin::Documents,
        })
    );
    assert_eq!(app.search.matches.len(), 1);
    assert_eq!(app.navigation.history_lengths().0, 1);
}

#[test]
fn search_menu_actions_keep_confirmed_results_available() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "help".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.search.matches.len() >= 2);

    app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
    for _ in 0..4 {
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.overlay, Overlay::None);
    assert!(app.search.is_open());
    assert_eq!(app.search.active_match, app.search.matches.len() - 1);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.search.active_match, 0);
    app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
    assert_eq!(app.search.active_match, app.search.scope_matches.len() - 1);
}
