use std::cell::RefCell;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{Terminal, backend::TestBackend};

use super::super::*;

fn reader() -> App {
    let parsed = mant_codec::parse_markdown(
        "# SOURCE_RETAINED\n\n## Links\n\n[OPEN_DESTINATION](man:destination(1))\n\n[EXTERNAL_DESTINATION](https://example.test/)\n\nCOPY_PAYLOAD\n",
        None,
    )
    .unwrap();
    App::new(&ResolvedContent {
        address: None,
        label: "SOURCE_RETAINED".into(),
        document: Some(parsed.document),
        tldr: parsed.tldr,
    })
}

fn screen(app: &mut App) -> (String, ratatui::buffer::Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(140, 28)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let text = buffer
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    (text, buffer)
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_event(&Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }));
}

fn content_position(app: &mut App, text: &str) -> (u16, u16) {
    let (_, buffer) = screen(app);
    for row in 0..buffer.area.height {
        let line: String = (0..buffer.area.width)
            .map(|column| buffer[(column, row)].symbol())
            .collect();
        // These ASCII labels are deliberately longer than the sidebar and
        // unique to body links/text, so their screen cells are unambiguous.
        if let Some(column) = line.rfind(text) {
            return (u16::try_from(line[..column].chars().count()).unwrap(), row);
        }
    }
    panic!("missing visible fixture text: {text}");
}

fn click(app: &mut App, label: &str) {
    let (column, row) = content_position(app, label);
    mouse(app, MouseEventKind::Down(MouseButton::Left), column, row);
    mouse(app, MouseEventKind::Up(MouseButton::Left), column, row);
}

fn queue_copy(app: &mut App) {
    let (column, row) = content_position(app, "COPY_PAYLOAD");
    mouse(app, MouseEventKind::Down(MouseButton::Left), column, row);
    mouse(
        app,
        MouseEventKind::Drag(MouseButton::Left),
        column + 4,
        row,
    );
    mouse(app, MouseEventKind::Up(MouseButton::Left), column + 4, row);
}

fn queue_discovery(app: &mut App) {
    app.handle_event(&Event::Key(KeyEvent::new(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    )));
}

#[test]
fn explicit_capabilities_are_called_once_in_original_service_order() {
    let mut app = reader();
    click(&mut app, "OPEN_DESTINATION");
    click(&mut app, "EXTERNAL_DESTINATION");
    queue_copy(&mut app);
    queue_discovery(&mut app);
    let calls = RefCell::new(Vec::new());
    let mut discover = |_: &CatalogQuery| {
        calls.borrow_mut().push("discover");
        Ok(DocumentCatalog::default())
    };
    let mut open = |target: &DocumentOpenTarget| {
        calls.borrow_mut().push("open");
        assert_eq!(
            target,
            &DocumentOpenTarget::Address {
                address: mant_protocol::DocumentAddress::Manual {
                    name: "destination".into(),
                    manual_section: "1".into(),
                },
            }
        );
        Err("load failed".into())
    };
    let mut external = |uri: &ExternalUri| {
        calls.borrow_mut().push("external");
        assert_eq!(uri.as_str(), "https://example.test/");
        Ok(())
    };
    let mut copy = |request| {
        calls.borrow_mut().push("copy");
        let CopyRequest::Selection { text } = request else {
            panic!("drag must queue selection copy");
        };
        assert_eq!(text, "COPY_");
        Ok(())
    };
    let mut services = ReaderServices {
        discover_documents: Some(&mut discover),
        open_document: Some(&mut open),
        open_external: Some(&mut external),
        copy_to_clipboard: Some(&mut copy),
    };
    assert!(app.service_pending(&mut services));
    assert_eq!(*calls.borrow(), ["discover", "open", "external", "copy"]);
    assert!(!app.service_pending(&mut services));
    assert_eq!(calls.borrow().len(), 4);
}

#[test]
fn successful_open_keeps_already_queued_external_and_source_copy_effects() {
    let mut app = reader();
    click(&mut app, "OPEN_DESTINATION");
    click(&mut app, "EXTERNAL_DESTINATION");
    queue_copy(&mut app);
    let calls = RefCell::new(Vec::new());
    let mut open = |_: &DocumentOpenTarget| {
        calls.borrow_mut().push("open");
        let parsed = mant_codec::parse_markdown("# NEW_PAGE\n\nReplacement body.\n", None).unwrap();
        Ok(ResolvedContent {
            address: None,
            label: "NEW_PAGE".into(),
            document: Some(parsed.document),
            tldr: parsed.tldr,
        })
    };
    let mut external = |uri: &ExternalUri| {
        calls.borrow_mut().push("external");
        assert_eq!(uri.as_str(), "https://example.test/");
        Ok(())
    };
    let mut copy = |request| {
        calls.borrow_mut().push("copy");
        let CopyRequest::Selection { text } = request else {
            panic!("selection");
        };
        assert_eq!(text, "COPY_");
        Ok(())
    };
    let mut services = ReaderServices {
        discover_documents: None,
        open_document: Some(&mut open),
        open_external: Some(&mut external),
        copy_to_clipboard: Some(&mut copy),
    };
    assert!(app.service_pending(&mut services));
    assert_eq!(*calls.borrow(), ["open", "external", "copy"]);
    assert!(!app.service_pending(&mut services));
    let (text, _) = screen(&mut app);
    assert!(text.contains("Replacement body."));
    assert!(!text.contains("COPY_PAYLOAD"));
}

#[test]
fn absent_capabilities_report_unavailable_without_fallback_or_replay() {
    for (queue, notice) in [
        (
            queue_discovery as fn(&mut App),
            "document discovery is unavailable in this host",
        ),
        (
            |app| click(app, "OPEN_DESTINATION"),
            "document loading is unavailable in this host",
        ),
        (
            |app| click(app, "EXTERNAL_DESTINATION"),
            "external links are unavailable in this host",
        ),
        (queue_copy, "clipboard access is unavailable in this host"),
    ] {
        let mut app = reader();
        queue(&mut app);
        let mut services = ReaderServices::default();
        assert!(app.service_pending(&mut services));
        assert!(screen(&mut app).0.contains(notice));
        assert!(!app.service_pending(&mut services));
        assert!(screen(&mut app).0.contains("SOURCE_RETAINED"));
    }
}

#[test]
fn failed_loading_retains_source_and_a_new_activation_can_retry() {
    let mut app = reader();
    let calls = RefCell::new(0);
    let mut open = |_: &DocumentOpenTarget| {
        *calls.borrow_mut() += 1;
        Err("fixture load rejected".into())
    };
    let mut services = ReaderServices {
        open_document: Some(&mut open),
        ..ReaderServices::default()
    };
    for expected in [1, 2] {
        click(&mut app, "OPEN_DESTINATION");
        assert!(app.service_pending(&mut services));
        assert_eq!(*calls.borrow(), expected);
        let text = screen(&mut app).0;
        assert!(text.contains("SOURCE_RETAINED"));
        assert!(text.contains("fixture load rejected"));
    }
}

#[test]
fn supplied_events_keep_press_filter_and_do_not_service_host_capabilities() {
    let mut app = reader();
    let release = KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );
    assert_eq!(
        app.handle_event(&Event::Key(release)),
        UpdateOutcome::Unchanged
    );
    assert!(!app.should_quit());
    assert_eq!(
        app.handle_event(&Event::Resize(90, 20)),
        UpdateOutcome::Redraw
    );
    assert_eq!(
        app.handle_event(&Event::Paste("q".into())),
        UpdateOutcome::Unchanged
    );
    assert!(!app.should_quit());
    app.handle_event(&Event::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    )));
    assert!(app.should_quit());
}
