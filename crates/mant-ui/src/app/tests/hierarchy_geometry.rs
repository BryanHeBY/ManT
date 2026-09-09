//! A frame's final outline width owns drawing, hit testing and scrollbar state.

use super::*;
use ratatui::{buffer::Buffer, style::Color};
use unicode_width::UnicodeWidthStr;

// Each non-ASCII part is one deliberately chosen, complete display cluster.
// ASCII runs are split below. This oracle does not call navigation's layout,
// prefix builder, wrapping, row-range or hit-map helpers.
const TITLE_PARTS: &[&str] = &[
    "C",
    "a",
    "f",
    "e\u{301}",
    "👩‍💻",
    "中",
    "文",
    "ABCDEFGHIJKLMNOP",
];
const TARGET_PARTS: &[&str] = &[
    "目",
    "标",
    "-",
    "C",
    "a",
    "f",
    "e\u{301}",
    "-",
    "👩‍💻",
    "-",
    "abcdefghijklmnopqrstuvwxyz",
];

fn hierarchy_bundle() -> ResolvedContent {
    let mut bundle = navigation_bundle();
    bundle.address = Some(DocumentAddress::Markdown {
        path: "catalog".into(),
        origin: MarkdownOrigin::Documents,
    });
    let document = bundle.document.as_mut().expect("manual");
    let mut owner = document.sections[0].clone();
    owner.id = "owner".into();
    owner.heading = mant_ir::Heading {
        content: vec![Inline::Link {
            target: mant_ir::LinkTarget::Document {
                name: TARGET_PARTS.concat(),
                fragment: None,
            },
            title: None,
            children: vec![Inline::Text {
                value: TITLE_PARTS.concat(),
            }],
        }],
        source: None,
    };
    owner.blocks.clear();
    owner.children = (0..24)
        .map(|index| Section {
            id: format!("child-{index}").into(),
            fragment_aliases: Vec::new(),
            heading: format!("CHILD {index}").into(),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: Vec::new(),
            source: None,
        })
        .collect();
    let next = Section {
        id: "next".into(),
        fragment_aliases: Vec::new(),
        heading: "NEXT".into(),
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    };
    document.sections = vec![owner, next];
    bundle
}

fn owner_index(app: &App) -> usize {
    app.session
        .document
        .navigation()
        .iter()
        .position(|node| node.id == "owner")
        .expect("owner")
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn expected_clusters() -> Vec<(String, Color)> {
    let mut clusters = Vec::new();
    for (parts, color) in [
        (TITLE_PARTS, theme::SELECTED_TEXT),
        (&[" ", "↗", " "][..], theme::LINK),
        (TARGET_PARTS, theme::LINK),
    ] {
        for part in parts {
            if part.is_ascii() {
                clusters.extend(part.chars().map(|character| (character.to_string(), color)));
            } else {
                clusters.push(((*part).to_owned(), color));
            }
        }
    }
    clusters
}

/// Fixed geometry oracle: root labels start at column five regardless of
/// children, expansion or a badge. Greedy packing here uses only authored
/// fixture clusters, not rows produced by the implementation under test.
fn assert_owner_frame(app: &App, buffer: &Buffer, frame: &Buffer, scrollbar: bool) -> usize {
    let area = app.geometry.navigation;
    let owner = owner_index(app);
    assert_eq!(app.selected, owner);
    assert_eq!(app.navigation_scroll, 0);
    assert_eq!(app.geometry.navigation_scrollbar.is_some(), scrollbar);
    let right = area.right() - u16::from(scrollbar);
    let start = area.x + 5;
    let mut column = start;
    let mut row = area.y;
    let mut badge_rows = HashSet::new();
    let mut hidden_cells = HashSet::new();
    for (cluster, foreground) in expected_clusters() {
        let width = u16::try_from(cluster.width()).expect("fixture cluster width");
        if column + width > right {
            row += 1;
            column = start;
        }
        assert!(row < area.bottom(), "the complete selected label must fit");
        assert_eq!(
            app.geometry.navigation_rows[usize::from(row - area.y)],
            owner
        );
        let cell = buffer.cell((column, row)).expect("label cell");
        assert_eq!(cell, frame.cell((column, row)).unwrap());
        assert_eq!(cell.symbol(), cluster, "cluster at ({column}, {row})");
        assert_eq!(cell.fg, foreground, "role at ({column}, {row})");
        assert_eq!(cell.bg, theme::SELECTED);
        // Paragraph first paints its whole area with SIDEBAR, then render_line
        // writes only each grapheme's head (unlike Buffer::set_stringn, which
        // resets tails). Thus canonical tails keep the blank sidebar cell.
        // The terminal diff omits these hidden cells; TestBackend's raw tails
        // may retain old symbols/styles after reflow. Check canonical tails
        // precisely, and every visible backend head.
        for tail in 1..width {
            let position = (column + tail, row);
            hidden_cells.insert(position);
            let mut expected = ratatui::buffer::Cell::default();
            expected.set_bg(theme::SIDEBAR);
            assert_eq!(frame.cell(position).unwrap(), &expected);
        }
        if foreground == theme::LINK {
            badge_rows.insert(row);
        }
        column += width;
    }
    assert!(
        badge_rows.len() >= 2,
        "fixture must exercise badge continuation"
    );
    let expected_rows = usize::from(row - area.y + 1);
    assert_eq!(
        app.geometry
            .navigation_rows
            .iter()
            .filter(|index| **index == owner)
            .count(),
        expected_rows
    );
    for label_row in area.y..=row {
        for x in start..right {
            if !hidden_cells.contains(&(x, label_row)) {
                assert_eq!(buffer.cell((x, label_row)).unwrap().bg, theme::SELECTED);
            }
        }
    }
    for y in area.y..area.bottom() {
        let last = buffer.cell((area.right() - 1, y)).unwrap();
        if scrollbar {
            assert_eq!(last.symbol(), " ");
            assert!(last.bg == theme::SCROLLBAR_THUMB || last.bg == theme::SCROLLBAR_TRACK);
        } else {
            // SELECTED and SCROLLBAR_TRACK deliberately share BORDER. Absence
            // of a scrollbar is geometry plus the final content cell, not a
            // promise that this color cannot appear in the selected label.
            let coordinate = (area.right() - 1, y);
            let canonical = frame.cell(coordinate).unwrap();
            let hidden = hidden_cells.contains(&coordinate);
            assert_eq!(
                canonical.bg,
                if y <= row && !hidden {
                    theme::SELECTED
                } else {
                    theme::SIDEBAR
                }
            );
            if !hidden {
                assert_eq!(last, canonical);
            }
        }
    }
    expected_rows
}

#[test]
fn hierarchy_collapse_and_expand_reuse_final_width_for_badges_and_hits() {
    let bundle = hierarchy_bundle();
    let original = bundle.clone();
    let mut app = App::new(&bundle);
    let owner = owner_index(&app);
    app.set_selected_index(owner);
    app.full_outline_labels = true;
    let original_nodes = app.session.document.navigation().to_vec();
    let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();

    for expanded in [true, false, true, false] {
        if app.expanded.contains("owner") != expanded {
            app.toggle_selected();
        }
        crate::navigation::start_layout_trace();
        let frame = terminal
            .draw(|frame| app.draw(frame))
            .unwrap()
            .buffer
            .clone();
        let (builds, layouts) = crate::navigation::take_layout_trace();
        assert_eq!(builds, 1, "exactly one final-forest plan per draw");
        assert_eq!(layouts.len(), if expanded { 1 } else { 2 });
        assert_eq!(layouts[0].1, usize::from(app.geometry.navigation.width - 1));
        if !expanded {
            assert_eq!(
                layouts[0].0, layouts[1].0,
                "both widths borrow the same plan"
            );
            assert_eq!(layouts[1].1, layouts[0].1 + 1);
        }
        assert_owner_frame(&app, terminal.backend().buffer(), &frame, expanded);
        assert_eq!(app.session.document.navigation(), original_nodes);
        assert_eq!(app.session.current_bundle.as_ref(), &original);
        assert!(app.take_open_request().is_none());
    }
    assert_eq!(bundle, original, "display metadata must not rewrite IR");
}

#[test]
fn every_title_and_badge_continuation_click_keeps_the_original_owner_and_target() {
    let bundle = hierarchy_bundle();
    let mut app = App::new(&bundle);
    let owner = owner_index(&app);
    let other = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.id == "next")
        .unwrap();
    app.set_selected_index(owner);
    app.full_outline_labels = true;
    let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
    let frame = terminal
        .draw(|frame| app.draw(frame))
        .unwrap()
        .buffer
        .clone();
    let rows = assert_owner_frame(&app, terminal.backend().buffer(), &frame, true);
    for offset in 0..rows {
        // Keep the last drawn frame, but make this a selection click rather
        // than the documented re-click-to-collapse action on a selected owner.
        app.selected = other;
        let area = app.geometry.navigation;
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.x + 5,
            area.y + u16::try_from(offset).unwrap(),
        ));
        assert_eq!(app.selected, owner, "physical continuation {offset}");
        assert!(
            app.take_open_request().is_none(),
            "selection is not activation"
        );
        let frame = terminal
            .draw(|frame| app.draw(frame))
            .unwrap()
            .buffer
            .clone();
        assert_owner_frame(&app, terminal.backend().buffer(), &frame, true);
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::SHIFT));
    assert!(matches!(app.overlay, Overlay::References(_)));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let request = app
        .take_open_request()
        .expect("explicitly open the associated heading link");
    assert!(matches!(request.document,
        mant_protocol::DocumentOpenTarget::Address {
            address: DocumentAddress::Markdown { path, origin: MarkdownOrigin::Documents }
        } if path == TARGET_PARTS.concat()));
}

#[test]
fn disappearing_outline_scrollbar_finishes_drag_before_old_pointer_events() {
    let mut app = App::new(&hierarchy_bundle());
    app.set_selected_index(owner_index(&app));
    app.full_outline_labels = true;
    let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let scrollbar = app.geometry.navigation_scrollbar.unwrap().area();
    app.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        scrollbar.x,
        scrollbar.y,
    ));
    assert!(matches!(
        app.pointer.drag(),
        PointerDrag::NavigationScrollbar(_)
    ));

    app.toggle_selected();
    let frame = terminal
        .draw(|frame| app.draw(frame))
        .unwrap()
        .buffer
        .clone();
    assert_owner_frame(&app, terminal.backend().buffer(), &frame, false);
    assert_eq!(app.pointer.drag(), PointerDrag::None);
    let width = app.sidebar_width;
    for kind in [
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse(mouse(kind, scrollbar.x, scrollbar.bottom() - 1));
        assert_eq!(app.pointer.drag(), PointerDrag::None);
        assert_eq!(app.navigation_scroll, 0);
        assert_eq!(app.sidebar_width, width);
    }

    app.toggle_selected();
    let frame = terminal
        .draw(|frame| app.draw(frame))
        .unwrap()
        .buffer
        .clone();
    assert_owner_frame(&app, terminal.backend().buffer(), &frame, true);
    let restored = app.geometry.navigation_scrollbar.unwrap();
    let area = restored.area();
    app.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x,
        area.bottom() - 1,
    ));
    assert_eq!(app.navigation_scroll, restored.maximum());
    assert!(matches!(
        app.pointer.drag(),
        PointerDrag::NavigationScrollbar(_)
    ));
    app.handle_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        area.x,
        area.bottom() - 1,
    ));
    assert_eq!(app.pointer.drag(), PointerDrag::None);
    assert_eq!(app.sidebar_width, width);
}
