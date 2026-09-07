//! Existing regressions grouped by entries behavior; expected values remain independent.
use super::*;

#[test]
fn semantic_entries_are_revealed_only_after_their_group_expands() {
    let mut app = App::new(&navigation_bundle());

    assert_eq!(app.visible_navigation_indices(), vec![0, 1, 3]);
    app.selected = 1;
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.visible_navigation_indices(), vec![0, 1, 2, 3]);
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.selected, 2);
    assert_eq!(
        app.session.document.navigation()[2].target_id,
        "help-option"
    );
}

#[test]
fn edit_actions_copy_complete_semantic_nodes_only() {
    let mut app = App::new(&navigation_bundle());

    app.activate_menu_action(MenuAction::CopyNodeMarkdown);
    match app.take_copy_request().expect("semantic copy request") {
        CopyRequest::Node {
            selector, format, ..
        } => {
            assert_eq!(selector, "options");
            assert_eq!(format, CopyFormat::Markdown);
        }
        CopyRequest::Selection { .. } => panic!("semantic action emitted visual text"),
    }

    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::EntryGroup)
        .expect("entry group");
    app.activate_menu_action(MenuAction::CopyNodeText);
    assert!(app.take_copy_request().is_none());
    assert_eq!(
        app.notice.as_deref(),
        Some("Select a complete document node before copying")
    );
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
    assert!(terminal.backend().to_string().contains("Previous Node"));
}
