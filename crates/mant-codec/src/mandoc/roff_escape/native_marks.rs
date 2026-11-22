//! Device operands whose printed glyphs and native buffer writes differ.

use super::{RoffInlineEvent, decode};
use libmandoc_rs::TableColumnMetrics;

const EN: usize = 24;

pub(in crate::mandoc) struct LineStroke {
    pub(in crate::mandoc) glyph: char,
    pub(in crate::mandoc) width: usize,
    pub(in crate::mandoc) count: usize,
}

/// Resolve the repetition of one stroke in term.c's `ESCAPE_HLINE` arm.
/// Nonpositive lengths use the actual available field width.
pub(in crate::mandoc) fn horizontal_line_stroke(
    argument: &str,
    available_units: usize,
) -> Option<LineStroke> {
    let (units, suffix) = TableColumnMetrics::motion_argument(argument)?;
    let length = usize::try_from(units)
        .ok()
        .filter(|units| *units > 0)
        .unwrap_or(available_units);
    if length == 0 {
        return None;
    }
    let glyph = line_glyph(suffix);
    let width = mant_ir::geometry::text_width(glyph.encode_utf8(&mut [0; 4]));
    (width > 0).then(|| LineStroke {
        glyph,
        width,
        count: length.div_ceil(width * EN),
    })
}

fn line_glyph(suffix: &str) -> char {
    let glyph = if suffix.starts_with('\\') {
        decode(suffix).first().and_then(|event| match event {
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _) => value.chars().next(),
            _ => None,
        })
    } else {
        suffix.chars().next().filter(char::is_ascii)
    };
    glyph.filter(|glyph| !glyph.is_control()).unwrap_or('_')
}

/// The `ESCAPE_OVERSTRIKE` loop skips the reverse-solidus byte but keeps
/// consuming the remaining operand. Raw trailing input still arms BACK
/// after the current write, even when no graphic operand follows.
pub(in crate::mandoc) fn overstrike_operands(source: &str) -> impl Iterator<Item = (char, bool)> {
    source.char_indices().filter_map(|(offset, glyph)| {
        (glyph != '\\').then_some((glyph, offset + glyph.len_utf8() < source.len()))
    })
}
