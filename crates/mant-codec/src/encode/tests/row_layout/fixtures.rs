//! Shared row hints and Markdown event projection.
use super::super::{Event, Parser};
use mant_ir::{InlineLayout, RowLayoutHint};

pub(super) fn layout(rows: &[(u32, i32)]) -> InlineLayout {
    InlineLayout {
        row_hints: rows
            .iter()
            .map(|&(row, indent_columns)| RowLayoutHint {
                row,
                indent_columns,
            })
            .collect(),
    }
}

pub(super) fn visible(markdown: &str) -> String {
    Parser::new(markdown)
        .filter_map(|event| match event {
            Event::Text(value) | Event::Code(value) => Some(value.into_string()),
            Event::HardBreak | Event::SoftBreak => Some("\n".into()),
            _ => None,
        })
        .collect()
}
