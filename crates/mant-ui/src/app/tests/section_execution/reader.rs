//! Native source rows in actual App viewport, search, and pointer events.
use super::super::contribution::{copy_label, draw, key};
use super::*;

#[test]
fn section_source_rows_paint_without_a_stale_literal_in_the_reader() {
    for case in matrix() {
        let input = case.input();
        let mut app = App::new(&input);
        key(&mut app, KeyCode::Char('B'));
        let mut terminal = Terminal::new(TestBackend::new(124, 24)).unwrap();
        for width in WIDTHS {
            terminal.backend_mut().resize(width + 4, 24);
            draw(&mut app, &mut terminal);
            assert_eq!(app.geometry.content.width, width, "{}", case.id);
            assert_painted_rows(&case, &app, terminal.backend().buffer());
            report(&case.id, "full/buffer", Some(width));
        }
    }
}

#[test]
fn section_search_reveal_keeps_source_owner_and_link_through_resize() {
    for case in matrix() {
        let input = case.input();
        let mut app = App::new(&input);
        key(&mut app, KeyCode::Char('B'));
        let mut terminal = Terminal::new(TestBackend::new(82, 8)).unwrap();
        draw(&mut app, &mut terminal);
        let initial = &app.session.rendered_cache[&78].search(BODY)[0];
        assert!(initial.row >= usize::from(app.geometry.content.height));

        key(&mut app, KeyCode::Char('/'));
        for letter in BODY.chars() {
            key(&mut app, KeyCode::Char(letter));
        }
        key(&mut app, KeyCode::Enter);
        draw(&mut app, &mut terminal);
        let hit = &app.search.scope_matches[app.search.active_match].rendered;
        let source_record = app.session.rendered_cache[&78]
            .viewport_anchor(hit.row)
            .unwrap()
            .0;
        for width in WIDTHS {
            terminal.backend_mut().resize(width + 4, 8);
            draw(&mut app, &mut terminal);
            assert_eq!(app.geometry.content.width, width, "{}", case.id);
            assert_eq!(app.search.query, BODY, "{}", case.id);
            assert_eq!(app.search.scope_matches.len(), 1, "{}", case.id);
            let hit = &app.search.scope_matches[app.search.active_match].rendered;
            assert_eq!(
                app.session.rendered_cache[&width]
                    .viewport_anchor(hit.row)
                    .unwrap()
                    .0,
                source_record,
                "{}",
                case.id
            );
            assert_eq!(
                app.session.document.navigation()[app.selected].id,
                "next",
                "{} width {width}",
                case.id
            );
            assert_link_viewport(&case, &mut app, &mut terminal);
            report(
                &case.id,
                "selected/search-reveal-and-hit-range",
                Some(width),
            );
        }
    }
}

fn assert_painted_rows(case: &Case, app: &App, buffer: &ratatui::buffer::Buffer) {
    let width = app.geometry.content.width;
    let rendered = &app.session.rendered_cache[&width];
    let expected = case.reading_rows();
    let mut row = rendered.search(expected[0])[0].row;
    for expected in expected {
        if expected.is_empty() {
            let (left, y) = viewport_cell(app, row, 0);
            assert!(
                (left..app.geometry.content.right()).all(|x| buffer[(x, y)].symbol() == " "),
                "{} width {width}: expected a blank row at {row}",
                case.id
            );
            row += 1;
        } else {
            let hit = &rendered.search(expected)[0];
            assert_eq!(hit.row, row, "{} width {width} {expected:?}", case.id);
            if expected == case.heading() {
                assert_eq!(rendered.anchor_row("next"), Some(row), "{}", case.id);
            }
            assert_painted_glyphs(app, buffer, &source_cells(rendered, expected));
            // Only soft continuations of this logical source row may add
            // viewport rows. A blank left by the old owner is not a wrap.
            row += hit.fragment_count();
        }
    }
}

pub(super) fn source_cells(
    rendered: &crate::RenderedDocument,
    text: &str,
) -> Vec<(char, usize, usize)> {
    let (record, start) = (0..rendered.search_record_count())
        .find_map(|record| {
            rendered
                .search_record_text(record)
                .find(text)
                .map(|start| (record, start))
        })
        .unwrap_or_else(|| panic!("missing source record {text:?}"));
    text.char_indices()
        .filter(|(_, character)| *character != ' ')
        .map(|(offset, character)| {
            let start = start + offset;
            let hit = rendered
                .project_search_range(record, start..start + character.len_utf8())
                .unwrap();
            (character, hit.row, hit.start_column)
        })
        .collect()
}

pub(super) fn viewport_cell(app: &App, row: usize, column: usize) -> (u16, u16) {
    let area = app.geometry.content;
    let scroll = app.session.content_scroll;
    assert!((scroll..scroll + usize::from(area.height)).contains(&row));
    assert!(column < usize::from(area.width));
    (
        area.x + u16::try_from(column).unwrap(),
        area.y + u16::try_from(row - scroll).unwrap(),
    )
}

pub(super) fn assert_painted_glyphs(
    app: &App,
    buffer: &ratatui::buffer::Buffer,
    cells: &[(char, usize, usize)],
) {
    for &(character, row, column) in cells {
        assert_eq!(
            buffer[viewport_cell(app, row, column)].symbol(),
            character.to_string()
        );
    }
}

fn assert_link_viewport(case: &Case, app: &mut App, terminal: &mut Terminal<TestBackend>) {
    let width = app.geometry.content.width;
    let rendered = &app.session.rendered_cache[&width];
    let cells = source_cells(rendered, BODY);
    assert_painted_glyphs(app, terminal.backend().buffer(), &cells);
    if let Some(target) = case.link_target.as_deref() {
        let &(_, first_row, first_column) = cells.first().unwrap();
        let &(_, last_row, last_column) = cells.last().unwrap();
        let heading = source_cells(rendered, case.heading())[0];
        for _ in heading.1..app.session.content_scroll {
            key(app, KeyCode::Up);
        }
        draw(app, terminal);
        click_document_cell(app, heading.2, heading.1);
        assert!(app.take_external_request().is_none());
        for (row, column) in [(first_row, first_column), (last_row, last_column)] {
            click_document_cell(app, column, row);
            assert_eq!(
                app.take_external_request()
                    .as_ref()
                    .map(crate::ExternalUri::as_str),
                Some(target)
            );
        }
        if last_column + 1 < usize::from(width) {
            click_document_cell(app, last_column + 1, last_row);
            assert!(app.take_external_request().is_none());
        }
        // Clipboard copies visual rows. Keep just the link label, with
        // continuation indentation only when the viewport wraps it.
        let mut expected = String::new();
        let mut previous = first_row;
        for &(character, row, column) in &cells {
            if row != previous {
                expected.push('\n');
                expected.extend(std::iter::repeat_n(' ', column));
            }
            expected.push(character);
            previous = row;
        }
        let first = viewport_cell(app, first_row, first_column);
        let last = viewport_cell(app, last_row, last_column);
        assert_eq!(copy_label(app, first, last), expected);
    }
}
