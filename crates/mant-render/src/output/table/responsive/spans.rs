//! Bounded ownership receipts for horizontal and vertical covered regions.
//! `tbl_data.c` counts vspans over data rows; rule rows do not consume them.
use super::{Row, TableRowKind};
#[derive(Clone, Copy, Default)]
struct Slot {
    start: usize,
    end: usize,
    rows: u32,
}
pub(super) struct Spans {
    slots: Vec<Slot>,
}
impl Spans {
    pub(super) fn new(count: usize) -> Self {
        Self {
            slots: vec![Slot::default(); count],
        }
    }
    pub(super) fn enter<T>(&mut self, row: &Row<'_, T>) {
        if !matches!(row.source.kind, TableRowKind::Data) {
            return;
        }
        for slot in &mut self.slots {
            slot.rows = slot.rows.saturating_sub(1);
        }
        for owner in &row.owners {
            if owner.row_span > 1 {
                let end = owner
                    .column
                    .saturating_add(owner.span)
                    .min(self.slots.len());
                for slot in &mut self.slots[owner.column..end] {
                    *slot = Slot {
                        start: owner.column,
                        end,
                        rows: owner.row_span,
                    };
                }
            }
        }
    }
    pub(super) fn interior(&self, boundary: usize) -> bool {
        self.slots
            .get(boundary)
            .is_some_and(|slot| slot.rows > 0 && slot.start < boundary && boundary < slot.end)
    }
    pub(super) fn rules(&self) -> Vec<bool> {
        self.slots.iter().map(|slot| slot.rows > 1).collect()
    }
}
