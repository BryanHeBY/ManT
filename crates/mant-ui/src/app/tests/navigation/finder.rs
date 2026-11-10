//! Document finder filtering, paging, opening, and pointer navigation.

use super::super::*;

#[test]
fn document_finder_filters_live_and_emits_an_exact_address() {
    let mut app = App::with_catalog(&navigation_bundle(), document_catalog());
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert_eq!(app.overlay, Overlay::None);
    app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert_eq!(app.overlay, Overlay::DocumentFinder);

    for character in "print".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    assert_eq!(app.finder.matches.len(), 1);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.take_open_request()
            .map(|request| request.address().clone()),
        Some(DocumentAddress::Manual {
            name: "printf".to_owned(),
            manual_section: "3".to_owned(),
        })
    );
}

#[test]
fn document_finder_tree_collapses_expands_and_opens_a_nested_document() {
    let address = DocumentAddress::Markdown {
        path: "languages/zh-CN/tool".to_owned(),
        origin: MarkdownOrigin::Documents,
    };
    let catalog = DocumentCatalog {
        schema: CatalogSchema::V0Dot14,
        query: mant_protocol::CatalogQuery::default(),
        coverage: mant_protocol::CatalogCoverage::default(),
        total: 1,
        returned: 1,
        offset: 0,
        truncated: false,
        next_offset: None,
        documents: vec![DocumentSummary {
            address: address.clone(),
        }],
    };
    let mut app = App::with_catalog(&navigation_bundle(), catalog);
    app.open_document_finder();

    let languages = app
        .finder
        .tree
        .iter()
        .position(|row| {
            matches!(row, FinderTreeRow::Folder { path, .. } if path == "documents/languages")
        })
        .expect("languages folder");
    app.finder.selected = languages;
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert!(!app.finder.expanded("documents/languages"));
    assert!(!app.finder.tree.iter().any(|row| {
        matches!(row, FinderTreeRow::Folder { path, .. } if path == "documents/languages/zh-CN")
    }));

    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert!(app.finder.expanded("documents/languages"));
    let document = app
        .finder
        .tree
        .iter()
        .position(|row| {
            matches!(
                row,
                FinderTreeRow::Document { index, .. }
                    if app.finder.catalog[*index].address == address
            )
        })
        .expect("nested document row");
    app.finder.selected = document;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.take_open_request()
            .map(|request| request.address().clone()),
        Some(address)
    );
}

#[test]
fn document_finder_is_available_from_the_manual_menu() {
    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::with_catalog(&navigation_bundle(), document_catalog());
    app.activate_menu_action(MenuAction::OpenDocument);

    terminal.draw(|frame| app.draw(frame)).expect("draw finder");
    let screen = terminal.backend().to_string();
    assert!(screen.contains("Open Document"));
    assert!(screen.contains("Start-Process"));
    assert!(screen.contains("pwsh7"));
    assert!(screen.contains("printf"));
    assert!(screen.contains("manual"));
    assert!(screen.contains('3'));
}

#[test]
fn document_finder_scrolls_with_the_wheel_and_opens_a_clicked_result() {
    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::with_catalog(&navigation_bundle(), overflowing_document_catalog());
    app.open_document_finder();
    for character in "tool-".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    terminal.draw(|frame| app.draw(frame)).expect("draw finder");

    let results = app.geometry.finder_results;
    assert!(app.geometry.finder_scrollbar.is_some());
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: results.x,
        row: results.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.finder.scroll, 3);
    assert_eq!(app.finder.selected, 3);

    let clicked_row = app.finder.scroll + 2;
    let expected = app.finder.catalog[app.finder.matches[clicked_row]]
        .address
        .clone();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: results.x,
        row: results.y + 2,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.take_open_request()
            .map(|request| request.address().clone()),
        Some(expected)
    );
}

#[test]
fn document_finder_scrollbar_track_and_drag_control_the_result_viewport() {
    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::with_catalog(&navigation_bundle(), overflowing_document_catalog());
    app.open_document_finder();
    terminal.draw(|frame| app.draw(frame)).expect("draw finder");
    let scrollbar = app.geometry.finder_scrollbar.expect("finder scrollbar");
    let area = scrollbar.area();

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.bottom() - 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.finder.scroll, scrollbar.maximum());
    assert!(matches!(
        app.pointer.drag(),
        PointerDrag::FinderScrollbar(_)
    ));

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.finder.scroll, 0);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.pointer.drag(), PointerDrag::None);
    assert_eq!(app.overlay, Overlay::DocumentFinder);
}

#[test]
fn document_finder_orders_exact_then_prefix_then_substring_matches() {
    let names = ["woman", "MANUAL", "manpath", "man", "man.conf"];
    let documents = names
        .into_iter()
        .map(|name| DocumentSummary {
            address: DocumentAddress::Manual {
                name: name.to_owned(),
                manual_section: "1".to_owned(),
            },
        })
        .collect::<Vec<_>>();
    let catalog = DocumentCatalog {
        schema: CatalogSchema::V0Dot14,
        query: mant_protocol::CatalogQuery::default(),
        coverage: mant_protocol::CatalogCoverage::default(),
        total: 5,
        returned: 5,
        offset: 0,
        truncated: false,
        next_offset: None,
        documents,
    };
    let mut app = App::with_catalog(&navigation_bundle(), catalog);
    app.open_document_finder();
    for character in "man".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    let ordered = app
        .finder
        .matches
        .iter()
        .map(|index| app.finder.catalog[*index].address.name())
        .collect::<Vec<_>>();
    assert_eq!(ordered, ["man", "man.conf", "manpath", "MANUAL", "woman"]);
}

#[test]
fn document_finder_keeps_matches_found_only_in_a_hierarchical_path() {
    let catalog = DocumentCatalog {
        schema: CatalogSchema::V0Dot14,
        query: mant_protocol::CatalogQuery::default(),
        coverage: mant_protocol::CatalogCoverage::default(),
        total: 1,
        returned: 1,
        offset: 0,
        truncated: false,
        next_offset: None,
        documents: vec![DocumentSummary {
            address: DocumentAddress::Markdown {
                path: "languages/zh-CN/tool".to_owned(),
                origin: MarkdownOrigin::Documents,
            },
        }],
    };
    let mut app = App::with_catalog(&navigation_bundle(), catalog);
    app.open_document_finder();
    for character in "zh-cn".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    assert_eq!(app.finder.matches, [0]);
}

#[test]
fn document_finder_queries_beyond_the_initial_catalog_page() {
    let initial = DocumentCatalog {
        schema: CatalogSchema::V0Dot14,
        query: mant_protocol::CatalogQuery::default(),
        coverage: mant_protocol::CatalogCoverage::default(),
        total: 20_000,
        returned: 1,
        offset: 0,
        truncated: true,
        next_offset: Some(1),
        documents: vec![DocumentSummary {
            address: DocumentAddress::Manual {
                name: ".k5identity".to_owned(),
                manual_section: "5".to_owned(),
            },
        }],
    };
    let mut app = App::with_catalog(&navigation_bundle(), initial);
    app.open_document_finder();
    assert_eq!(
        app.take_discovery_request().expect("initial query").pattern,
        None
    );

    for character in "man".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    assert_eq!(
        app.take_discovery_request().expect("live query").pattern,
        Some("man".to_owned())
    );

    app.complete_discovery(DocumentCatalog {
        schema: CatalogSchema::V0Dot14,
        query: mant_protocol::CatalogQuery::default(),
        coverage: mant_protocol::CatalogCoverage::default(),
        total: 2,
        returned: 2,
        offset: 0,
        truncated: false,
        next_offset: None,
        documents: vec![
            DocumentSummary {
                address: DocumentAddress::Manual {
                    name: "woman".to_owned(),
                    manual_section: "1".to_owned(),
                },
            },
            DocumentSummary {
                address: DocumentAddress::Manual {
                    name: "man".to_owned(),
                    manual_section: "1".to_owned(),
                },
            },
        ],
    });

    let first = app.finder.matches[0];
    assert_eq!(app.finder.catalog[first].address.name(), "man");
    assert_eq!(app.finder.total, 2);
}
