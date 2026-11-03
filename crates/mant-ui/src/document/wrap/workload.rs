//! Deterministic source inspection bounds, independent of machine timing.

use std::cell::Cell;

use super::*;

thread_local! {
    static VISITS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn visit(count: usize) {
    VISITS.with(|visits| visits.set(visits.get() + count));
}

#[test]
fn wrapping_consumes_long_suffixes_without_revisiting_the_prefix() {
    for length in [1024, 4096, 16384] {
        for value in ["x".repeat(length), " ".repeat(length), "x ".repeat(length)] {
            for mode in [WrapMode::Word, WrapMode::Character] {
                for width in [1, 7, 80] {
                    let line = LogicalLine::plain(0, &value, Style::default()).wrap_mode(mode);
                    VISITS.with(|visits| visits.set(0));
                    let rows = wrap_line_with_links(&line, width);
                    let visits = VISITS.with(Cell::get);
                    assert!(
                        visits <= 8 * value.len(),
                        "{length} {mode:?} {width}: {visits}"
                    );
                    if mode == WrapMode::Character || !value.contains(' ') {
                        let actual: String = rows
                            .iter()
                            .flat_map(|row| &row.line.spans)
                            .map(|span| span.content.as_ref())
                            .collect();
                        assert_eq!(actual, value);
                    }
                }
            }
        }
    }
}
