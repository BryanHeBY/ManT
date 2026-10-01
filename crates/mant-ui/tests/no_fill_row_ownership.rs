//! Actual reader rows and selections retain accepted invisible native cells.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView, ReaderServices};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

fn pointer(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn position(buffer: &Buffer, label: &str) -> (u16, u16) {
    let width = u16::try_from(label.len()).unwrap();
    let mut found = Vec::new();
    for row in 1..buffer.area.height.saturating_sub(1) {
        for column in 0..buffer.area.width.saturating_sub(width) {
            let text: String = (column..column + width)
                .map(|x| buffer[(x, row)].symbol())
                .collect();
            if text == label {
                found.push((column, row));
            }
        }
    }
    found.into_iter().max_by_key(|(column, _)| *column).unwrap()
}

fn visual_literal_selection(logical: &str, margin: usize) -> String {
    let padding = " ".repeat(margin);
    logical
        .split('\n')
        .enumerate()
        .map(|(index, row)| {
            // The first row starts at ALPHA. Following rows start at visual
            // column zero; ASCII-only empty rows lose padding in selection's
            // existing trim_end rule, while NBSP/content retains that margin.
            if index == 0 || row.is_empty() {
                row.to_owned()
            } else {
                format!("{padding}{row}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_literal_row_copy(
    query: &ResolvedContent,
    source: &str,
    last_word: &str,
    logical_selection: &str,
    distance: usize,
) {
    let mut app = App::new(query);
    let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    for width in [80, 120, 240, 80] {
        terminal.backend_mut().resize(width, 40);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let (start_column, start_row) = position(buffer, "ALPHA");
        let (end_column, end_row) = position(buffer, last_word);
        assert_eq!(usize::from(end_row - start_row), distance, "{source}");
        let rendered = DocumentView::new(query).render(width);
        let hits = rendered.search(last_word);
        let [hit] = hits.as_slice() else {
            panic!("one literal end word expected: {source}");
        };
        let margin = hit.start_column;
        let (heading_column, _) = position(buffer, "DESCRIPTION");
        assert_eq!(usize::from(end_column - heading_column), margin, "{source}");
        let expected = visual_literal_selection(logical_selection, margin);
        let last = end_column + u16::try_from(last_word.len()).unwrap() - 1;
        for (kind, column, row) in [
            (
                MouseEventKind::Down(MouseButton::Left),
                start_column,
                start_row,
            ),
            (MouseEventKind::Drag(MouseButton::Left), last, end_row),
            (MouseEventKind::Up(MouseButton::Left), last, end_row),
        ] {
            pointer(&mut app, kind, column, row);
        }
        let mut selections = Vec::new();
        let mut copy = |request| {
            let CopyRequest::Selection { text } = request else {
                panic!("cross-row visual selection expected")
            };
            selections.push(text);
            Ok(())
        };
        app.service_pending(&mut ReaderServices {
            copy_to_clipboard: Some(&mut copy),
            ..ReaderServices::default()
        });
        assert_eq!(selections, [expected], "{source}\nwidth={width}");
    }
}

#[test]
fn invisible_literal_rows_survive_real_reader_resize_and_cross_row_copy() {
    // These seven exact TH/nf/ALPHA/(cell)/BETA/fi/NEXT sources first ran
    // registered pristine CVS in all five profiles (35 successes, lint=0).
    // term_fill's ASCII_NBRZW graph and no-fill source term_newln retain one
    // occupied blank row, unlike the unbuffered bare \z's overwrite of B.
    // selected_text copies visual cells: subsequent rows start at column
    // zero and keep the reader's declared padding. ASCII-only empty rows
    // lose trailing spaces, while NBSP/content keeps its measured margin.
    // All source row delimiters and surviving glyphs remain strict.
    for (line, last_word, copied, distance) in [
        ("\\&        ", "BETA", "ALPHA\n\nBETA", 2),
        ("\\&", "BETA", "ALPHA\n\nBETA", 2),
        ("        ", "BETA", "ALPHA\n\nBETA", 2),
        (
            "\\~\\~\\~\\~\\~\\~\\~\\~",
            "BETA",
            "ALPHA\n\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\nBETA",
            2,
        ),
        ("\\z", "ETA", "ALPHA\nETA", 1),
        ("\\z        ", "BETA", "ALPHA\n\nBETA", 2),
        ("", "BETA", "ALPHA\n\nBETA", 2),
    ] {
        let source = format!(
            ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.nf\nALPHA\n{line}\nBETA\n.fi\nNEXT\n"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(!json.contains("\\u0000mant:"));
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        for width in [20, 40, 80, 120, 240, 20] {
            let rendered = DocumentView::new(&query).render(width);
            let alpha = rendered.search("ALPHA");
            let beta = rendered.search(last_word);
            assert_eq!(alpha.len(), 1, "{source}\n{:?}", rendered.text);
            assert_eq!(beta.len(), 1, "{source}\n{:?}", rendered.text);
            assert_eq!(beta[0].row - alpha[0].row, distance, "{source}");
        }

        assert_literal_row_copy(&query, &source, last_word, copied, distance);
    }
}
