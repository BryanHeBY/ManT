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
    assert_eq!(app.navigation.history_lengths().0, 0);
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
            app.navigation.history_lengths().0,
            app.navigation.history_lengths().1,
            app.navigation.tabs().len(),
            app.navigation.active_tab(),
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
                app.navigation.history_lengths().0,
                app.navigation.history_lengths().1,
                app.navigation.tabs().len(),
                app.navigation.active_tab(),
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
    let mut bundle = mant_loader::load_markdown_text(
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
    let mut bundle = mant_loader::load_markdown_text(
        "# Catalog\n\n## [First](target.md#first) and [Second](target.md#second) and [Again](target.md#first)\n\nBody.\n", None).unwrap();
    bundle.address = Some(DocumentAddress::Markdown {
        path: "catalog".into(),
        origin: MarkdownOrigin::Documents,
    });
    bundle
}

#[test]
fn direct_and_picker_copy_keep_encoded_native_topics_distinct_from_sections() {
    for heading in [false, true] {
        let source = if heading {
            "# Catalog\n\n## [Native](man:demo%281%29)\n"
        } else {
            "# Catalog\n\n## Native\n\n[Native](man:demo%281%29)\n"
        };
        let bundle = mant_loader::load_markdown_text(source, None).unwrap();
        let mut app = App::new(&bundle);
        app.selected = app
            .session
            .document
            .navigation()
            .iter()
            .position(|node| {
                node.kind
                    == if heading {
                        NavKind::Section
                    } else {
                        NavKind::Reference
                    }
            })
            .unwrap();
        app.copy_selected_reference();
        if heading {
            assert!(matches!(app.overlay, Overlay::References(_)));
            let choice = &app.overlay.references().unwrap().choices()[0].1;
            assert!(
                choice.contains("man:demo(1)"),
                "picker remains readable: {choice}"
            );
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        let Some(CopyRequest::Reference { text }) = app.take_copy_request() else {
            panic!("explicit copy must provide a reusable URI");
        };
        assert_eq!(text, "man:demo%281%29");
        assert_eq!(
            mant_ir::LinkTarget::from_uri(&text),
            mant_ir::LinkTarget::Manual {
                name: "demo(1)".into(),
                manual_section: None,
            }
        );
        assert!(app.take_open_request().is_none());
    }
}

#[test]
fn malformed_reference_copy_reports_failure_without_repairing_the_address() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().unwrap().sections[0].blocks = vec![AstBlock::Paragraph {
        children: vec![Inline::Link {
            target: mant_ir::LinkTarget::Manual {
                name: "bad\nname".into(),
                manual_section: None,
            },
            title: None,
            children: vec![Inline::Text {
                value: "BAD".into(),
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let mut app = App::new(&bundle);
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Reference)
        .unwrap();
    app.copy_selected_reference();
    assert!(app.take_copy_request().is_none());
    assert!(
        app.notice
            .as_deref()
            .unwrap()
            .contains("no reusable link address")
    );
    app.show_reference_chooser(super::super::references::ReferencePurpose::Copy);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.take_copy_request().is_none());
    assert!(
        app.notice
            .as_deref()
            .unwrap()
            .contains("no reusable link address")
    );
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
    assert!(matches!(app.overlay, Overlay::References(_)));
    assert_eq!(app.overlay.references().unwrap().choices().len(), 3);
    assert!(app.take_open_request().is_none());
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.take_open_request().unwrap().target.id(), Some("second"));
    assert_eq!(app.overlay, Overlay::None);
    app.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT));
    assert!(app.take_copy_request().is_none());
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(app.take_copy_request(), Some(CopyRequest::Reference { text }) if text == "target.md#second")
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
        app.show_reference_chooser(super::super::references::ReferencePurpose::Open);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let chooser = app.overlay.references().unwrap();
        let second_id = chooser.choices()[1].0.clone();
        let area = chooser.area();
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 1,
            row: area.y + 2,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.take_open_request().is_none());
        assert_eq!(app.overlay.references().unwrap().selected(), 1);
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
    app.show_reference_chooser(super::super::references::ReferencePurpose::Open);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.take_open_request().is_none());
    assert_eq!(app.selected, selected);
    app.show_reference_chooser(super::super::references::ReferencePurpose::Open);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.take_open_request().is_none());
    assert!(app.notice.as_ref().unwrap().contains("no registered"));
    assert!(Arc::ptr_eq(&app.session.current_bundle, &current));
    assert_eq!(app.navigation.history_lengths().0, 0);
}

#[test]
fn reference_chooser_renders_and_scrolls_whole_graphemes_in_narrow_terminals() {
    let mut app = App::new(&associated_bundle());
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Section)
        .unwrap();
    for width in [8, 10, 12, 20, 80] {
        let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
        app.show_reference_chooser(super::super::references::ReferencePurpose::Open);
        app.overlay
            .references_mut()
            .unwrap()
            .set_label(0, "Cafe\u{301} 👩‍💻".into());
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let area = app.overlay.references().unwrap().area();
        let symbols: Vec<_> = (area.x + 1..area.right() - 1)
            .map(|x| terminal.backend().buffer()[(x, area.y + 1)].symbol())
            .collect();
        for symbol in &symbols {
            if symbol.contains('\u{301}') {
                assert_eq!(*symbol, "e\u{301}");
            }
            if symbol.contains(['👩', '💻', '\u{200d}']) {
                assert_eq!(*symbol, "👩‍💻");
            }
        }
        if width >= 20 {
            assert!(symbols.contains(&"e\u{301}"));
            assert!(symbols.contains(&"👩‍💻"));
        }

        // Twelve graphemes end after the complete e + accent. Scalar-based
        // scrolling used to start on the accent instead of the emoji.
        app.overlay
            .references_mut()
            .unwrap()
            .set_label(0, "12345678901e\u{301}👩‍💻TAIL".into());
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert_eq!(
            terminal.backend().buffer()[(area.x + 3, area.y + 1)].symbol(),
            "👩‍💻"
        );
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert_eq!(
            terminal.backend().buffer()[(area.x + 3, area.y + 1)].symbol(),
            "1"
        );
    }
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
        matches!(app.take_copy_request(), Some(CopyRequest::Reference { text }) if text == "target.md#details")
    );
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let request = app.take_open_request().unwrap();
    assert_eq!(request.target.id(), Some("details"));
    assert!(
        matches!(request.document, mant_protocol::DocumentOpenTarget::Address { address: DocumentAddress::Markdown { path, .. } } if path == "target")
    );
    assert_eq!(app.navigation.history_lengths().0, 0);
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
        mant_loader::load_markdown_text("# Target\n\n## Details\n\nTARGET BODY\n", None).unwrap();
    target.address = Some(DocumentAddress::Markdown {
        path: "target".into(),
        origin: MarkdownOrigin::Documents,
    });
    app.complete_open(&target, request);
    assert_eq!(app.navigation.history_lengths().0, 1);
    assert_eq!(app.navigation.tabs().len(), 2);
    assert_eq!(
        app.session.document.navigation()[app.selected].target_id,
        "details"
    );
    app.navigate_history(true);
    let request = app.take_open_request().unwrap();
    assert!(matches!(
        request.target,
        super::super::LocalTarget::ReferenceOccurrence(_)
    ));
    assert_eq!(request.target.id(), Some(id.as_str()));
    app.complete_open(&bundle, request);
    assert_eq!(
        app.session.document.navigation()[app.selected].target_id,
        id
    );
    assert_eq!(app.navigation.history_lengths().0, 0);
    assert_eq!(app.navigation.history_lengths().1, 1);
}

#[test]
fn failed_history_and_tab_reloads_preserve_typed_targets_and_the_current_page() {
    for reference in [false, true] {
        let source = reference_bundle();
        let mut app = App::new(&source);
        app.selected = app
            .session
            .document
            .navigation()
            .iter()
            .position(|node| {
                if reference {
                    node.kind == NavKind::Reference && node.title.contains("BETA")
                } else {
                    node.kind == NavKind::Section && node.id == "links"
                }
            })
            .unwrap();
        let expected = app.current_local_target();
        assert_eq!(
            matches!(&expected, super::super::LocalTarget::ReferenceOccurrence(_)),
            reference
        );
        open_manual(&mut app, "destination", "1");
        let current = Arc::clone(&app.session.current_bundle);
        let before = (
            app.navigation.history_lengths(),
            app.navigation.active_tab(),
            app.navigation.tabs().len(),
            app.selected,
            app.session.content_scroll,
        );
        let mut missing = empty_bundle();
        missing.address.clone_from(&source.address);

        app.navigate_history(true);
        let request = app.take_open_request().unwrap();
        assert_eq!(request.target, expected);
        app.complete_open(&missing, request);
        assert!(Arc::ptr_eq(&current, &app.session.current_bundle));
        assert_eq!(
            before,
            (
                app.navigation.history_lengths(),
                app.navigation.active_tab(),
                app.navigation.tabs().len(),
                app.selected,
                app.session.content_scroll
            )
        );
        assert!(app.notice.is_some());

        app.activate_document_tab(0);
        let request = app.take_open_request().unwrap();
        assert_eq!(request.target, expected);
        app.complete_open(&missing, request);
        assert!(Arc::ptr_eq(&current, &app.session.current_bundle));
        assert_eq!(
            before,
            (
                app.navigation.history_lengths(),
                app.navigation.active_tab(),
                app.navigation.tabs().len(),
                app.selected,
                app.session.content_scroll
            )
        );

        // The failed back request is still retryable and commits exactly once.
        app.navigate_history(true);
        let request = app.take_open_request().unwrap();
        assert_eq!(request.target, expected);
        app.complete_open(&source, request);
        assert_eq!(app.navigation.history_lengths(), (0, 1));
        assert_eq!(app.navigation.active_tab(), 0);
        assert_eq!(app.current_local_target(), expected);
    }
}

#[test]
fn chooser_keyboard_and_footer_use_the_same_open_copy_and_reveal_actions() {
    use super::super::references::ReferencePurpose;
    for purpose in [ReferencePurpose::Open, ReferencePurpose::Copy] {
        for reveal in [false, true] {
            for mouse in [false, true] {
                let mut app = App::new(&associated_bundle());
                let owner = app
                    .session
                    .document
                    .navigation()
                    .iter()
                    .position(|node| node.kind == NavKind::Section)
                    .unwrap();
                app.selected = owner;
                let current = Arc::clone(&app.session.current_bundle);
                app.show_reference_chooser(purpose);
                let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let chooser = app.overlay.references().unwrap();
                let id = chooser.choices()[0].0.clone();
                let area = chooser.area();
                if mouse {
                    app.handle_mouse(MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: area.x + if reveal { area.width / 2 + 1 } else { 1 },
                        row: area.bottom() - 2,
                        modifiers: KeyModifiers::NONE,
                    });
                } else {
                    app.handle_key(KeyEvent::new(
                        if reveal {
                            KeyCode::Char('r')
                        } else {
                            KeyCode::Enter
                        },
                        KeyModifiers::NONE,
                    ));
                }
                assert_eq!(app.overlay, Overlay::None);
                assert!(Arc::ptr_eq(&app.session.current_bundle, &current));
                if reveal {
                    assert_eq!(app.selected, owner);
                    assert_eq!(
                        app.session.content_scroll,
                        app.session.rendered_cache[&app.geometry.content.width]
                            .anchor_row(&id)
                            .unwrap()
                    );
                    assert!(app.take_open_request().is_none());
                    assert!(app.take_copy_request().is_none());
                } else if purpose == ReferencePurpose::Copy {
                    let Some(CopyRequest::Reference { text }) = app.take_copy_request() else {
                        panic!("copy action")
                    };
                    assert_eq!(text, "target.md#first");
                    assert!(app.take_open_request().is_none());
                } else {
                    let request = app.take_open_request().expect("open action");
                    assert_eq!(request.target.id(), Some("first"));
                    assert!(app.take_copy_request().is_none());
                }
            }
        }
    }
}

#[test]
fn chooser_dismissal_or_replacement_drops_its_data_without_click_through() {
    use super::super::references::ReferencePurpose;
    for purpose in [ReferencePurpose::Open, ReferencePurpose::Copy] {
        let mut app = App::new(&associated_bundle());
        let owner = app
            .session
            .document
            .navigation()
            .iter()
            .position(|node| node.kind == NavKind::Section)
            .unwrap();
        app.selected = owner;
        let current = Arc::clone(&app.session.current_bundle);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        for replacement in 0..4 {
            app.show_reference_chooser(purpose);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(app.overlay.references().is_some());
            match replacement {
                0 => {
                    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                }
                1 => {
                    // A menu-bar click would normally open a menu. The modal
                    // closes instead and does not forward that same event.
                    app.handle_mouse(MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: 1,
                        row: 0,
                        modifiers: KeyModifiers::NONE,
                    });
                    assert_eq!(app.overlay, Overlay::None);
                }
                2 => app.open_menu(MenuId::Manual),
                3 => app.open_document_finder(),
                _ => unreachable!(),
            }
            assert!(app.overlay.references().is_none());
            assert!(Arc::ptr_eq(&app.session.current_bundle, &current));
            assert!(app.take_open_request().is_none());
            assert!(app.take_copy_request().is_none());
            assert_eq!(app.navigation.history_lengths(), (0, 0));
        }

        app.show_reference_chooser(purpose);
        open_manual(&mut app, "destination", "1");
        assert_eq!(app.overlay, Overlay::None);
        assert!(app.overlay.references().is_none());
        assert_eq!(app.session.current_bundle.label, "destination");
        // A later cancellation/action cannot resurrect the previous page's
        // chooser or emit a stale occurrence against the new document.
        app.handle_reference_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.take_open_request().is_none());
        assert!(app.take_copy_request().is_none());

        app.navigate_history(true);
        let request = app.take_open_request().unwrap();
        app.complete_open(&current, request);
        app.show_reference_chooser(purpose);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let tab = app
            .geometry
            .document_tabs
            .iter()
            .find(|tab| tab.index == 1)
            .unwrap();
        let (column, row) = (tab.area.x + 1, tab.area.y);
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.navigation.active_tab(), 0);
        assert!(
            app.take_open_request().is_none(),
            "dismissing over another tab must not activate it"
        );
    }
}
