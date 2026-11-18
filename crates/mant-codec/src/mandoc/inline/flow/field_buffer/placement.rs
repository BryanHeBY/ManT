//! Hand device-resolved writes to the same fill scanner as ordinary words.

use std::ops::Range;

use super::{FieldBuffer, FieldCell, FieldWrite};

impl FieldBuffer {
    pub(super) fn push_placed(&mut self, cell: &FieldCell, scalars: usize) {
        self.clear_backtracking();
        if *cell == FieldCell::BreakMarker {
            self.last_break_marker = Some(self.cells.len());
        }
        let previous = self.projection_prefix.last().copied().unwrap_or(0);
        self.push_cell(cell.clone());
        *self.projection_prefix.last_mut().expect("placed cell") = previous + scalars;
    }
}

/// A tbl input word executes independently. Consumer soft-wrap is still
/// responsive outside this explicit device-slot path.
pub(in crate::mandoc::inline) fn isolated_passes(
    writes: &[FieldWrite],
    table_field_units: Option<usize>,
) -> Vec<(Range<usize>, bool)> {
    let mut field = FieldBuffer::default();
    field.apply_writes(writes);
    let mut passes = Vec::new();
    let target = table_field_units.unwrap_or(usize::MAX / 4);
    while let Some(pass) = field.scan_fill_pass(target, true) {
        let end = field.printed_content_end(pass.start, pass.end);
        let closes = field.has_non_ignorable_after(pass.end, false);
        passes.push((pass.start..end, closes));
        if !closes {
            break;
        }
        field.advance_past(pass.end);
        field.consume_break_blanks();
    }
    passes
}
