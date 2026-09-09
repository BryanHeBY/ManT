//! Existing regressions grouped by navigation behavior; expected values remain independent.
use super::*;

#[test]
fn clicking_unsafe_tldr_more_information_does_not_reach_the_host() {
    let mut bundle = tldr_bundle();
    bundle.tldr.as_mut().expect("tldr").more_information = Some("file:///etc/passwd".to_owned());
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("file:///etc/passwd")
        .into_iter()
        .next()
        .expect("visible tldr URL");

    click_document_cell(&mut app, region.start_column, region.row);

    assert!(app.take_external_request().is_none());
}

#[test]
fn clicking_safe_tldr_more_information_reaches_the_host() {
    let mut bundle = tldr_bundle();
    bundle.tldr.as_mut().expect("tldr").more_information =
        Some("https://example.test/tldr".to_owned());
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("https://example.test/tldr")
        .into_iter()
        .next()
        .expect("visible tldr URL");

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.take_external_request()
            .as_ref()
            .map(crate::ExternalUri::as_str),
        Some("https://example.test/tldr")
    );
}

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
        schema: CatalogSchema::V0Dot11,
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
    assert!(matches!(app.pointer_drag, PointerDrag::FinderScrollbar(_)));

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
    assert_eq!(app.pointer_drag, PointerDrag::None);
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
        schema: CatalogSchema::V0Dot11,
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
        schema: CatalogSchema::V0Dot11,
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
        schema: CatalogSchema::V0Dot11,
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
        schema: CatalogSchema::V0Dot11,
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

#[test]
fn cross_document_history_moves_back_and_forward_transactionally() {
    let first = manual_bundle("first", "1");
    let second = manual_bundle("second", "5");
    let mut app = App::new(&first);

    app.request_open(second.address.clone().expect("second address"), None);
    let request = app.take_open_request().expect("open second");
    app.complete_open(&second, request);
    assert_eq!(app.session.document.label(), "second");

    app.navigate_history(true);
    let request = app.take_open_request().expect("back to first");
    assert_eq!(
        request.address(),
        first.address.as_ref().expect("first address")
    );
    app.complete_open(&first, request);
    assert_eq!(app.session.document.label(), "first");

    app.navigate_history(false);
    let request = app.take_open_request().expect("forward to second");
    assert_eq!(
        request.address(),
        second.address.as_ref().expect("second address")
    );
    app.complete_open(&second, request);
    assert_eq!(app.session.document.label(), "second");
}

#[test]
fn history_restores_an_initial_direct_markdown_without_a_host_request() {
    let direct = navigation_bundle();
    let manual = manual_bundle("manual", "1");
    let mut app = App::new(&direct);

    app.request_open(manual.address.clone().expect("manual address"), None);
    let request = app.take_open_request().expect("open manual");
    app.complete_open(&manual, request);
    app.navigate_history(true);

    assert!(app.take_open_request().is_none());
    assert_eq!(app.session.document.label(), "demo");
    assert!(app.navigation.address().cloned().is_none());
}

#[test]
fn same_document_fragment_jumps_participate_in_history() {
    let bundle = manual_bundle("demo", "1");
    let address = bundle.address.clone().expect("address");
    let mut app = App::new(&bundle);
    app.geometry.content.width = 60;

    app.request_open(address, Some("details".to_owned()));
    assert_eq!(
        app.session.document.navigation()[app.selected].target_id,
        "details"
    );
    app.navigate_history(true);
    assert_eq!(
        app.session.document.navigation()[app.selected].target_id,
        "options"
    );
    app.navigate_history(false);
    assert_eq!(
        app.session.document.navigation()[app.selected].target_id,
        "details"
    );
}

#[test]
fn document_tabs_keep_first_open_order_and_reuse_existing_documents() {
    let mut app = App::new(&manual_bundle("git", "1"));
    open_manual(&mut app, "man", "1");
    app.geometry.content.width = 60;
    assert!(app.jump_to_anchor("details"));
    open_manual(&mut app, "printf", "3");

    assert_eq!(
        app.navigation
            .tabs()
            .iter()
            .map(super::super::navigation_state::DocumentTab::label)
            .collect::<Vec<_>>(),
        ["git(1)", "man(1)", "printf(3)"]
    );
    assert_eq!(app.navigation.active_tab(), 2);

    assert_eq!(app.activate_document_tab(1), UpdateOutcome::Redraw);
    let request = app.take_open_request().expect("existing tab request");
    assert_eq!(
        request.address(),
        &DocumentAddress::Manual {
            name: "man".to_owned(),
            manual_section: "1".to_owned(),
        }
    );
    assert_eq!(request.target.id(), Some("details"));
    assert_eq!(
        app.navigation.active_tab(),
        2,
        "requesting a tab does not activate it before the host succeeds"
    );
    app.complete_open(&manual_bundle("man", "1"), request);

    assert_eq!(app.navigation.active_tab(), 1);
    assert_eq!(app.navigation.tabs().len(), 3);
    assert_eq!(
        app.navigation
            .tabs()
            .iter()
            .map(super::super::navigation_state::DocumentTab::label)
            .collect::<Vec<_>>(),
        ["git(1)", "man(1)", "printf(3)"]
    );

    app.navigate_history(true);
    assert_eq!(
        app.take_open_request()
            .map(|request| request.address().clone()),
        Some(DocumentAddress::Manual {
            name: "printf".to_owned(),
            manual_section: "3".to_owned(),
        }),
        "a tab jump participates in ordinary backward history"
    );
}

#[test]
fn overflowing_document_tabs_keep_the_active_tab_visible_and_clickable() {
    let backend = TestBackend::new(86, 14);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&manual_bundle("initial-tool", "1"));
    for index in 1..6 {
        open_manual(&mut app, &format!("document-tool-{index}"), "1");
    }

    terminal.draw(|frame| app.draw(frame)).expect("draw tabs");
    assert!(
        app.geometry
            .document_tabs
            .iter()
            .any(|tab| tab.index == app.navigation.active_tab()),
        "the newly activated tab remains inside the visible window"
    );
    assert_ne!(app.geometry.previous_document_tabs, Rect::default());
    assert_eq!(app.geometry.next_document_tabs, Rect::default());
    for pair in app.geometry.document_tabs.windows(2) {
        assert!(pair[0].area.right() <= pair[1].area.x);
    }
    for tab in &app.geometry.document_tabs {
        assert!(tab.area.right() <= 86);
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((tab.area.x, tab.area.y))
                .expect("tab separator")
                .symbol(),
            "│"
        );
    }
    let active = app
        .geometry
        .document_tabs
        .iter()
        .find(|tab| tab.index == app.navigation.active_tab())
        .copied()
        .expect("active tab geometry");
    assert_eq!(
        terminal
            .backend()
            .buffer()
            .cell((active.area.x + 1, active.area.y))
            .expect("active tab cell")
            .bg,
        theme::SELECTED
    );

    let previous = app.geometry.previous_document_tabs;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: previous.x,
        row: previous.y,
        modifiers: KeyModifiers::NONE,
    });
    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw earlier tabs");
    let target = app
        .geometry
        .document_tabs
        .first()
        .copied()
        .expect("visible earlier tab");
    assert!(target.index < app.navigation.active_tab());
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: target.area.x + 1,
        row: target.area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.take_open_request()
            .map(|request| request.address().clone()),
        app.navigation.tabs()[target.index].address().cloned()
    );
}

#[test]
fn collapse_all_over_an_empty_navigation_does_not_panic() {
    // An empty document yields no navigation entries. Collapse All then walks
    // the (absent) selected ancestor; indexing it directly would panic, so the
    // path must tolerate a selection with nothing to resolve.
    let mut app = App::new(&empty_bundle());
    app.set_selected_index(3);
    app.activate_menu_action(MenuAction::CollapseAll);
    assert!(app.session.document.navigation().is_empty());
}

#[test]
fn overflowing_navigation_exposes_a_scrollbar() {
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());

    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    let scrollbar_column = app.geometry.navigation.right().saturating_sub(1);
    assert!(
        (app.geometry.navigation.y..app.geometry.navigation.bottom()).any(|row| {
            terminal
                .backend()
                .buffer()
                .cell((scrollbar_column, row))
                .is_some_and(|cell| cell.bg == theme::SCROLLBAR_THUMB)
        })
    );
    assert_eq!(app.geometry.navigation.right(), app.sidebar_width);
    assert!(
        (app.geometry.navigation.y..app.geometry.navigation.bottom()).any(|row| {
            terminal
                .backend()
                .buffer()
                .cell((scrollbar_column, row))
                .is_some_and(|cell| cell.bg == theme::SCROLLBAR_TRACK)
        })
    );
}

#[test]
fn overflowing_navigation_reserves_its_final_column_for_the_scrollbar() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0].heading =
        "A deliberately long option section ending in XYZ".into();
    for (height, reserves_gutter) in [(8, true), (14, false)] {
        let backend = TestBackend::new(80, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&bundle);
        let options = app
            .session
            .document
            .navigation()
            .iter()
            .position(|node| node.target_id == "options")
            .expect("options navigation node");
        app.selected = (0..app.session.document.navigation().len())
            .find(|index| *index != options)
            .expect("another navigation node");

        terminal.draw(|frame| app.draw(frame)).expect("draw app");

        let row = app.geometry.navigation.y
            + u16::try_from(
                app.geometry
                    .navigation_rows
                    .iter()
                    .position(|index| *index == options)
                    .expect("visible options row"),
            )
            .expect("navigation row");
        let last_column = app
            .geometry
            .navigation
            .right()
            .saturating_sub(1 + u16::from(reserves_gutter));
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((last_column, row))
                .expect("last label cell")
                .symbol(),
            "Z"
        );
        assert_eq!(app.geometry.navigation_scrollbar.is_some(), reserves_gutter);
    }
}

#[test]
fn navigation_scrollbar_click_and_drag_do_not_resize_the_sidebar() {
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let scrollbar = app
        .geometry
        .navigation_scrollbar
        .expect("navigation scrollbar");
    let area = scrollbar.area();
    let maximum = scrollbar.maximum();
    let sidebar_width = app.sidebar_width;
    assert!(area.height > 1);
    assert!(maximum > 0);

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.bottom() - 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.navigation_scroll, maximum);
    assert!(matches!(
        app.pointer_drag,
        PointerDrag::NavigationScrollbar(_)
    ));
    assert_eq!(app.sidebar_width, sidebar_width);

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.navigation_scroll, 0);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.pointer_drag, PointerDrag::None);
}

#[test]
fn selected_navigation_titles_wrap_with_a_continuous_background() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0].children[0].heading =
        "A deliberately long nested section title".into();
    let backend = TestBackend::new(64, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    app.selected = 3;

    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let buffer = terminal.backend().buffer();

    assert_eq!(app.geometry.navigation_rows[2], 3);
    assert_eq!(app.geometry.navigation_rows[3], 3);
    assert_eq!(
        buffer.cell((5, 7)).expect("first selected row").bg,
        theme::SELECTED
    );
    assert_eq!(
        buffer.cell((5, 8)).expect("wrapped selected row").bg,
        theme::SELECTED
    );
}

#[test]
fn navigation_visibility_keeps_the_complete_selected_title_on_screen() {
    let mut app = App::new(&navigation_bundle());
    app.navigation_scroll = 4;

    app.keep_selected_navigation_visible(8..11, 5);
    assert_eq!(app.navigation_scroll, 6);

    app.keep_selected_navigation_visible(2..5, 5);
    assert_eq!(app.navigation_scroll, 2);

    app.keep_selected_navigation_visible(7..14, 5);
    assert_eq!(app.navigation_scroll, 7);

    app.keep_selected_navigation_at_row(20..23, 10, 4, 50);
    assert_eq!(app.navigation_scroll, 16);

    app.keep_selected_navigation_at_row(20..28, 10, 6, 50);
    assert_eq!(app.navigation_scroll, 18, "row moves up only enough to fit");

    app.keep_selected_navigation_at_row(20..35, 10, 4, 50);
    assert_eq!(
        app.navigation_scroll, 16,
        "oversized nodes retain their first row"
    );
}

#[test]
fn shift_click_moves_the_active_endpoint_and_retains_the_true_anchor() {
    let backend = TestBackend::new(80, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("Show help")
        .into_iter()
        .next()
        .expect("visible description");
    app.selection = Some(RenderedSelection {
        anchor: TextPosition {
            row: region.row,
            column: region.start_column + 2,
        },
        focus: TextPosition {
            row: region.row,
            column: region.start_column,
        },
    });
    let column =
        app.geometry.content.x + u16::try_from(region.end_column - 1).expect("selection column");
    let row = app.geometry.content.y + u16::try_from(region.row).expect("selection row");
    let mouse = |kind| MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::SHIFT,
    };

    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left)));
    app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left)));

    let CopyRequest::Selection { text } = app.take_copy_request().expect("extended copy") else {
        panic!("visual selection emitted a semantic node");
    };
    assert_eq!(text, "ow help");
    let selection = app.selection.expect("retained extended selection");
    assert_eq!(selection.anchor.column, region.start_column + 2);
    assert_eq!(selection.focus.column, region.end_column - 1);
}

#[test]
fn closing_search_removes_highlights_but_retains_navigation() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    for character in "show".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    assert!(app.search.matches.is_empty());
    assert!(app.search.is_editing());

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.search.query, "show");
    assert_eq!(app.search.matches.len(), 1);
    assert!(!app.search.is_editing());

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.search.mode, SearchMode::Closed);
    assert_eq!(app.search.query, "show");
    assert!(app.search.matches.is_empty());
    assert_eq!(app.search.scope_matches.len(), 1);

    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert!(app.search.matches.is_empty());
    app.refresh_search(80);
    assert!(app.search.matches.is_empty());

    app.open_search();
    assert_eq!(app.search.matches.len(), 1);
}

#[test]
fn content_scrolling_updates_navigation_only_after_the_idle_deadline() {
    let mut app = App::new(&navigation_bundle());
    app.geometry.content = Rect::new(0, 0, 80, 10);
    app.session.content_scroll = 100;
    let deadline = Instant::now() + NAVIGATION_SYNC_IDLE;
    app.navigation_sync_deadline = Some(deadline);

    app.tick(
        deadline
            .checked_sub(Duration::from_millis(1))
            .expect("deadline is in the future"),
    );
    assert_eq!(app.selected, 0);

    app.tick(deadline);
    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
    assert!(app.navigation_sync_deadline.is_none());
}

#[test]
fn clicking_a_wrapped_section_reference_opens_its_target() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0]
        .blocks
        .insert(
            0,
            AstBlock::Paragraph {
                children: vec![
                    Inline::Text {
                        value: "Continue with ".to_owned(),
                    },
                    Inline::Link {
                        target: mant_ir::LinkTarget::Section {
                            id: "details".into(),
                        },
                        title: None,
                        children: vec![Inline::Text {
                            value: "the nested details section".to_owned(),
                        }],
                    },
                ],
                layout: LayoutHint::default(),
                source: None,
            },
        );
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    app.expanded.clear();
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("nested")
        .into_iter()
        .next()
        .expect("visible reference text");

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
    assert!(app.expanded.contains("options"));
    assert_eq!(
        app.session.content_scroll,
        app.session.rendered_cache[&width]
            .anchor_row("details")
            .expect("details anchor")
    );
}

#[test]
fn clicking_a_parsed_markdown_fragment_jumps_and_participates_in_history() {
    let bundle = mant_engine::query_markdown_text(
        "# Demo\n\nContinue with [the detailed section](#details).\n\n## Details\n\nDone.\n",
        Some("demo.md".to_owned()),
    )
    .expect("parse Markdown fragment link");
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("detailed")
        .into_iter()
        .next()
        .expect("visible parsed fragment link");

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
    assert_eq!(app.navigation.history_lengths().0, 1);
    app.navigate_history(true);
    assert_ne!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
    app.navigate_history(false);
    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
}

#[test]
fn missing_page_fragment_does_not_modify_history() {
    let bundle = manual_bundle("demo", "1");
    let address = bundle.address.clone().expect("address");
    let mut app = App::new(&bundle);

    app.request_open(address, Some("missing".to_owned()));

    assert_eq!(app.navigation.history_lengths().0, 0);
    assert_eq!(app.navigation.history_lengths().1, 0);
    assert_eq!(
        app.notice.as_deref(),
        Some("No outline node matches #missing")
    );
}

#[test]
fn clicking_a_relative_markdown_link_preserves_its_source_and_fragment() {
    let mut bundle = mant_engine::query_markdown_text(
        "# Index\n\nContinue with [Build](../commands/build.md#usage).\n",
        Some("/documents/guides/index.md".to_owned()),
    )
    .expect("parse relative Markdown link");
    bundle.address = Some(DocumentAddress::Markdown {
        path: "guides/index".to_owned(),
        origin: MarkdownOrigin::Source {
            name: "team".to_owned(),
        },
    });
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("Build")
        .into_iter()
        .next()
        .expect("visible Markdown link");

    click_document_cell(&mut app, region.start_column, region.row);

    let request = app.take_open_request().expect("Markdown request");
    assert_eq!(
        request.address(),
        &DocumentAddress::Markdown {
            path: "commands/build".to_owned(),
            origin: MarkdownOrigin::Source {
                name: "team".to_owned(),
            },
        }
    );
    assert_eq!(request.target.id(), Some("usage"));
}

#[test]
fn clicking_an_external_link_returns_the_uri_to_the_host() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0]
        .blocks
        .insert(
            0,
            AstBlock::Paragraph {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://example.test/docs".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "external docs".to_owned(),
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            },
        );
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("external docs")
        .into_iter()
        .next()
        .expect("visible external link");

    let column = app.geometry.content.x + u16::try_from(region.start_column).expect("link column");
    let row = app.geometry.content.y + u16::try_from(region.row).expect("link row");
    for (kind, pointer_column) in [
        (MouseEventKind::Down(MouseButton::Left), column),
        (MouseEventKind::Drag(MouseButton::Left), column + 1),
        (MouseEventKind::Up(MouseButton::Left), column),
    ] {
        app.handle_mouse(MouseEvent {
            kind,
            column: pointer_column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }
    assert!(app.take_external_request().is_none());

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.take_external_request()
            .as_ref()
            .map(crate::ExternalUri::as_str),
        Some("https://example.test/docs")
    );
}

#[test]
fn clicking_encoded_invalid_mailto_links_never_reaches_the_host() {
    for (index, uri) in [
        "mailto:%2Euser@example.test?subject=x",
        "mailto:user%2E%2Ename@example.test?subject=x",
        "mailto:user%40evil@example.test?subject=x",
        "mailto:%2Euser@example.test#fragment",
    ]
    .into_iter()
    .enumerate()
    {
        let label = format!("invalid mail target {index}");
        let mut bundle = navigation_bundle();
        bundle.document.as_mut().expect("manual").sections[0]
            .blocks
            .insert(
                0,
                AstBlock::Paragraph {
                    children: vec![Inline::Link {
                        target: mant_ir::LinkTarget::External {
                            uri: uri.to_owned(),
                        },
                        title: None,
                        children: vec![Inline::Text {
                            value: label.clone(),
                        }],
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                },
            );
        let backend = TestBackend::new(72, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&bundle);
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let width = app.geometry.content.width;
        let region = app.session.rendered_cache[&width]
            .search(&label)
            .into_iter()
            .next()
            .expect("invalid mail label remains visible");

        click_document_cell(&mut app, region.start_column, region.row);

        assert!(
            app.take_external_request().is_none(),
            "unsafe mailto URI reached host activation: {uri}"
        );
    }
}

#[test]
fn keyboard_navigation_moves_from_tldr_and_markdown_overview_to_manual_sections() {
    let mut with_tldr = navigation_bundle();
    with_tldr.tldr = tldr_bundle().tldr;
    let mut app = App::new(&with_tldr);
    assert_eq!(
        app.session.document.navigation()[app.selected].kind,
        NavKind::Tldr
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "options"
    );

    let mut with_overview = navigation_bundle();
    with_overview.document.as_mut().expect("document").blocks = vec![AstBlock::Paragraph {
        children: vec![Inline::Text {
            value: "Document overview".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let mut app = App::new(&with_overview);
    assert_eq!(
        app.session.document.navigation()[app.selected].kind,
        NavKind::Root
    );
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "options"
    );
}

#[test]
fn terminal_title_includes_the_manual_section_but_the_sidebar_does_not() {
    let mut bundle = navigation_bundle();
    bundle
        .document
        .as_mut()
        .expect("document")
        .meta
        .manual_section = Some("1".to_owned());
    let backend = TestBackend::new(80, 14);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);

    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let screen = terminal.backend().to_string();

    assert!(screen.lines().next().expect("menu row").contains("demo(1)"));
    assert!(screen.contains("MANUAL · demo"));
    assert!(!screen.contains("MANUAL · demo(1)"));
}
