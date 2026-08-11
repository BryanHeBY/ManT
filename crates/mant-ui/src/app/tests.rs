//! End-to-end interaction and rendering regression tests for the application state machine.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ast::{
    Block as AstBlock, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole,
    DocumentMeta, DocumentSchema, DocumentSource, Inline, LayoutHint, MantDocument, Producer,
    QueryBundle, QuerySchema, Section, SourceFormat, TldrDocument, TldrOrigin,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use super::{
    App, NAVIGATION_SYNC_IDLE, Overlay, PointerDrag, SIDEBAR_RESIZE_FRAME_INTERVAL, UpdateOutcome,
    menu::{MenuAction, MenuId},
    render::sidebar_metadata,
    search::SearchMode,
};
use crate::{
    NavKind,
    layout::{CONTENT_SCROLLBAR_GAP, DEFAULT_SIDEBAR_WIDTH, SIDEBAR_SPLITTER_WIDTH},
    theme,
};

fn empty_bundle() -> QueryBundle {
    QueryBundle {
        schema: QuerySchema::V6,
        label: "demo".to_owned(),
        document: None,
        tldr: None,
    }
}

fn tldr_bundle() -> QueryBundle {
    QueryBundle {
        schema: QuerySchema::V6,
        label: "demo".to_owned(),
        document: None,
        tldr: Some(TldrDocument {
            title: "demo".to_owned(),
            description: vec!["A polished quick reference".to_owned()],
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "demo.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        }),
    }
}

fn navigation_bundle() -> QueryBundle {
    let paragraph = |value: &str| AstBlock::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    QueryBundle {
        schema: QuerySchema::V6,
        label: "demo".to_owned(),
        document: Some(MantDocument {
            schema: DocumentSchema::V6,
            producer: Producer {
                name: "mant".to_owned(),
                version: "test".to_owned(),
                engine: None,
            },
            source: DocumentSource {
                format: SourceFormat::Man,
                path: None,
            },
            meta: DocumentMeta::default(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "options".to_owned(),
                title: "OPTIONS".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![AstBlock::DefinitionList {
                    items: vec![DefinitionItem {
                        identity: Some(DefinitionIdentity {
                            id: "help-option".to_owned(),
                            role: DefinitionRole::Option,
                            case: DefinitionCase::Sensitive,
                            names: vec!["-h".to_owned(), "--help".to_owned()],
                        }),
                        terms: vec![vec![Inline::Text {
                            value: "-h, --help".to_owned(),
                        }]],
                        description: vec![paragraph("Show help")],
                        inline_term: false,
                        spacing_before_lines: None,
                    }],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: vec![Section {
                    id: "details".to_owned(),
                    title: "Details".to_owned(),
                    spacing_before_lines: 0,
                    blocks: vec![paragraph("Nested details")],
                    children: Vec::new(),
                    source: None,
                }],
                source: None,
            }],
        }),
        tldr: None,
    }
}

#[test]
fn renders_the_application_chrome_in_a_test_backend() {
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&empty_bundle());

    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let screen = terminal.backend().to_string();

    assert!(screen.contains("File"));
    assert!(screen.contains("SECTIONS"));
    assert!(screen.contains("MANUAL · demo"));
    assert!(screen.contains("0 visible sections"));
}

#[test]
fn status_counts_only_sections_visible_in_the_folded_tree() {
    let backend = TestBackend::new(80, 14);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());

    terminal
        .draw(|frame| app.draw(frame))
        .expect("initial draw");
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("2 visible sections")
    );

    app.set_selected_index(1);
    app.activate_menu_action(MenuAction::CollapseAll);
    terminal.draw(|frame| app.draw(frame)).expect("folded draw");
    let screen = terminal.backend().to_string();
    assert!(screen.contains("1 visible sections"));
    assert_eq!(app.selected, 0, "hidden child selects its visible parent");
}

#[test]
fn collapse_all_over_an_empty_navigation_does_not_panic() {
    // An empty document yields no navigation entries. Collapse All then walks
    // the (absent) selected ancestor; indexing it directly would panic, so the
    // path must tolerate a selection with nothing to resolve.
    let mut app = App::new(&empty_bundle());
    app.set_selected_index(3);
    app.activate_menu_action(MenuAction::CollapseAll);
    assert!(app.document.navigation().is_empty());
}

#[test]
fn terminal_title_includes_the_manual_section_but_the_sidebar_does_not() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("document").meta.section = Some("1".to_owned());
    let backend = TestBackend::new(80, 14);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);

    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let screen = terminal.backend().to_string();

    assert!(screen.lines().next().expect("menu row").contains("demo(1)"));
    assert!(screen.contains("MANUAL · demo"));
    assert!(!screen.contains("MANUAL · demo(1)"));
}

#[test]
fn the_final_section_heading_can_become_the_first_content_row() {
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal
        .draw(|frame| app.draw(frame))
        .expect("initial draw");
    app.set_selected_index(3);
    app.scroll_to_selected();
    let width = app.geometry.content.width;
    let expected = app.rendered_cache[&width]
        .anchor_row("details")
        .expect("details anchor");

    terminal
        .draw(|frame| app.draw(frame))
        .expect("scrolled draw");

    assert_eq!(app.content_scroll, expected);
    let row = app.geometry.content.y;
    let content = (app.geometry.content.x..app.geometry.content.right())
        .filter_map(|column| terminal.backend().buffer().cell((column, row)))
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(content.trim_start().starts_with("Details"));
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
fn help_overlay_is_safe_on_a_tiny_terminal() {
    let backend = TestBackend::new(12, 4);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&empty_bundle());
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    terminal
        .draw(|frame| app.draw(frame))
        .expect("tiny help draw");
}

#[test]
fn q_requests_a_clean_exit() {
    let mut app = App::new(&empty_bundle());
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(app.should_quit());
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
fn semantic_entries_are_revealed_only_after_their_group_expands() {
    let mut app = App::new(&navigation_bundle());

    assert_eq!(app.visible_navigation_indices(), vec![0, 1, 3]);
    app.selected = 1;
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.visible_navigation_indices(), vec![0, 1, 2, 3]);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.selected, 2);
    assert_eq!(app.document.navigation()[2].target_id, "help-option");
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
    assert_eq!(app.document.navigation()[app.selected].id, "details");
    let width = app.geometry.content.width;
    assert_eq!(
        app.content_scroll,
        app.rendered_cache[&width]
            .anchor_row("details")
            .expect("details anchor")
    );

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.document.navigation()[app.selected].id, "options");
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.visible_navigation_indices(), vec![0]);
}

#[test]
fn selected_navigation_titles_wrap_with_a_continuous_background() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0].children[0].title =
        "A deliberately long nested section title".to_owned();
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
}

#[test]
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
        app.rendered_cache.keys().copied().collect::<HashSet<_>>(),
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
    let initial_rendered = &app.rendered_cache[&initial_width];
    let code_row = initial_rendered.search("sentinel_code_block")[0].row;
    let logical_anchor = initial_rendered
        .viewport_anchor(code_row)
        .expect("code viewport anchor");
    app.content_scroll = code_row;

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

    let resized = &app.rendered_cache[&app.geometry.content.width];
    assert_eq!(
        app.content_scroll,
        resized
            .row_for_viewport_anchor(logical_anchor)
            .expect("resized code anchor")
    );
    assert!(
        resized
            .viewport_text(app.content_scroll, 1, &[], None)
            .lines[0]
            .to_string()
            .contains("sentinel_code_block")
    );
}

#[test]
fn sidebar_metadata_never_clips_the_tldr_label_mid_word() {
    assert_eq!(
        sidebar_metadata(10, 93, true, DEFAULT_SIDEBAR_WIDTH),
        " 10 top · 93 sections · TLDR"
    );
    assert_eq!(sidebar_metadata(10, 93, true, 8), " TLDR");
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
    assert_eq!(app.content_scroll, maximum);
    assert!(matches!(app.pointer_drag, PointerDrag::ContentScrollbar(_)));

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.content_scroll, 0);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.pointer_drag, PointerDrag::None);
}

#[test]
fn search_runs_only_on_confirmation_and_escape_removes_highlights() {
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
    assert!(app.search.query.is_empty());
    assert!(app.search.matches.is_empty());
}

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
fn view_menu_is_clickable_and_toggles_the_sidebar() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 7,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    terminal.draw(|frame| app.draw(frame)).expect("draw menu");
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("Reset Sidebar Width")
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
        column: 8,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    assert!(!app.show_sidebar);
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn open_menus_follow_pointer_hover_across_entries_and_menu_buttons() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    app.open_menu(MenuId::View);
    assert_eq!(
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: MenuId::View.left() + 2,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }),
        UpdateOutcome::Redraw
    );
    assert_eq!(
        app.overlay,
        Overlay::Menu {
            id: MenuId::View,
            cursor: 1,
        }
    );

    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw hovered entry");
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer
            .cell((MenuId::View.left(), 1))
            .expect("unhovered first entry")
            .bg,
        theme::BASE
    );
    assert_eq!(
        buffer
            .cell((MenuId::View.left(), 2))
            .expect("hovered second entry")
            .bg,
        theme::SELECTED
    );

    assert_eq!(
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: MenuId::Navigate.left() + 2,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        UpdateOutcome::Redraw
    );
    assert_eq!(
        app.overlay,
        Overlay::Menu {
            id: MenuId::Navigate,
            cursor: 0,
        }
    );
    terminal
        .draw(|frame| app.draw(frame))
        .expect("draw hovered menu button");
    assert!(terminal.backend().to_string().contains("Previous Section"));
}

#[test]
fn question_mark_opens_and_closes_keyboard_help() {
    let backend = TestBackend::new(100, 22);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());

    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    terminal.draw(|frame| app.draw(frame)).expect("draw help");
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("Keyboard Shortcuts")
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn content_scrolling_updates_navigation_only_after_the_idle_deadline() {
    let mut app = App::new(&navigation_bundle());
    app.geometry.content = Rect::new(0, 0, 80, 10);
    app.content_scroll = 100;
    let deadline = Instant::now() + NAVIGATION_SYNC_IDLE;
    app.navigation_sync_deadline = Some(deadline);

    app.tick(
        deadline
            .checked_sub(Duration::from_millis(1))
            .expect("deadline is in the future"),
    );
    assert_eq!(app.selected, 0);

    app.tick(deadline);
    assert_eq!(app.document.navigation()[app.selected].id, "details");
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
                    Inline::SectionReference {
                        target: "details".to_owned(),
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
    let region = app.rendered_cache[&width]
        .search("nested")
        .into_iter()
        .next()
        .expect("visible reference text");

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.geometry.content.x + u16::try_from(region.start_column).expect("link column"),
        row: app.geometry.content.y + u16::try_from(region.row).expect("link row"),
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(app.document.navigation()[app.selected].id, "details");
    assert!(app.expanded.contains("options"));
    assert_eq!(
        app.content_scroll,
        app.rendered_cache[&width]
            .anchor_row("details")
            .expect("details anchor")
    );
}

#[test]
fn keyboard_navigation_moves_from_tldr_and_markdown_overview_to_manual_sections() {
    let mut with_tldr = navigation_bundle();
    with_tldr.tldr = tldr_bundle().tldr;
    let mut app = App::new(&with_tldr);
    assert_eq!(app.document.navigation()[app.selected].kind, NavKind::Tldr);
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(app.document.navigation()[app.selected].id, "options");

    let mut with_overview = navigation_bundle();
    with_overview.document.as_mut().expect("document").blocks = vec![AstBlock::Paragraph {
        children: vec![Inline::Text {
            value: "Document overview".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let mut app = App::new(&with_overview);
    assert_eq!(app.document.navigation()[app.selected].kind, NavKind::Root);
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.document.navigation()[app.selected].id, "options");
}

#[test]
fn mouse_wheel_over_sidebar_does_not_scroll_the_document() {
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let content_scroll = app.content_scroll;

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 5,
        row: 7,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(app.navigation_scroll, 3);
    assert_eq!(app.content_scroll, content_scroll);
    assert!(app.navigation_sync_deadline.is_none());
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
    for _ in 0..3 {
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.overlay, Overlay::None);
    assert!(app.search.is_open());
    assert_eq!(app.search.active_match, app.search.matches.len() - 1);
}
