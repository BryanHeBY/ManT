//! Generated blank runs borrow the native buffer cursor without owning text.

use super::{EN, FieldBuffer, FieldCell, FieldWrite};

/// A bare tbl word has no live field owner during text projection. Replay
/// its same native writes once, incrementally, only when motion reads a
/// drawn space run. Direct words outside this case allocate no probe.
#[derive(Default)]
pub(in crate::mandoc::inline) struct DrawingProbe {
    buffer: FieldBuffer,
    observed: usize,
}

impl DrawingProbe {
    pub(in crate::mandoc::inline) fn observes_fixed_advance(
        &mut self,
        writes: &[FieldWrite],
    ) -> bool {
        self.buffer.apply_writes(&writes[self.observed..]);
        self.observed = writes.len();
        matches!(
            self.buffer.cells.last(),
            Some(FieldCell::FixedDrawingAdvance(_))
        )
    }
}

impl FieldBuffer {
    pub(super) fn draw_spaces(&mut self, count: usize) -> usize {
        let start = self.cells.len();
        // The first two encode1 calls retire BACKBEFORE and BACKAFTER.
        // After that the remaining SP form one contiguous device run.
        let lead = count.min(2);
        for _ in 0..lead {
            let retreat = std::mem::take(&mut self.backbefore_armed);
            match (retreat, self.cells.last()) {
                (
                    true,
                    Some(FieldCell::BreakableBlank | FieldCell::DrawnSpaces(_) | FieldCell::Tab),
                ) => {}
                (true, Some(_)) => {
                    self.push_cell(FieldCell::Backline);
                    self.push_cell(FieldCell::DrawnSpaces(1));
                }
                _ => self.push_cell(FieldCell::DrawnSpaces(1)),
            }
            self.backbefore_armed = std::mem::take(&mut self.backafter_armed);
            self.backbefore_graph = None;
            self.backline_projection_target = None;
        }
        if count > lead {
            self.push_cell(FieldCell::DrawnSpaces(count - lead));
        }
        start
    }

    pub(super) fn move_in_drawing(&mut self, start: usize, absolute: bool, units: i32) {
        // An HORIZ immediately after the drawing may address its generated
        // slots, including an unprojected backspace. Never rewind author
        // cells on this path.
        if std::mem::take(&mut self.backafter_armed) {
            return;
        }
        let extent = self.cells[start..].iter().fold(start, |width, cell| {
            width.saturating_add(match cell {
                FieldCell::DrawnSpaces(count) => *count,
                _ => 1,
            })
        });
        let current = i64::try_from(extent.saturating_mul(EN)).unwrap_or(i64::MAX);
        let units = i64::from(units);
        let delta = if absolute {
            units.saturating_sub(current)
        } else {
            units
        };
        if absolute || delta < 0 {
            self.proved_rejection = None;
        }
        if delta > 0 {
            // term_word() writes ASCII_NBRSP, not ordinary SP. Its first
            // fill counts this advance as graph, then normalizes the run.
            let cells = usize::try_from(delta)
                .unwrap_or(usize::MAX)
                .div_ceil(EN)
                .saturating_sub(usize::from(std::mem::take(&mut self.backbefore_armed)));
            if cells > 0 {
                self.push_cell(FieldCell::FixedDrawingAdvance(cells));
            }
            self.backbefore_graph = None;
            self.backline_projection_target = None;
            return;
        }
        if delta == 0 {
            return;
        }
        let retreat = usize::from(std::mem::take(&mut self.backbefore_armed));
        let slots = usize::try_from(delta.unsigned_abs() / EN as u64).unwrap_or(usize::MAX);
        let left = extent.saturating_sub(slots.saturating_add(retreat));
        if left < start || start < self.resume {
            return;
        }
        let mut width = extent;
        while width > left {
            let position = self.cells.len() - 1;
            self.invalidate_projection_scan(position);
            let cell = self.cells.last_mut().expect("drawing tail");
            if let FieldCell::DrawnSpaces(count) = cell
                && *count > width - left
            {
                *count -= width - left;
                break;
            }
            width -= match cell {
                FieldCell::DrawnSpaces(count) => *count,
                _ => 1,
            };
            if matches!(cell, FieldCell::DrawnSpaces(_)) {
                self.blank_positions.pop_last();
            }
            self.cells.pop();
            self.projection_prefix.pop();
            self.normalized_until = self.normalized_until.min(position);
        }
        if self
            .last_graph_position
            .is_some_and(|index| index >= self.cells.len())
        {
            self.last_graph_position = self.previous_graph_position.take();
        }
        self.word_first_content = self
            .word_first_content
            .filter(|index| *index < self.cells.len());
        self.backbefore_graph = None;
        self.backline_projection_target = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_blank_run_keeps_its_width_without_one_slot_per_stroke() {
        let mut buffer = FieldBuffer::default();
        let start = buffer.draw_spaces(80_000);
        assert!(buffer.cells().len() <= 3);
        buffer.move_in_drawing(start, false, -24);
        let remaining: usize = buffer
            .cells()
            .iter()
            .map(|cell| match cell {
                FieldCell::DrawnSpaces(count) => *count,
                _ => panic!("generated blank run only"),
            })
            .sum();
        assert_eq!(remaining, 79_999);
        assert_eq!(buffer.projection_length(0, buffer.cells().len()), 0);
    }
}
