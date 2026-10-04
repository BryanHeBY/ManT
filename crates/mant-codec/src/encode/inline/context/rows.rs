//! One source-row cursor shared by transparent annotation traversal.

use mant_ir::{InlineContentRef, InlineLayout};

pub(in crate::encode::inline) struct RowCursor<'a> {
    layout: &'a InlineLayout,
    row: usize,
    at_row_start: bool,
}

impl Default for RowCursor<'_> {
    fn default() -> Self {
        Self {
            layout: InlineContentRef::unpositioned(&[]).layout,
            row: 0,
            at_row_start: true,
        }
    }
}

impl<'a> RowCursor<'a> {
    pub(in crate::encode::inline) fn owner(&mut self, layout: &'a InlineLayout) {
        self.layout = layout;
        self.row = 0;
        // A Joined seam retains the physical row while changing its local
        // owner coordinate. A first-row hint cannot introduce a word gap.
    }

    pub(in crate::encode::inline) fn pending_padding(&self) -> usize {
        if self.at_row_start {
            mant_ir::geometry::padding(self.layout.row_indent(self.row))
        } else {
            0
        }
    }

    pub(in crate::encode::inline) fn visible(&mut self) -> String {
        let padding = self.pending_padding();
        self.at_row_start = false;
        "&#160;".repeat(padding)
    }

    pub(in crate::encode::inline) fn newline(&mut self) {
        self.row = self.row.saturating_add(1);
        self.at_row_start = true;
    }
}
