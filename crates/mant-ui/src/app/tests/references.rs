//! Reference activation is explicit and candidate validation is transactional.
use super::*;
use std::sync::Arc;

#[test]
fn unqualified_manual_link_preserves_manual_only_intent_for_the_host() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().unwrap().sections[0].blocks = vec![AstBlock::Paragraph {
        children: vec![Inline::Link {
            target: mant_ir::LinkTarget::Manual {
                name: "printf".into(),
                manual_section: None,
            },
            title: None,
            children: vec![Inline::Text {
                value: "printf".into(),
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let mut app = App::new(&bundle);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let region =
        app.session.rendered_cache[&app.geometry.content.width].search("printf")[0].clone();
    click_document_cell(&mut app, region.start_column, region.row);
    assert!(matches!(app.take_open_request().unwrap().document,
        mant_protocol::DocumentOpenTarget::Manual { name, manual_section: None } if name == "printf"));
    assert!(app.back_history.is_empty());
    assert_eq!(app.session.current_bundle.label, "demo");
}

#[test]
fn invalid_loaded_fragments_preserve_source_session_history_selection_and_tabs() {
    for ambiguous in [false, true] {
        let source = manual_bundle("source", "1");
        let mut target = manual_bundle("target", "1");
        if ambiguous {
            let mut duplicate = target.document.as_ref().unwrap().sections[0].clone();
            duplicate.id = "other".into();
            duplicate.fragment_aliases = vec!["shared".into()];
            let document = target.document.as_mut().unwrap();
            document.sections[0].fragment_aliases = vec!["shared".into()];
            document.sections.push(duplicate);
        }
        let mut app = App::new(&source);
        app.selected = 3;
        app.session.content_scroll = 7;
        let before = (
            app.selected,
            app.session.content_scroll,
            app.back_history.len(),
            app.forward_history.len(),
            app.document_tabs.len(),
            app.active_document_tab,
        );
        let current = Arc::clone(&app.session.current_bundle);
        app.request_open(
            target.address.clone().unwrap(),
            Some(if ambiguous { "shared" } else { "missing" }.into()),
        );
        let request = app.take_open_request().unwrap();
        app.complete_open(&target, request);
        assert!(Arc::ptr_eq(&app.session.current_bundle, &current));
        assert_eq!(
            before,
            (
                app.selected,
                app.session.content_scroll,
                app.back_history.len(),
                app.forward_history.len(),
                app.document_tabs.len(),
                app.active_document_tab,
            )
        );
        assert!(app.notice.as_ref().unwrap().contains(if ambiguous {
            "Ambiguous"
        } else {
            "No outline"
        }));
    }
}

#[test]
fn same_spelled_duplicate_targets_are_not_first_match_navigation() {
    let mut bundle = manual_bundle("duplicate", "1");
    let mut duplicate = bundle.document.as_ref().unwrap().sections[0].clone();
    duplicate.children.clear();
    bundle.document.as_mut().unwrap().sections.push(duplicate);
    let mut app = App::new(&bundle);
    app.session.content_scroll = 7;
    assert!(!app.jump_to_anchor("options"));
    assert_eq!(app.session.content_scroll, 7);
    assert!(app.notice.as_ref().unwrap().contains("Ambiguous"));
}

#[test]
fn a_real_tldr_fragment_does_not_silently_jump_to_the_quick_reference_panel() {
    let mut bundle = reference_bundle();
    bundle.tldr = tldr_bundle().tldr;
    bundle.document.as_mut().unwrap().sections[0]
        .fragment_aliases
        .push("tldr".into());
    let mut app = App::new(&bundle);
    let selected = app.selected;
    assert!(!app.jump_to_anchor("tldr"));
    assert_eq!(app.selected, selected);
    assert!(app.notice.as_ref().unwrap().contains("Ambiguous"));
}

fn reference_bundle() -> ResolvedContent {
    let mut bundle = mant_engine::query_markdown_text(
        "# Catalog\n\n## Links\n\n[ALPHA](target.md#details) and [BETA](target.md#details).\n",
        None,
    )
    .unwrap();
    bundle.address = Some(DocumentAddress::Markdown {
        path: "catalog".into(),
        origin: MarkdownOrigin::Documents,
    });
    bundle
}

fn associated_bundle() -> ResolvedContent {
    let mut bundle = mant_engine::query_markdown_text(
        "# Catalog\n\n## [First](target.md#first) and [Second](target.md#second) and [Again](target.md#first)\n\nBody.\n", None).unwrap();
    bundle.address = Some(DocumentAddress::Markdown {
        path: "catalog".into(),
        origin: MarkdownOrigin::Documents,
    });
    bundle
}

#[test]
fn associated_owner_keeps_fold_action_and_explicit_chooser_retains_every_occurrence() {
    let mut app = App::new(&associated_bundle());
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Section)
        .unwrap();
    let owner = app.session.document.navigation()[app.selected].id.clone();
    assert_eq!(
        app.session.document.reference_badges()[&owner],
        "↗ 2 targets"
    );
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.take_open_request().is_none());
    assert_eq!(app.overlay, Overlay::None);
    app.handle_key(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::SHIFT));
    assert_eq!(app.overlay, Overlay::References);
    assert_eq!(app.reference_chooser.as_ref().unwrap().choices.len(), 3);
    assert!(app.take_open_request().is_none());
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.take_open_request().unwrap().target.as_deref(),
        Some("second")
    );
    assert_eq!(app.overlay, Overlay::None);
    app.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT));
    assert!(app.take_copy_request().is_none());
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(app.take_copy_request(), Some(CopyRequest::Reference { text }) if text == "target#second")
    );
}

#[test]
fn associated_chooser_reveals_each_source_and_mouse_selection_does_not_open() {
    let mut app = App::new(&associated_bundle());
    let owner = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Section)
        .unwrap();
    app.selected = owner;
    for width in [100, 45, 80] {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        app.selected = owner;
        app.show_reference_chooser(false);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let chooser = app.reference_chooser.as_ref().unwrap();
        let second_id = chooser.choices[1].0.clone();
        let area = chooser.area;
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 1,
            row: area.y + 2,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.take_open_request().is_none());
        assert_eq!(app.reference_chooser.as_ref().unwrap().selected, 1);
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(
            app.session.content_scroll,
            app.session.rendered_cache[&app.geometry.content.width]
                .anchor_row(&second_id)
                .unwrap()
        );
        assert!(app.take_open_request().is_none());
    }
}

#[test]
fn associated_chooser_cancellation_and_unopenable_targets_leave_source_intact() {
    let mut bundle = associated_bundle();
    bundle.address = None;
    let mut app = App::new(&bundle);
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Section)
        .unwrap();
    let selected = app.selected;
    let current = Arc::clone(&app.session.current_bundle);
    app.show_reference_chooser(false);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.take_open_request().is_none());
    assert_eq!(app.selected, selected);
    app.show_reference_chooser(false);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.take_open_request().is_none());
    assert!(app.notice.as_ref().unwrap().contains("no registered"));
    assert!(Arc::ptr_eq(&app.session.current_bundle, &current));
    assert!(app.back_history.is_empty());
}

#[test]
fn reference_selection_reveals_but_only_enter_opens_and_copy_target_is_separate() {
    let bundle = reference_bundle();
    let mut app = App::new(&bundle);
    assert!(
        app.session
            .document
            .navigation()
            .iter()
            .filter(|node| node.kind == NavKind::ReferenceGroup)
            .all(|node| !app.expanded.contains(&node.id))
    );
    let index = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Reference && node.title.contains("BETA"))
        .unwrap();
    let id = app.session.document.navigation()[index].id.clone();
    app.selected = index;
    app.reveal_anchor(&id);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.scroll_to_selected();
    assert!(app.take_open_request().is_none());
    assert_eq!(
        app.session.content_scroll,
        app.session.rendered_cache[&app.geometry.content.width]
            .anchor_row(&id)
            .unwrap()
    );
    app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    assert!(app.take_open_request().is_none());
    app.copy_selected_node(CopyFormat::Text);
    assert!(app.take_copy_request().is_none());
    app.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT));
    assert!(
        matches!(app.take_copy_request(), Some(CopyRequest::Reference { text }) if text == "target#details")
    );
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let request = app.take_open_request().unwrap();
    assert_eq!(request.target.as_deref(), Some("details"));
    assert!(
        matches!(request.document, mant_protocol::DocumentOpenTarget::Address { address: DocumentAddress::Markdown { path, .. } } if path == "target")
    );
    assert!(app.back_history.is_empty());
}

#[test]
fn reference_without_namespace_stays_visible_and_cannot_invent_a_file_open() {
    let mut bundle = reference_bundle();
    bundle.address = None;
    let mut app = App::new(&bundle);
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Reference)
        .unwrap();
    app.open_selected_reference();
    assert!(app.take_open_request().is_none());
    assert!(app.notice.as_ref().unwrap().contains("no registered"));
}

#[test]
fn clicking_a_reference_only_reveals_and_resizing_keeps_its_typed_position() {
    let mut app = App::new(&reference_bundle());
    let index = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Reference && node.title.contains("BETA"))
        .unwrap();
    let id = app.session.document.navigation()[index].id.clone();
    app.reveal_anchor(&id);
    let location = app
        .session
        .document
        .reference_location(&id)
        .unwrap()
        .clone();
    for width in [100, 45, 80] {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        if app.geometry.navigation.width == 0 {
            assert!(app.geometry.navigation_rows.is_empty());
            assert_eq!(
                app.session.document.reference_location(&id),
                Some(&location)
            );
            assert!(app.take_open_request().is_none());
            continue;
        }
        let row = app
            .geometry
            .navigation_rows
            .iter()
            .position(|item| *item == index)
            .unwrap();
        app.selected = 0;
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: app.geometry.navigation.x + 3,
            row: app.geometry.navigation.y + u16::try_from(row).unwrap(),
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.selected, index);
        assert!(app.take_open_request().is_none());
        assert_eq!(
            app.session.document.reference_location(&id),
            Some(&location)
        );
        assert_eq!(
            app.session.content_scroll,
            app.session.rendered_cache[&app.geometry.content.width]
                .anchor_row(&id)
                .unwrap()
        );
    }
}

#[test]
fn valid_destination_fragment_commits_once_and_back_returns_to_reference_occurrence() {
    let bundle = reference_bundle();
    let mut app = App::new(&bundle);
    let index = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Reference && node.title.contains("BETA"))
        .unwrap();
    let id = app.session.document.navigation()[index].id.clone();
    app.selected = index;
    app.open_selected_reference();
    let request = app.take_open_request().unwrap();
    let mut target =
        mant_engine::query_markdown_text("# Target\n\n## Details\n\nTARGET BODY\n", None).unwrap();
    target.address = Some(DocumentAddress::Markdown {
        path: "target".into(),
        origin: MarkdownOrigin::Documents,
    });
    app.complete_open(&target, request);
    assert_eq!(app.back_history.len(), 1);
    assert_eq!(app.document_tabs.len(), 2);
    assert_eq!(
        app.session.document.navigation()[app.selected].target_id,
        "details"
    );
    app.navigate_history(true);
    let request = app.take_open_request().unwrap();
    assert!(request.reference);
    assert_eq!(request.target.as_deref(), Some(id.as_str()));
    app.complete_open(&bundle, request);
    assert_eq!(
        app.session.document.navigation()[app.selected].target_id,
        id
    );
    assert!(app.back_history.is_empty());
    assert_eq!(app.forward_history.len(), 1);
}
