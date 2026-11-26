//! Map overlapping terminal writes to the source owners still visible in each column.

use super::FieldCell;
use crate::mandoc::inline::flow::native_slots::FillSlot;
use std::ops::Range;

#[derive(Default)]
pub(super) struct Settlement {
    pub(super) spans: Vec<Range<usize>>,
    pub(super) trailing_blank_slots: Vec<(usize, usize)>,
}

/// `term.c::term_field` emits *each* backspace to the device. In an
/// unbreakable run of single-column glyphs the cursor can revisit a column
/// several times, leaving previously printed glyphs visible to its right.
/// Keep the native cells for field measurement and publish surviving owners
/// at the first slot for each column, in reading order.
pub(super) fn settle(slots: &mut [FillSlot]) -> Settlement {
    let mut settled = Settlement::default();
    let mut start = 0;
    for end in 0..=slots.len() {
        if slots.get(end).is_some_and(|slot| {
            matches!(slot.cell, FieldCell::Graph { width: 1, .. } if slot.count == 1)
                || slot.cell == FieldCell::Backline
        }) {
            continue;
        }
        let span = &mut slots[start..end];
        if has_repeated_backspace(span)
            && settle_columns(span, start, &mut settled.trailing_blank_slots)
        {
            settled.spans.push(start..end);
        }
        start = end + 1;
    }
    settled
}

fn has_repeated_backspace(span: &[FillSlot]) -> bool {
    let mut previous = false;
    for slot in span {
        let backspace = slot.cell == FieldCell::Backline;
        if backspace && (previous || slot.count > 1) {
            return true;
        }
        previous = backspace;
    }
    false
}

fn settle_columns(
    span: &mut [FillSlot],
    start: usize,
    trailing_blank_slots: &mut Vec<(usize, usize)>,
) -> bool {
    let mut columns = Vec::new();
    let mut cursor: usize = 0;
    for (index, slot) in span.iter().enumerate() {
        if slot.cell == FieldCell::Backline {
            // The preceding cell may belong to a different pass or a
            // multicolumn glyph. Only publish a locally proven ordering.
            let Some(previous) = cursor.checked_sub(slot.count) else {
                return false;
            };
            cursor = previous;
        } else {
            if cursor == columns.len() {
                columns.push((index, index, slot.owner));
            } else {
                columns[cursor].1 = index;
                columns[cursor].2 = slot.owner;
            }
            cursor += 1;
        }
    }
    // The first slot is only a reading-order address. Keep the native
    // address of a displaced trailing blank after surviving ink so its
    // generated seam cannot become a leading lexical space.
    if let [.., (_, ink, _), (first, last, Some(owner))] = columns.as_slice()
        && first < last
        && ink < last
        && matches!(span[*ink].cell, FieldCell::Graph { text, .. } if !text.is_whitespace())
        && matches!(span[*last].cell, FieldCell::Graph { text, .. } if text.is_whitespace())
    {
        trailing_blank_slots.push((*owner, start + *last));
    }
    for slot in span.iter_mut() {
        slot.owner = None;
    }
    for (index, _, owner) in columns {
        span[index].owner = owner;
    }
    true
}
