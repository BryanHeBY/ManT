//! Document and fragment history, tab identity, and tab navigation.

use super::super::*;
use crate::app::navigation_state::DocumentTab;

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
            .map(DocumentTab::label)
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
            .map(DocumentTab::label)
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

#[cfg(feature = "native-link-layout")]
#[test]
fn native_unaddressed_history_and_tab_restore_the_exact_click_members() {
    // AU members/history.json: source-bound Z and U overwrite the cursor,
    // while the physically surviving old Y was never authored by Lk.
    const INPUT: &str = ".Dd November 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd use\n.Sh DESCRIPTION\n.No HEAD\nY\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z U\nEND\n.br\n.Xr printf 3\n.Sh NEXT\nDone.\n";
    let initial = mant_loader::load_roff_bytes_with_link_layout(INPUT.as_bytes()).unwrap();
    let original = initial.content_arc();
    assert_eq!(initial.receipts().len(), 1);
    let addressed =
        mant_loader::ResolvedWithLinkLayout::without_native_links(manual_bundle("printf", "3"));
    for width in [80, 140] {
        let mut app = App::from_native_snapshot(&initial, DocumentCatalog::default());
        let mut terminal = Terminal::new(TestBackend::new(width, 25)).unwrap();
        for step in [0, 1, 2] {
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(Arc::ptr_eq(&app.session.current_bundle, &original));
            let rendered = &app.session.rendered_cache[&app.geometry.content.width];
            let labels = rendered.search("ZYU");
            let [label] = labels.as_slice() else {
                panic!("one mixed label");
            };
            let nexts = rendered.search("printf(3)");
            let [next] = nexts.as_slice() else {
                panic!("one manual link");
            };
            for (index, owned) in [true, false, true].into_iter().enumerate() {
                click_document_cell(&mut app, label.start_column + index, label.row);
                assert_eq!(
                    app.take_external_request()
                        .as_ref()
                        .map(crate::ExternalUri::as_str),
                    owned.then_some("https://z.test"),
                    "width={width}, step={step}, glyph={index}",
                );
            }
            if step == 2 {
                break;
            }
            click_document_cell(&mut app, next.start_column, next.row);
            if step == 0 {
                assert!(app.service_pending_with_native_links(
                    &mut crate::ReaderServices::default(),
                    |_| Err("unavailable".into()),
                ));
                assert!(Arc::ptr_eq(&app.session.current_bundle, &original));
                assert_eq!(app.navigation.history_lengths(), (0, 0));
                assert_eq!(app.navigation.active_tab(), 0);
                click_document_cell(&mut app, next.start_column, next.row);
            }
            let mut source_opens = 0;
            assert!(app.service_pending_with_native_links(
                &mut crate::ReaderServices::default(),
                |_| {
                    source_opens += 1;
                    Ok(addressed.clone())
                },
            ));
            assert_eq!(source_opens, 1);
            assert_eq!(app.session.document.label(), "printf");
            terminal.draw(|frame| app.draw(frame)).unwrap();
            if step == 0 {
                app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
                assert!(app.take_open_request().is_none());
                assert_eq!(app.navigation.history_lengths(), (0, 1));
                app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
                // Addressed forward navigation still asks the host. A failed
                // reload neither advances the history cursor nor changes source.
                assert!(app.service_pending_with_native_links(
                    &mut crate::ReaderServices::default(),
                    |_| Err("forward unavailable".into()),
                ));
                assert_eq!(app.navigation.history_lengths(), (0, 1));
                assert!(Arc::ptr_eq(&app.session.current_bundle, &original));
                app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
                assert!(app.service_pending_with_native_links(
                    &mut crate::ReaderServices::default(),
                    |_| Ok(addressed.clone()),
                ));
                assert_eq!(app.session.document.label(), "printf");
                app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
                assert!(app.take_open_request().is_none());
            } else {
                let native_tab = app
                    .geometry
                    .document_tabs
                    .iter()
                    .find(|tab| tab.index == 0)
                    .unwrap()
                    .area;
                app.handle_mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: native_tab.x + 1,
                    row: native_tab.y,
                    modifiers: KeyModifiers::NONE,
                });
                assert!(app.take_open_request().is_none());
                assert_eq!(app.navigation.active_tab(), 0);
            }
        }
    }
}
