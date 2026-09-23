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
            assert_eq!(selector, mant_protocol::ContentSelector::id("options"));
            assert_eq!(format, CopyFormat::Markdown);
        }
        CopyRequest::Selection { .. } | CopyRequest::Reference { .. } => {
            panic!("semantic action emitted non-node text")
        }
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
fn fixed_node_copy_menu_routes_to_exact_visual_selection_only() {
    let fixed = serde_json::from_value(serde_json::json!({
        "surface": {
            "text": "X",
            "rows": [{"key": 1, "firstRun": 1, "runCount": 1, "columnCount": 1, "breakAfter": false}],
            "runs": [{
                "key": 1, "row": 1, "column": 0, "width": 1,
                "byteStart": 0, "byteCount": 1,
                "label": {"owner": null, "link": null, "source": null,
                          "style": {"bold": false, "underline": false}, "role": "body"}
            }]
        },
        "headings": [{
            "key": 1, "id": "test-section", "parent": null, "levelHint": 1,
            "at": {"kind": "run-boundary", "run": 1, "byte": 0},
            "title": {"parts": [{"run": 1, "startByte": 0, "endByte": 1}], "joins": []},
            "directBody": {"parts": [], "joins": []}, "source": null
        }],
        "owners": [], "links": [], "regions": [], "anchors": []
    }))
    .expect("valid final Fixed fixture");
    let bundle = ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(Document {
            parser: None,
            sources: Vec::new(),
            root_source: SourceKey::FIRST,
            body: DocumentBody::Fixed(fixed),
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
        }),
        tldr: None,
    };
    let mut app = App::new(&bundle);
    for action in [MenuAction::CopyNodeText, MenuAction::CopyNodeMarkdown] {
        app.activate_menu_action(action);
        assert!(app.take_copy_request().is_none());
        assert_eq!(
            app.notice.as_deref(),
            Some("For Fixed pages, drag across text and use Copy Selection")
        );
    }
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Section)
        .expect("Fixed heading");
    app.activate_menu_action(MenuAction::CopyNodeText);
    assert!(app.take_copy_request().is_none());
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
