use super::*;

#[test]
fn preserved_public_ir_padding_keeps_plain_and_ansi_cursors_monotonic() {
    let block = declared_column_table(&[3, 3], &["X          ", "CLICK"]);
    let expected = "X          CLICK";
    let plain =
        super::super::super::plain_renderer().render_blocks(std::slice::from_ref(&block), 0);
    assert_eq!(plain, expected);
    let decorated = BlockRenderer {
        locations: None,
        names: None,
        decorate: &|_, text| format!("\u{1b}[1m{text}\u{1b}[0m"),
    };
    let text = decorated.render_blocks(&[block], 0);
    assert_eq!(
        text.replace("\u{1b}[1m", "").replace("\u{1b}[0m", ""),
        expected
    );
}

#[test]
fn decorated_columns_measure_before_ansi_styling() {
    // External audit §18.3: positioning must measure the projection
    // before the decorator adds zero-width ANSI styles; the decorator
    // contract only preserves visible text, never byte length.
    let decorated = BlockRenderer {
        locations: None,
        names: None,
        decorate: &|_, text| {
            if text.trim().is_empty() {
                text.to_owned()
            } else {
                format!("\x1b[1m{text}\x1b[0m")
            }
        },
    };
    let block = declared_column_table(&[10, 10], &["styled", "cells"]);
    let projected = decorated.render_blocks(std::slice::from_ref(&block), 0);
    let plain = super::super::super::plain_renderer().render_blocks(&[block], 0);
    let stripped: String = projected
        .split("\x1b[1m")
        .map(|rest| rest.replace("\x1b[0m", ""))
        .collect();
    assert_eq!(stripped, plain);
}
