//! Locate authored non-whitespace labels across real responsive visual rows.

use mant_ui::RenderedDocument;
use unicode_width::UnicodeWidthStr;

pub(super) fn fragments(
    rendered: &RenderedDocument,
    rows: std::ops::Range<usize>,
    label: &str,
) -> Vec<(String, usize, usize)> {
    assert!(!label.chars().any(char::is_whitespace));
    let mut cells = Vec::new();
    for row in rows {
        let text = rendered.text.lines[row].to_string();
        let mut column = 0;
        for grapheme in mant_render::cells::graphemes(&text) {
            if !grapheme.text().chars().all(char::is_whitespace) {
                cells.push((grapheme.text().to_owned(), row, column));
            }
            column += grapheme.columns();
        }
    }
    let length = mant_render::cells::graphemes(label).count();
    let matches = cells
        .windows(length)
        .filter(|window| {
            window
                .iter()
                .map(|cell| cell.0.as_str())
                .collect::<String>()
                == label
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "wrapped body label {label:?}");
    let mut fragments: Vec<(String, usize, usize)> = Vec::new();
    for (text, row, column) in matches[0] {
        if let Some(last) = fragments.last_mut()
            && last.1 == *row
        {
            assert_eq!(last.2 + last.0.width(), *column);
            last.0.push_str(text);
        } else {
            fragments.push((text.clone(), *row, *column));
        }
    }
    fragments
}
