//! Existing regressions grouped by layout behavior; expected values remain independent.
use super::*;

#[test]
fn settled_sidebar_resize_keeps_the_visible_code_logically_anchored() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![
        AstBlock::Paragraph {
            children: vec![Inline::Text {
                value: "A long paragraph before the example repeats enough words to wrap very differently when the content pane changes width. ".repeat(8),
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        AstBlock::Preformatted {
            children: vec![Inline::Text {
                value: "sentinel_code_block();".to_owned(),
            }],
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
    ];
    let backend = TestBackend::new(100, 12);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal
        .draw(|frame| app.draw(frame))
        .expect("initial draw");

    let initial_width = app.geometry.content.width;
    let initial_rendered = &app.session.rendered_cache[&initial_width];
    let code_row = initial_rendered.search("sentinel_code_block")[0].row;
    let logical_anchor = initial_rendered
        .viewport_anchor(code_row)
        .expect("code viewport anchor");
    app.session.content_scroll = code_row;

    let boundary = app.geometry.sidebar_splitter.x;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: boundary,
        row: 6,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 50,
            row: 6,
            modifiers: KeyModifiers::NONE,
        }),
        UpdateOutcome::Redraw
    );
    terminal
        .draw(|frame| app.draw(frame))
        .expect("resized draw");

    let resized = &app.session.rendered_cache[&app.geometry.content.width];
    assert_eq!(
        app.session.content_scroll,
        resized
            .row_for_viewport_anchor(logical_anchor)
            .expect("resized code anchor")
    );
    assert!(
        resized
            .viewport_text(app.session.content_scroll, 1, &[], None, None)
            .lines[0]
            .to_string()
            .contains("sentinel_code_block")
    );
}

#[test]
fn status_counts_nodes_visible_in_the_folded_outline() {
    let backend = TestBackend::new(80, 14);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());

    terminal
        .draw(|frame| app.draw(frame))
        .expect("initial draw");
    assert!(terminal.backend().to_string().contains("3 visible nodes"));

    app.set_selected_index(1);
    app.activate_menu_action(MenuAction::CollapseAll);
    terminal.draw(|frame| app.draw(frame)).expect("folded draw");
    let screen = terminal.backend().to_string();
    assert!(screen.contains("1 visible nodes"));
    assert_eq!(app.selected, 0, "hidden child selects its visible parent");
}

#[test]
fn preserves_the_established_menu_sidebar_and_tldr_surfaces() {
    let backend = TestBackend::new(80, 14);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&tldr_bundle());

    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer.cell((0, 0)).expect("menu cell").bg, theme::MENU);
    assert_eq!(
        buffer.cell((0, 1)).expect("sidebar cell").bg,
        theme::SIDEBAR
    );
    assert_eq!(
        buffer
            .cell((DEFAULT_SIDEBAR_WIDTH - 1, 1))
            .expect("borderless sidebar edge")
            .bg,
        theme::SIDEBAR
    );
    assert_eq!(
        buffer
            .cell((DEFAULT_SIDEBAR_WIDTH, 1))
            .expect("sidebar splitter")
            .symbol(),
        "│"
    );
    assert_eq!(
        buffer
            .cell((DEFAULT_SIDEBAR_WIDTH, 1))
            .expect("sidebar splitter background")
            .bg,
        theme::SIDEBAR
    );
    assert_eq!(
        buffer.cell((0, 5)).expect("selected tldr navigation").bg,
        theme::TLDR_SELECTED
    );
    assert_eq!(
        buffer
            .cell((DEFAULT_SIDEBAR_WIDTH + SIDEBAR_SPLITTER_WIDTH + 1, 2,))
            .expect("tldr panel border")
            .bg,
        theme::TLDR_SURFACE
    );
    let panel_right = app.geometry.content.right().saturating_sub(1);
    assert_eq!(
        buffer
            .cell((panel_right, 2))
            .expect("tldr right border")
            .symbol(),
        "┐"
    );
    assert_eq!(
        app.geometry
            .content_scrollbar
            .expect("content scrollbar")
            .area()
            .x,
        app.geometry.content.right() + CONTENT_SCROLLBAR_GAP
    );
    assert_eq!(
        buffer
            .cell((app.geometry.content.right(), 2))
            .expect("content-scrollbar gap")
            .bg,
        theme::CONTENT
    );
}

#[test]
fn default_geometry_keeps_the_established_sidebar_and_content_padding() {
    let backend = TestBackend::new(100, 14);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&tldr_bundle());

    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    assert_eq!(app.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
    assert_eq!(app.geometry.navigation.right(), DEFAULT_SIDEBAR_WIDTH);
    assert_eq!(
        app.geometry.sidebar_splitter,
        Rect::new(DEFAULT_SIDEBAR_WIDTH, 1, SIDEBAR_SPLITTER_WIDTH, 12)
    );
    assert_eq!(
        app.geometry.content.x,
        DEFAULT_SIDEBAR_WIDTH + SIDEBAR_SPLITTER_WIDTH + 1
    );
    assert_eq!(app.geometry.content.y, 2);
    let scrollbar = app.geometry.content_scrollbar.expect("content scrollbar");
    assert_eq!(
        scrollbar.area().x,
        app.geometry.content.right() + CONTENT_SCROLLBAR_GAP
    );
    assert_eq!(scrollbar.area().y, app.geometry.content.y);
}

#[test]
fn clicking_the_sidebar_selects_and_reclicking_a_branch_collapses_it() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 7,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
    let width = app.geometry.content.width;
    assert_eq!(
        app.session.content_scroll,
        app.session.rendered_cache[&width]
            .anchor_row("details")
            .expect("details anchor")
    );

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "options"
    );
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.visible_navigation_indices(), vec![0]);
}

#[test]
fn full_outline_labels_mode_wraps_every_visible_title() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0].children[0].heading =
        "A deliberately long nested section title".into();
    let backend = TestBackend::new(64, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);

    terminal
        .draw(|frame| app.draw(frame))
        .expect("compact draw");
    assert_eq!(
        app.geometry
            .navigation_rows
            .iter()
            .filter(|index| **index == 3)
            .count(),
        1
    );

    app.activate_menu_action(MenuAction::ToggleFullOutlineLabels);
    terminal.draw(|frame| app.draw(frame)).expect("full draw");
    assert!(
        app.geometry
            .navigation_rows
            .iter()
            .filter(|index| **index == 3)
            .count()
            > 1
    );
    app.open_menu(MenuId::View);
    terminal.draw(|frame| app.draw(frame)).expect("draw menu");
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("[x] Full Outline Labels")
    );
}

#[test]
fn outline_reflows_preserve_the_selected_nodes_viewport_row() {
    let backend = TestBackend::new(80, 14);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&reflow_navigation_bundle());
    app.expanded.clear();
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.id == "section-12")
        .expect("selected section");
    app.navigation_scroll = 8;

    terminal
        .draw(|frame| app.draw(frame))
        .expect("compact draw");
    let anchored_row = selected_navigation_viewport_row(&app);
    assert!(anchored_row > 0);

    for action in [MenuAction::ExpandAll, MenuAction::ToggleFullOutlineLabels] {
        app.activate_menu_action(action);
        terminal.draw(|frame| app.draw(frame)).expect("reflow draw");
        assert_eq!(selected_navigation_viewport_row(&app), anchored_row);
    }

    assert!(app.commit_sidebar_width(40));
    terminal.draw(|frame| app.draw(frame)).expect("wide draw");
    assert_eq!(selected_navigation_viewport_row(&app), anchored_row);

    for action in [MenuAction::ResetSidebar, MenuAction::CollapseAll] {
        app.activate_menu_action(action);
        terminal.draw(|frame| app.draw(frame)).expect("reflow draw");
        assert_eq!(selected_navigation_viewport_row(&app), anchored_row);
    }
}

#[test]
#[allow(clippy::too_many_lines)] // One continuous drag checks leading, throttled and final frames.
fn dragging_the_sidebar_boundary_renders_leading_throttled_and_final_widths() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let initial_render_width = app.geometry.content.width;
    let boundary = app.geometry.sidebar_splitter.x;
    let splitter_row = app.geometry.sidebar_splitter.y;
    assert_eq!(boundary, DEFAULT_SIDEBAR_WIDTH);
    assert!(!app.is_sidebar_boundary(boundary.saturating_sub(1), splitter_row));
    assert!(app.is_sidebar_boundary(boundary, splitter_row));
    assert!(!app.is_sidebar_boundary(boundary, 0));
    let started = Instant::now();

    app.handle_pointer_control_at(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: boundary,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        started,
    );
    assert_eq!(app.pointer_drag, PointerDrag::Sidebar);
    assert_eq!(
        app.handle_pointer_control_at(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 40,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            started,
        ),
        Some(UpdateOutcome::Redraw)
    );
    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw leading resize frame");
    assert_eq!(app.sidebar_width, 40);
    assert!(app.sidebar_resize.pending.is_none());
    assert_eq!(app.pointer_drag, PointerDrag::Sidebar);
    assert_ne!(app.geometry.content.width, initial_render_width);
    assert_eq!(
        app.session
            .rendered_cache
            .keys()
            .copied()
            .collect::<HashSet<_>>(),
        HashSet::from([app.geometry.content.width])
    );

    app.handle_pointer_control_at(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 44,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        started + Duration::from_millis(1),
    );
    assert_eq!(app.sidebar_width, 40);
    assert_eq!(
        app.sidebar_resize.pending.map(|pending| pending.column),
        Some(44)
    );
    let deadline = app.sidebar_resize.deadline().expect("scheduled live frame");
    app.handle_pointer_control_at(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 48,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        started + Duration::from_millis(30),
    );
    assert_eq!(
        app.sidebar_resize.pending.map(|pending| pending.column),
        Some(48)
    );
    assert_eq!(app.sidebar_resize.deadline(), Some(deadline));
    app.tick(
        deadline
            .checked_sub(Duration::from_millis(1))
            .expect("frame deadline follows the request"),
    );
    assert_eq!(app.sidebar_width, 40);
    app.tick(deadline);
    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw final live width");
    assert_eq!(app.sidebar_width, 48);
    assert_eq!(app.pointer_drag, PointerDrag::Sidebar);

    app.handle_pointer_control_at(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 46,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        started + SIDEBAR_RESIZE_FRAME_INTERVAL + Duration::from_millis(2),
    );

    assert_eq!(app.sidebar_width, 46);
    assert_eq!(app.pointer_drag, PointerDrag::None);
    assert!(app.sidebar_resize.pending.is_none());
}

#[test]
fn scheduled_sidebar_drag_requests_redraw_only_at_the_frame_deadline() {
    let mut app = App::new(&navigation_bundle());
    app.geometry.body = Rect::new(0, 1, 100, 18);
    app.geometry.sidebar_splitter = Rect::new(DEFAULT_SIDEBAR_WIDTH, 1, 1, 18);
    let started = Instant::now();
    let pointer = |kind, column| MouseEvent {
        kind,
        column,
        row: 8,
        modifiers: KeyModifiers::NONE,
    };

    assert_eq!(
        app.handle_pointer_control_at(
            pointer(
                MouseEventKind::Down(MouseButton::Left),
                DEFAULT_SIDEBAR_WIDTH,
            ),
            started,
        ),
        Some(UpdateOutcome::Unchanged)
    );
    assert_eq!(
        app.handle_pointer_control_at(
            pointer(MouseEventKind::Drag(MouseButton::Left), 44),
            started,
        ),
        Some(UpdateOutcome::Redraw)
    );
    assert_eq!(app.sidebar_width, 44);
    assert_eq!(
        app.handle_pointer_control_at(
            pointer(MouseEventKind::Drag(MouseButton::Left), 48),
            started + Duration::from_millis(1),
        ),
        Some(UpdateOutcome::Unchanged)
    );
    let deadline = app.sidebar_resize.deadline().expect("scheduled live frame");
    assert_eq!(
        app.tick(
            deadline
                .checked_sub(Duration::from_millis(1))
                .expect("frame deadline follows the request"),
        ),
        UpdateOutcome::Unchanged
    );
    assert_eq!(app.tick(deadline), UpdateOutcome::Redraw);
    assert_eq!(app.sidebar_width, 48);
}

#[test]
fn sidebar_metadata_never_clips_the_tldr_label_mid_word() {
    assert_eq!(
        sidebar_metadata(93, true, DEFAULT_SIDEBAR_WIDTH),
        " 93 outline nodes · TLDR"
    );
    assert_eq!(sidebar_metadata(93, true, 8), " TLDR");
}

#[test]
fn clicking_and_dragging_the_content_scrollbar_moves_the_document() {
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let scrollbar = app.geometry.content_scrollbar.expect("content scrollbar");
    let area = scrollbar.area();
    let maximum = scrollbar.maximum();
    assert!(area.height > 1);

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.bottom() - 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.session.content_scroll, maximum);
    assert!(matches!(app.pointer_drag, PointerDrag::ContentScrollbar(_)));

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.session.content_scroll, 0);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.pointer_drag, PointerDrag::None);
}

#[test]
fn dragging_document_text_emits_a_plain_text_copy_request() {
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
    let start_column =
        app.geometry.content.x + u16::try_from(region.start_column).expect("selection column");
    let end_column = app.geometry.content.x
        + u16::try_from(region.end_column.saturating_sub(1)).expect("selection column");
    let row = app.geometry.content.y + u16::try_from(region.row).expect("selection row");

    for (kind, column) in [
        (MouseEventKind::Down(MouseButton::Left), start_column),
        (MouseEventKind::Drag(MouseButton::Left), end_column),
        (MouseEventKind::Up(MouseButton::Left), end_column),
    ] {
        app.handle_mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }
    match app.take_copy_request().expect("copy request") {
        CopyRequest::Selection { text } => assert_eq!(text, "Show help"),
        CopyRequest::Node { .. } | CopyRequest::Reference { .. } => {
            panic!("visual selection emitted non-selection content")
        }
    }
}

#[test]
fn selection_drag_auto_scrolls_repeatedly_at_both_viewport_edges() {
    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let area = app.geometry.content;
    assert!(
        app.geometry
            .content_scrollbar
            .is_some_and(|scrollbar| scrollbar.maximum() > 2)
    );
    let started = Instant::now();
    let pointer = |kind, row| MouseEvent {
        kind,
        column: area.x + 4,
        row,
        modifiers: KeyModifiers::NONE,
    };

    app.handle_pointer_control_at(
        pointer(MouseEventKind::Down(MouseButton::Left), area.y + 1),
        started,
    );
    app.handle_pointer_control_at(
        pointer(MouseEventKind::Drag(MouseButton::Left), area.bottom() - 1),
        started,
    );
    assert_eq!(app.session.content_scroll, 1);
    let deadline = app
        .selection_auto_scroll
        .expect("scheduled downward selection scroll")
        .deadline;
    assert_eq!(deadline, started + SELECTION_AUTO_SCROLL_INTERVAL);
    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw scrolled selection");

    assert_eq!(app.tick(deadline), UpdateOutcome::Redraw);
    assert_eq!(app.session.content_scroll, 2);
    app.handle_pointer_control_at(
        pointer(MouseEventKind::Up(MouseButton::Left), area.bottom() - 1),
        deadline,
    );
    assert!(app.selection_auto_scroll.is_none());
    assert!(app.take_copy_request().is_some());

    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw before upward selection");
    let restarted = deadline + Duration::from_millis(1);
    app.handle_pointer_control_at(
        pointer(MouseEventKind::Down(MouseButton::Left), area.y + 1),
        restarted,
    );
    app.handle_pointer_control_at(
        pointer(MouseEventKind::Drag(MouseButton::Left), area.y),
        restarted,
    );
    assert_eq!(app.session.content_scroll, 1);
    assert_eq!(
        app.selection_auto_scroll.map(|scroll| scroll.direction),
        Some(-1)
    );
    app.handle_pointer_control_at(
        pointer(MouseEventKind::Drag(MouseButton::Left), area.y + 2),
        restarted + Duration::from_millis(1),
    );
    assert!(app.selection_auto_scroll.is_none());
}

#[test]
fn copy_success_toast_expires_at_its_deadline() {
    let backend = TestBackend::new(80, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    let started = Instant::now();
    app.report_copy_success_at("Copied selection".to_owned(), started);

    terminal.draw(|frame| app.draw(frame)).expect("draw toast");
    assert!(terminal.backend().to_string().contains("Copied selection"));
    assert_eq!(app.next_wakeup(started), Some(COPY_TOAST_DURATION));
    assert_eq!(
        app.tick(
            (started + COPY_TOAST_DURATION)
                .checked_sub(Duration::from_millis(1))
                .expect("toast deadline follows its start"),
        ),
        UpdateOutcome::Unchanged
    );
    assert_eq!(
        app.tick(started + COPY_TOAST_DURATION),
        UpdateOutcome::Redraw
    );
    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw expired toast");
    assert!(!terminal.backend().to_string().contains("Copied selection"));
}

#[test]
fn view_menu_is_clickable_and_toggles_the_sidebar() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: MenuId::View.left() + 1,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    terminal.draw(|frame| app.draw(frame)).expect("draw menu");
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("Reset Outline Width")
    );
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("[x] Outline Sidebar")
    );
    let buffer = terminal.backend().buffer();
    let menu_left = MenuId::View.left();
    let menu_right = menu_left + 29;
    assert_eq!(
        buffer.cell((menu_left, 1)).expect("menu left edge").bg,
        theme::SELECTED
    );
    assert_eq!(
        buffer
            .cell((menu_left, 1))
            .expect("menu left padding")
            .symbol(),
        " "
    );
    assert_eq!(
        buffer.cell((menu_right, 1)).expect("menu right edge").bg,
        theme::SELECTED
    );
    assert_eq!(
        buffer
            .cell((menu_right, 1))
            .expect("menu right padding")
            .symbol(),
        " "
    );

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: MenuId::View.left(),
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    assert!(!app.show_sidebar);
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn mouse_wheel_over_sidebar_does_not_scroll_the_document() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let content_scroll = app.session.content_scroll;

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 5,
        row: 7,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(app.navigation_scroll, 3);
    assert_eq!(app.session.content_scroll, content_scroll);
    assert!(app.navigation_sync_deadline.is_none());
}
