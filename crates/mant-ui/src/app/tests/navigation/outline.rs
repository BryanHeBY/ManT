//! Outline selection, scrolling, viewport layout, and keyboard navigation.

use super::super::*;
use crate::app::focus::ReaderFocus;

#[test]
fn collapse_all_over_an_empty_navigation_does_not_panic() {
    // An empty document yields no navigation entries. Collapse All then walks
    // the (absent) selected ancestor; indexing it directly would panic, so the
    // path must tolerate a selection with nothing to resolve.
    let mut app = App::new(&empty_bundle());
    app.set_selected_index(3);
    app.activate_menu_action(MenuAction::CollapseAll);
    assert_eq!(app.session.document.navigation().len(), 0);
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
        app.pointer.drag(),
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
    assert_eq!(app.pointer.drag(), PointerDrag::None);
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
    app.pointer.restore_selection(RenderedSelection {
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
    let selection = app
        .pointer
        .selection()
        .expect("retained extended selection");
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
    for character in "Show".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    assert_eq!(app.search.matches.len(), 0);
    assert!(app.search.is_editing());

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.search.query, "Show");
    assert_eq!(app.search.matches.len(), 1);
    assert!(!app.search.is_editing());

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.search.mode, SearchMode::Closed);
    assert_eq!(app.search.query, "Show");
    assert_eq!(app.search.matches.len(), 0);
    assert_eq!(app.search.scope_matches.len(), 1);

    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.search.matches.len(), 1);
    app.refresh_search(80);
    app.tick(app.search.deadline.expect("resumable refresh"));
    assert_eq!(app.search.matches.len(), 1);

    app.open_search();
    for character in "Show".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.tick(app.search.deadline.expect("new-width search"));
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
fn keyboard_navigation_moves_from_tldr_and_markdown_overview_to_manual_sections() {
    let mut with_tldr = navigation_bundle();
    with_tldr.tldr = tldr_bundle().tldr;
    let mut app = App::new(&with_tldr);
    app.focus = ReaderFocus::Outline;
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
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![Inline::Text {
            value: "Document overview".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let mut app = App::new(&with_overview);
    app.focus = ReaderFocus::Outline;
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
