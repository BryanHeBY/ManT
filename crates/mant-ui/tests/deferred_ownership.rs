//! Delayed source members at actual terminal cells and public App actions.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use mant_ir::LinkTarget;
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend, style::Modifier};
#[path = "../../mant-engine/tests/support/deferred_ownership.rs"]
mod cases;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn settle(app: &mut App) {
    let mut now = std::time::Instant::now();
    for _ in 0..16 {
        let Some(delay) = app.next_wakeup(now) else {
            break;
        };
        now += delay;
        app.tick(now);
    }
}

#[test]
fn exact_source_members_reach_real_buffers_without_acquiring_new_label_style() {
    for (case, _) in cases::consumers() {
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        cases::assert_rich(&content, &case, true);
        cases::assert_constraints(&content, &case);
        for width in [40, 78] {
            let rendered = DocumentView::new(&content).render(width);
            let buffer = terminal_cells::paint(&rendered, width);
            for recovery in &case.recoveries {
                let hits = rendered.search(&recovery.text);
                assert_eq!(hits.len(), recovery.occurrences, "{} recovery", case.id);
                let hit = &hits[recovery.ordinal];
                let cell = &buffer[(
                    u16::try_from(hit.start_column).unwrap(),
                    u16::try_from(hit.row).unwrap(),
                )];
                assert!(
                    recovery.text.starts_with(cell.symbol()),
                    "{} first actual recovery cell",
                    case.id
                );
                if recovery.style.strong {
                    assert!(cell.modifier.contains(Modifier::BOLD));
                }
                if recovery.style.emphasis {
                    assert!(cell.modifier.contains(Modifier::ITALIC));
                }
            }
            for link in &case.links {
                if link.text.is_empty() {
                    continue;
                }
                let hits = rendered.search(&link.text);
                assert!(!hits.is_empty(), "{} label {:?}", case.id, link.text);
            }
        }
    }
}

#[test]
fn actual_section_label_click_navigates_to_its_own_anchor() {
    let selected = cases::consumers()
        .into_iter()
        .filter(|(case, _)| {
            case.links
                .iter()
                .any(|link| matches!(link.target, LinkTarget::Section { .. }))
        })
        .collect::<Vec<_>>();
    assert!(!selected.is_empty());
    for (case, _) in selected {
        let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        for width in [40, 78] {
            let mut app = App::new(&content);
            key(&mut app, KeyCode::Char('B'));
            let mut terminal = Terminal::new(TestBackend::new(width, 7)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            key(&mut app, KeyCode::Home);
            key(&mut app, KeyCode::Char('/'));
            for ch in "NEXT".chars() {
                key(&mut app, KeyCode::Char(ch));
            }
            key(&mut app, KeyCode::Enter);
            settle(&mut app);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let positions = reader_actions::positions(terminal.backend().buffer(), "NEXT");
            assert!(
                !positions.is_empty(),
                "{} source label is revealed: {}",
                case.id,
                terminal.backend()
            );
            assert!(
                terminal.backend().buffer()[positions[0]]
                    .modifier
                    .contains(Modifier::ITALIC),
                "source Sx label has its authored style, unlike the target heading"
            );
            // The fixed sources have NAME / DESCRIPTION / NEXT. Observing
            // the selected source section before and destination after the
            // action proves navigation even when both sections fit onscreen.
            assert!(terminal.backend().to_string().contains("2/3"));
            let (column, row) = positions[0];
            reader_actions::click(&mut app, column, row);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(
                terminal.backend().to_string().contains("3/3"),
                "{} destination section selected",
                case.id
            );
            assert_eq!(
                reader_actions::opened_targets(&mut app),
                Vec::<String>::new()
            );
            assert_eq!(
                reader_actions::positions(terminal.backend().buffer(), "AFTER").len(),
                1,
                "{} section target reached",
                case.id
            );
            assert!(
                reader_actions::positions(terminal.backend().buffer(), "bad-name").is_empty(),
                "old source was not the destination"
            );
        }
    }
}

fn external(target: &LinkTarget) -> Option<String> {
    match target {
        LinkTarget::External { uri } => Some(uri.clone()),
        LinkTarget::Email { address } => Some(format!("mailto:{address}")),
        _ => None,
    }
}

#[test]
fn click_copy_search_reveal_and_same_app_resize_preserve_original_owners() {
    for (case, narrow) in cases::consumers() {
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        cases::assert_rich(&content, &case, true);
        let snapshot = cases::snapshot(&content);
        let mut app = App::new(&content);
        key(&mut app, KeyCode::Char('B'));
        let mut terminal = Terminal::new(TestBackend::new(78, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        for width in if narrow {
            &[78, 12, 16, 78][..]
        } else {
            &[40, 78][..]
        } {
            terminal.backend_mut().resize(*width, 40);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            key(&mut app, KeyCode::Home);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            // Search from a real offscreen page position, then use exactly
            // the same App after resize. Short unique query fragments locate
            // wrapped labels without treating UTF8 bytes as terminal columns.
            let word = case
                .links
                .iter()
                .find(|link| !link.text.is_empty())
                .map_or_else(
                    || {
                        case.recoveries
                            .first()
                            .map_or("AFTER", |recovery| recovery.text.as_str())
                    },
                    |link| link.text.as_str(),
                );
            let fragment = if word.len() > 8 { &word[..1] } else { word };
            key(&mut app, KeyCode::Char('G'));
            terminal.draw(|frame| app.draw(frame)).unwrap();
            key(&mut app, KeyCode::Char('?'));
            for ch in regex_syntax::escape(fragment).chars() {
                key(&mut app, KeyCode::Char(ch));
            }
            key(&mut app, KeyCode::Enter);
            settle(&mut app);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(
                !reader_actions::positions(terminal.backend().buffer(), fragment).is_empty(),
                "{} {width} search/reveal",
                case.id
            );
            key(&mut app, KeyCode::Home);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            check_recovery_actions(&mut app, terminal.backend().buffer(), &case, *width);
            check_label_actions(
                &mut app,
                terminal.backend().buffer(),
                &case,
                &snapshot,
                *width,
            );
        }
    }
}

fn check_label_actions(
    app: &mut App,
    buffer: &ratatui::buffer::Buffer,
    case: &cases::Case,
    snapshot: &cases::Snapshot,
    width: u16,
) {
    // Every complete displayed label has exact positive hit cells;
    // at narrow sizes its public search has already exercised wrapping.
    for (ordinal, link) in case.links.iter().enumerate() {
        let Some(uri) = external(&link.target) else {
            continue;
        };
        if link.text.is_empty() {
            continue;
        }
        let hits = wrapped_positions(buffer, &link.text);
        assert!(
            !hits.is_empty(),
            "{} {width}: own label {:?}",
            case.id,
            link.text
        );
        // Select the original IR member's scalar occurrence, already
        // checked against independent source gold above. BODY's final Y
        // is not the source label Y, nor are equal-URI links one owner.
        let source_ordinal = cases::positions(&snapshot.text(), &link.text)
            .iter()
            .position(|at| *at == snapshot.links[ordinal].start)
            .unwrap();
        {
            let cells = &hits[source_ordinal];
            for &(column, row) in cells {
                reader_actions::click(app, column, row);
                assert_eq!(
                    reader_actions::opened_targets(app),
                    std::slice::from_ref(&uri),
                    "{} {width}",
                    case.id
                );
            }
            check_copy(app, cells, &link.text, &case.id);
        }
    }
}

fn check_recovery_actions(
    app: &mut App,
    buffer: &ratatui::buffer::Buffer,
    case: &cases::Case,
    width: u16,
) {
    for recovery in &case.recoveries {
        let hits = wrapped_positions(buffer, &recovery.text);
        assert_eq!(
            hits.len(),
            recovery.occurrences,
            "{} {width}: original recovery cells",
            case.id
        );
        let cells = &hits[recovery.ordinal];
        let expected = recovery
            .link
            .and_then(|ordinal| external(&case.links[ordinal].target))
            .into_iter()
            .collect::<Vec<_>>();
        for &(column, row) in cells {
            reader_actions::click(app, column, row);
            assert_eq!(
                reader_actions::opened_targets(app),
                expected,
                "{} {width}: recovery owner",
                case.id
            );
        }
        check_copy(app, cells, &recovery.text, &case.id);
    }
}

fn check_copy(app: &mut App, cells: &[(u16, u16)], expected: &str, id: &str) {
    let &(column, row) = cells.first().unwrap();
    let &(last_column, last_row) = cells.last().unwrap();
    if row == last_row {
        reader_actions::select_span(app, column, row, last_column, Some(column + 1));
    } else {
        reader_actions::pointer(app, MouseEventKind::Down(MouseButton::Left), column, row);
        reader_actions::pointer(
            app,
            MouseEventKind::Drag(MouseButton::Left),
            column + 1,
            row,
        );
        reader_actions::pointer(
            app,
            MouseEventKind::Drag(MouseButton::Left),
            last_column,
            last_row,
        );
        reader_actions::pointer(
            app,
            MouseEventKind::Up(MouseButton::Left),
            last_column,
            last_row,
        );
    }
    let copies = reader_actions::copied_selections(app, |request| match request {
        CopyRequest::Selection { text } => text,
        _ => panic!("selection"),
    });
    // Visual selection intentionally retains soft-row newlines and
    // structural continuation padding. Source glyphs remain exact and
    // ordered; the 40/78 buffers also copy the whole source word.
    let mut visual = String::new();
    let mut previous = row;
    for (scalar, &(column, current)) in expected.chars().zip(cells) {
        if current != previous {
            visual.push('\n');
            // App's outer margin is not part of document visual copying.
            visual.extend(std::iter::repeat_n(
                ' ',
                usize::from(column.saturating_sub(1)),
            ));
        }
        visual.push(scalar);
        previous = current;
    }
    assert_eq!(copies, [visual], "{id} established visual selection");
}

/// Locate nonblank source labels at actual cells, crossing only the unused
/// remainder of a visual row. It neither folds source whitespace nor infers
/// link targets. Every returned cell is independently clicked above.
fn wrapped_positions(buffer: &ratatui::buffer::Buffer, value: &str) -> Vec<Vec<(u16, u16)>> {
    assert!(!value.is_empty() && !value.chars().any(char::is_whitespace));
    let mut found = Vec::new();
    for row in 1..buffer.area.height.saturating_sub(1) {
        for column in 0..buffer.area.width {
            let mut at = (column, row);
            let mut cells = Vec::new();
            let mut matched = true;
            for scalar in value.chars() {
                while at.0 >= buffer.area.width
                    || buffer[at].symbol() == " "
                        && (at.0..buffer.area.width).all(|x| buffer[(x, at.1)].symbol() == " ")
                {
                    at.1 += 1;
                    at.0 = 0;
                    if at.1 >= buffer.area.height.saturating_sub(1) {
                        matched = false;
                        break;
                    }
                    while at.0 < buffer.area.width && buffer[at].symbol() == " " {
                        at.0 += 1;
                    }
                }
                if !matched || buffer[at].symbol() != scalar.to_string() {
                    matched = false;
                    break;
                }
                cells.push(at);
                at.0 += 1;
            }
            if matched && !cells.is_empty() && cells[0] == (column, row) {
                found.push(cells);
            }
        }
    }
    found
}
