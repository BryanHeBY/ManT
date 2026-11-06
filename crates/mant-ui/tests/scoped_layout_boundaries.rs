//! Actual cells retain scoped origins, table rows, real Pp space and copy text.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView, RenderedDocument};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "../../mant-engine/tests/support/scoped_layout_cases.rs"]
mod recorded;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;
#[path = "support/wrapped_labels.rs"]
mod wrapped_labels;

fn reader_app(content: &ResolvedContent) -> App {
    let mut app = App::new(content);
    app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
    app
}

fn assert_source_cells(content: &ResolvedContent, case: &recorded::Case) {
    let rendered = DocumentView::new(content).render(78);
    let painted = terminal_cells::paint(&rendered, 78);
    let source_start = rendered.anchor_row("description").unwrap() + 1;
    let mut app = reader_app(content);
    // The hidden-sidebar App reserves one cell on each side and a two-cell
    // scrollbar gutter. Terminal82 therefore gives document width78, with
    // its own stable origin at x1, independently of the source's row hints.
    let mut terminal = Terminal::new(TestBackend::new(82, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    // The App can also display DESCRIPTION in its navigation/breadcrumbs.
    // Locate an authored body label at its independently recorded column,
    // then compare the entire source-bound row window, including its gaps.
    let label = case.labels()[0];
    let label_row = case
        .utf8_rows
        .iter()
        .position(|row| row.contains(label))
        .unwrap();
    let byte = case.utf8_rows[label_row].find(label).unwrap();
    let label_column = 4 + case.utf8_rows[label_row][..byte].width();
    let found = reader_actions::positions(buffer, label)
        .into_iter()
        .filter(|(column, _)| usize::from(*column) == label_column)
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "{}: actual body row", case.id);
    let first = usize::from(found[0].1).checked_sub(label_row).unwrap();
    // Copy only the declared document viewport, leaving the separately
    // rendered scrollbar out of the full-cell document-row comparison.
    let mut document_cells = Buffer::empty(Rect::new(0, 0, 78, buffer.area.height));
    for row in 0..buffer.area.height {
        for column in 0..78 {
            document_cells[(column, row)] = buffer[(column + 1, row)].clone();
        }
    }
    for (offset, row) in case.utf8_rows.iter().enumerate() {
        terminal_cells::assert_row(&document_cells, first + offset, 3, row, &case.id);
        terminal_cells::assert_row(&painted, source_start + offset, 3, row, &case.id);
    }
}

fn assert_search_copy(content: &ResolvedContent, case: &recorded::Case) {
    for width in [40, 78] {
        let rendered = DocumentView::new(content).render(width);
        let mut app = reader_app(content);
        let mut terminal = Terminal::new(TestBackend::new(width + 4, 40)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        for label in case.labels() {
            // This public literal search deliberately folds case. The table
            // header Description also matches the DESCRIPTION section title;
            // require both real matches, with exactly one outside that title.
            let matches = rendered.search(label);
            let heading_row = rendered.anchor_row("description").unwrap();
            let heading_matches = matches
                .iter()
                .filter(|matched| matched.row == heading_row)
                .count();
            assert_eq!(
                heading_matches,
                usize::from(label.eq_ignore_ascii_case("DESCRIPTION")),
                "{}/{width}: {label}: section match",
                case.id
            );
            assert_eq!(
                matches.len() - heading_matches,
                1,
                "{}/{width}: {label}: authored body match",
                case.id
            );
            assert_label_copy(&mut app, terminal.backend().buffer(), &rendered, label);
        }
    }
}

fn assert_label_copy(app: &mut App, buffer: &Buffer, rendered: &RenderedDocument, label: &str) {
    let fragments = if label.chars().any(char::is_whitespace) {
        let found = reader_actions::positions(buffer, label);
        assert_eq!(found.len(), 1, "actual body label: {label}");
        vec![(label.to_owned(), found[0].1, found[0].0)]
    } else {
        let start = rendered.anchor_row("description").unwrap() + 1;
        wrapped_labels::fragments(rendered, start..rendered.row_count, label)
            .into_iter()
            .map(|(text, row, column)| {
                (
                    text,
                    u16::try_from(row).unwrap() + 2,
                    u16::try_from(column).unwrap() + 1,
                )
            })
            .collect()
    };
    let mut copied_label = String::new();
    for (text, row, column) in fragments {
        let mut offset = 0;
        for grapheme in mant_render::cells::graphemes(&text) {
            let cell = column + u16::try_from(offset).unwrap();
            assert_eq!(buffer[(cell, row)].symbol(), grapheme.text(), "{label}");
            offset += grapheme.columns();
        }
        reader_actions::click(app, column, row);
        assert_eq!(reader_actions::opened_targets(app), Vec::<String>::new());
        let last = column + u16::try_from(text.width()).unwrap() - 1;
        reader_actions::select_span(
            app,
            column,
            row,
            last,
            (last == column).then_some(column + 1),
        );
        let copied = reader_actions::copied_selections(app, |request| {
            let CopyRequest::Selection { text } = request else {
                panic!("visual copy");
            };
            text
        });
        assert_eq!(copied, [text.as_str()], "{label}: exact selected fragment");
        copied_label.push_str(&copied[0]);
    }
    assert_eq!(copied_label, label, "exact complete authored label");
}

#[test]
fn pristine_scope_rows_paint_once_and_stay_searchable_after_json_and_two_readers() {
    // mdoc_term.c print_mdoc_node, termp_it_pre/post and termp_bd_pre/post:
    // source geometry is scoped, whereas completed physical rows remain real.
    for case in recorded::cases() {
        let mut content = json_roundtrip::json_string_roundtrip(&case.load());
        assert_eq!(recorded::cli_body(&content), case.utf8_rows.join("\n"));
        assert_source_cells(&content, &case);
        assert_eq!(
            recorded::description(&content).children,
            Vec::<mant_ir::Section>::new()
        );
        for round in 0..=2 {
            if round > 0 {
                case.assert_portable(&content);
            }
            assert_search_copy(&content, &case);
            if round < 2 {
                content = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(
                        &mant_codec::encode::render_markdown(&content),
                        None,
                    )
                    .unwrap(),
                );
            }
        }
    }
}
