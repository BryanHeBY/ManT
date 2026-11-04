//! Paint actual document rows and compare authored cells plus edge padding.

use mant_ui::RenderedDocument;
use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Span, widgets::Widget};
use unicode_width::UnicodeWidthStr;

pub(super) fn paint(rendered: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    (&rendered.text).render(area, &mut buffer);
    buffer
}

pub(super) fn assert_row(
    buffer: &Buffer,
    row: usize,
    origin: usize,
    expected: &str,
    context: &str,
) {
    let mut symbols = vec![" ".to_owned(); origin];
    let span = Span::raw(expected);
    // Segment only the frozen expected text, independently of product layout.
    // Wide graphemes occupy a leading symbol and blank continuation cells.
    for grapheme in span.styled_graphemes(Style::default()) {
        let width = grapheme.symbol.width();
        if width > 0 {
            symbols.push(grapheme.symbol.to_owned());
            symbols.extend(std::iter::repeat_n(" ".to_owned(), width - 1));
        }
    }
    assert!(
        symbols.len() <= usize::from(buffer.area.width),
        "{context}: expected row exceeds viewport"
    );
    symbols.resize(usize::from(buffer.area.width), " ".to_owned());
    for (column, expected) in symbols.iter().enumerate() {
        assert_eq!(
            buffer[(column.try_into().unwrap(), row.try_into().unwrap())].symbol(),
            expected,
            "{context}: row {row}, column {column}"
        );
    }
}

#[test]
fn authored_graphemes_and_edge_padding_are_checked_by_terminal_cell() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
    // This expected row is authored directly: one wide grapheme, one combined
    // grapheme, two internal SP, and a final letter. No product renderer runs.
    for (column, symbol) in [(2, "界"), (4, "e\u{301}"), (7, "Z")] {
        buffer[(column, 0)].set_symbol(symbol);
    }
    assert_row(&buffer, 0, 2, "界e\u{301}  Z", "Unicode row");

    // Leading padding, the wide continuation, internal SP and trailing padding
    // each remain independently observable cells.
    for column in [1, 3, 5, 9] {
        let mut changed = buffer.clone();
        changed[(column, 0)].set_symbol("!");
        assert!(
            std::panic::catch_unwind(|| {
                assert_row(&changed, 0, 2, "界e\u{301}  Z", "changed padding");
            })
            .is_err(),
            "changed cell {column} must fail"
        );
    }
}
