//! Existing regressions grouped by layout behavior; expected values remain independent.
use super::*;

#[test]
fn fixed_surface_uses_real_buffer_for_horizontal_reveal_search_and_link_hit() {
    let bundle = fixed_bundle();
    let document = bundle.document.as_ref().expect("fixed document");
    assert!(mant_ir::validate_document(document).is_empty());
    let mut terminal = Terminal::new(TestBackend::new(30, 12)).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal
        .draw(|frame| app.draw(frame))
        .expect("initial draw");
    let area = app.geometry.content;
    assert!(area.width < 36);
    let visible = |terminal: &Terminal<TestBackend>, area: Rect| {
        (area.y..area.bottom())
            .map(|row| {
                (area.x..area.right())
                    .map(|column| terminal.backend().buffer()[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(!visible(&terminal, area).contains("LINK"));
    assert_eq!(app.session.document.max_fixed_columns(), 37);
    let clipped = app.session.document.render_with_horizontal_offset(4, 31);
    let clipped_row = (0..clipped.row_count)
        .find(|row| clipped.is_fixed_row(*row))
        .expect("fixed row");
    assert_eq!(
        clipped.fixed_line_row(mant_ir::FixedLineKey::FIRST),
        Some(clipped_row)
    );
    assert_eq!(
        clipped.fixed_line_row(mant_ir::FixedLineKey::new(2).unwrap()),
        Some(clipped_row + 1)
    );
    assert_eq!(
        clipped.point_location(mant_ir::ContentPointKey::FIRST),
        Some((clipped_row, 36))
    );
    assert_eq!(clipped.text.lines[clipped_row].to_string(), " LIN");

    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
    assert_eq!(app.session.horizontal_offset, 0);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!(app.session.horizontal_offset, 4);
    terminal
        .draw(|frame| app.draw(frame))
        .expect("horizontal draw");
    assert!(!visible(&terminal, area).contains("LINK"));

    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "LINK".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.search.scope_matches.len(), 1);
    assert!(app.session.horizontal_offset > 4);
    terminal.draw(|frame| app.draw(frame)).expect("reveal draw");
    assert!(visible(&terminal, area).contains("LINK"));

    let width = app.geometry.content.width;
    let rendered = &app.session.rendered_cache[&width];
    let found = &app.search.scope_matches[0].rendered;
    assert!(rendered.search("│").is_empty());
    assert_eq!(rendered.anchor_row("fixed-end"), Some(found.row));
    let column = found.start_column - app.session.horizontal_offset;
    assert!(rendered.link_identity_at(found.row, column).is_some());
    assert!(matches!(
        app.session
            .document
            .link_target_at(rendered, found.row, column),
        Some(crate::document::LinkTarget::External(_))
    ));
    let viewport_row = found.row.saturating_sub(app.session.content_scroll);
    let cell = terminal
        .backend()
        .buffer()
        .cell((
            area.x + u16::try_from(column).unwrap(),
            area.y + u16::try_from(viewport_row).unwrap(),
        ))
        .expect("visible linked fixed cell");
    assert!(
        cell.modifier
            .contains(ratatui::style::Modifier::BOLD | ratatui::style::Modifier::UNDERLINED)
    );
    let selection = RenderedSelection::new(TextPosition {
        row: found.row,
        column,
    });
    assert_eq!(
        app.session.document.selected_text(
            rendered,
            RenderedSelection {
                anchor: selection.anchor,
                focus: TextPosition {
                    row: found.row,
                    column: column + 3,
                },
            }
        ),
        "LINK"
    );
}

#[test]
fn multirow_fixed_selection_copies_complete_intermediate_physical_rows() {
    use mant_ir::{Decoration, DecorationKey, DecorationKind, FixedLine, FixedLineKey, Provenance};

    let mut bundle = fixed_bundle();
    let store = &mut bundle.document.as_mut().unwrap().content_store;
    store.fixed_views[0].lines.push(FixedLine {
        key: FixedLineKey::new(3).unwrap(),
        terminal_columns: 37,
        placements: Vec::new(),
        decorations: vec![Decoration {
            key: DecorationKey::new(4).unwrap(),
            text: "─".repeat(37),
            start_column: 0,
            width_columns: 37,
            kind: DecorationKind::Rule,
            provenance: Provenance::Generated { trigger: None },
        }],
    });
    mant_ir::validate_content_store(store).unwrap();
    let view = crate::document::DocumentView::new(&bundle);
    let rendered = view.render_with_horizontal_offset(4, 31);
    let rows = (0..rendered.row_count)
        .filter(|row| rendered.is_fixed_row(*row))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    let copied = view.selected_text(
        &rendered,
        RenderedSelection {
            anchor: TextPosition {
                row: rows[0],
                column: 1,
            },
            focus: TextPosition {
                row: rows[2],
                column: 2,
            },
        },
    );
    let lines = copied.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "LINK│");
    assert_eq!(lines[1], format!("{}LINK│", " ".repeat(32)));
    assert_eq!(lines[2], "─".repeat(34));
}

#[test]
fn fixed_point_and_rule_only_line_reveal_on_the_real_narrow_terminal_buffer() {
    use mant_ir::{Decoration, DecorationKey, DecorationKind, FixedLine, FixedLineKey, Provenance};

    let mut bundle = fixed_bundle();
    let store = &mut bundle.document.as_mut().unwrap().content_store;
    // CVS tbl_term.c::term_tbl emits whole-row rules without visiting data
    // cells, so the line key must reveal the physical row on its own.
    store.fixed_views[0].lines.push(FixedLine {
        key: FixedLineKey::new(3).unwrap(),
        terminal_columns: 37,
        placements: Vec::new(),
        decorations: vec![Decoration {
            key: DecorationKey::new(4).unwrap(),
            text: "─".repeat(37),
            start_column: 0,
            width_columns: 37,
            kind: DecorationKind::Rule,
            provenance: Provenance::Generated { trigger: None },
        }],
    });
    assert!(mant_ir::validate_document(bundle.document.as_ref().unwrap()).is_empty());

    let mut app = App::new(&bundle);
    let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let area = app.geometry.content;
    assert!(usize::from(area.width) < 37);
    let rendered = &app.session.rendered_cache[&area.width];
    let (point_row, point_column) = rendered
        .point_location(mant_ir::ContentPointKey::FIRST)
        .expect("zero-width fixed point");
    assert_eq!(point_column, 36);
    let rule_row = rendered
        .fixed_line_row(FixedLineKey::new(3).unwrap())
        .expect("rule-only physical line");
    assert!(rule_row > point_row);

    // The typed point reveals an offscreen native column without inventing a
    // text anchor. Horizontal movement uses the un-clipped native coordinate.
    app.session.content_scroll = point_row;
    app.session.horizontal_offset = point_column.saturating_sub(3);
    app.session.rendered_cache.clear();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let point_column_on_screen = point_column - app.session.horizontal_offset;
    assert_eq!(
        terminal.backend().buffer()[(
            area.x + u16::try_from(point_column_on_screen).unwrap(),
            area.y
        )]
            .symbol(),
        "│"
    );

    // A rule line has no content point or cell, but its fixed-line key still
    // lands on the exact row and survives the same clipped Buffer rendering.
    app.session.content_scroll = rule_row;
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert_eq!(terminal.backend().buffer()[(area.x, area.y)].symbol(), "─");
}

#[test]
fn nested_fixed_display_in_table_cell_keeps_line_and_point_locations() {
    use mant_ir::{TableCell, TableRow, TableRowKind};

    // CVS tbl_term.c::tbl_data can print T{...T} cell content containing a
    // no-fill display; the cell wrapper must not discard fixed identities.
    let mut bundle = fixed_bundle();
    let document = bundle.document.as_mut().unwrap();
    let fixed = document.blocks.remove(0);
    document.content_store.owners[0].kind = mant_ir::ContentOwnerKind::TableCell;
    document.blocks = vec![AstBlock::Table {
        fixed_view: None,
        rows: vec![TableRow {
            kind: TableRowKind::Data,
            cells: vec![
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    point: None,
                    blocks: vec![fixed],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
                },
                TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    point: None,
                    blocks: Vec::new(),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
                },
            ],
        }],
        layout: mant_ir::LayoutHint::default(),
        source: None,
    }];
    assert!(mant_ir::validate_document(document).is_empty());
    let view = crate::document::DocumentView::new(&bundle);
    for width in [80, 10] {
        let rendered = view.render(width);
        let first = rendered
            .fixed_line_row(mant_ir::FixedLineKey::FIRST)
            .expect("first nested fixed line");
        assert_eq!(
            rendered.fixed_line_row(mant_ir::FixedLineKey::new(2).unwrap()),
            Some(first + 1)
        );
        let (row, column) = rendered
            .point_location(mant_ir::ContentPointKey::FIRST)
            .expect("nested fixed point");
        assert_eq!(row, first);
        assert!(column >= 36);
    }
}

#[test]
fn successful_page_change_cancels_splitter_timer_but_failed_candidate_retains_it() {
    use crate::app::{HistoryDirection, LocalTarget};
    use std::sync::Arc;
    for succeeds in [false, true] {
        let mut app = App::new(&navigation_bundle());
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let now = Instant::now();
        let mouse = |kind, column| MouseEvent {
            kind,
            column,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_pointer_control_at(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                app.geometry.sidebar_splitter.x,
            ),
            now,
        );
        app.handle_pointer_control_at(mouse(MouseEventKind::Drag(MouseButton::Left), 40), now);
        app.handle_pointer_control_at(
            mouse(MouseEventKind::Drag(MouseButton::Left), 44),
            now + Duration::from_millis(1),
        );
        let deadline = app.pointer.resize_deadline().expect("queued resize");
        assert_eq!(app.sidebar_width, 40);
        let old = Arc::clone(&app.session.current_bundle);
        let candidate = Arc::new(manual_bundle("replacement", "1"));
        let target = if succeeds {
            LocalTarget::Default
        } else {
            LocalTarget::Fragment("absent-target".into())
        };
        app.complete_loaded_navigation(Arc::clone(&candidate), target, HistoryDirection::New);
        if succeeds {
            assert!(Arc::ptr_eq(&app.session.current_bundle, &candidate));
            assert_eq!(app.pointer.drag(), PointerDrag::None);
            assert!(app.pointer.resize_deadline().is_none());
        } else {
            assert!(Arc::ptr_eq(&app.session.current_bundle, &old));
            assert_eq!(app.pointer.drag(), PointerDrag::Sidebar);
            assert_eq!(app.pointer.resize_deadline(), Some(deadline));
        }
        app.tick(deadline);
        assert_eq!(app.sidebar_width, if succeeds { 40 } else { 44 });
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert_eq!(app.geometry.content.width, app.session.content_render_width);
        assert!(
            app.session
                .rendered_cache
                .contains_key(&app.geometry.content.width)
        );
    }
}

#[test]
fn settled_sidebar_resize_keeps_the_visible_code_logically_anchored() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![
        AstBlock::Paragraph {
            children: vec![crate::test_content::text("A long paragraph before the example repeats enough words to wrap very differently when the content pane changes width. ".repeat(8))],
            layout: LayoutHint::default(),
            source: None,
        },
        AstBlock::Preformatted {
            children: vec![crate::test_content::text("sentinel_code_block();".to_owned())],
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
        crate::test_content::heading("A deliberately long nested section title");
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
    assert_eq!(app.pointer.drag(), PointerDrag::Sidebar);
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
    assert!(app.pointer.pending_resize_column().is_none());
    assert_eq!(app.pointer.drag(), PointerDrag::Sidebar);
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
    assert_eq!(app.pointer.pending_resize_column(), Some(44));
    let deadline = app.pointer.resize_deadline().expect("scheduled live frame");
    app.handle_pointer_control_at(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 48,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        started + Duration::from_millis(30),
    );
    assert_eq!(app.pointer.pending_resize_column(), Some(48));
    assert_eq!(app.pointer.resize_deadline(), Some(deadline));
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
    assert_eq!(app.pointer.drag(), PointerDrag::Sidebar);

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
    assert_eq!(app.pointer.drag(), PointerDrag::None);
    assert!(app.pointer.pending_resize_column().is_none());
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
    let deadline = app.pointer.resize_deadline().expect("scheduled live frame");
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
    assert!(matches!(
        app.pointer.drag(),
        PointerDrag::ContentScrollbar(_)
    ));

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
    assert_eq!(app.pointer.drag(), PointerDrag::None);
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
        .pointer
        .selection_scroll()
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
    assert!(app.pointer.selection_scroll().is_none());
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
        app.pointer
            .selection_scroll()
            .map(|scroll| scroll.direction),
        Some(-1)
    );
    app.handle_pointer_control_at(
        pointer(MouseEventKind::Drag(MouseButton::Left), area.y + 2),
        restarted + Duration::from_millis(1),
    );
    assert!(app.pointer.selection_scroll().is_none());
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
