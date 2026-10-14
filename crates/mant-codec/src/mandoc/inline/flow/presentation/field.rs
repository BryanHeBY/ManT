// Copyright (c) 2010-2022, 2025, 2026 Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! Source-relative device placement, distinct from responsive public alignment.

/// The same placement drives fill capacity and its accepted device print.
#[derive(Clone, Copy)]
pub(in crate::mandoc) struct FieldPlacement {
    pub(in crate::mandoc) projection_padding: usize,
    pub(super) padding: i64,
    pub(super) target_consumption: i64,
    pub(super) printed_shift: i32,
}

impl FieldPlacement {
    /// `term_flushln()`:150-160 advances the device before printing. The
    /// native 78-column default is used only to execute field/word rules;
    /// public alignment remains responsive to each consumer's viewport.
    pub(in crate::mandoc) fn aligned(
        mut self,
        alignment: mant_ir::TextAlignment,
        width: usize,
        margin: usize,
        parent: i32,
        occupied: bool,
    ) -> Self {
        if alignment == mant_ir::TextAlignment::Left {
            return self;
        }
        let screen = usize::try_from((78 * 24 - parent).max(0)).unwrap_or_default();
        let available = self.capacity(margin.min(screen));
        let remaining = available.saturating_sub(width);
        let extra = match alignment {
            mant_ir::TextAlignment::Center => remaining / 2,
            mant_ir::TextAlignment::Right => remaining,
            mant_ir::TextAlignment::Left => 0,
        };
        let extra = i64::try_from(extra).unwrap_or(i64::MAX);
        let baseline = if occupied { 0 } else { i64::from(parent) };
        let old_column = baseline
            .saturating_add(self.padding)
            .saturating_add(11)
            .div_euclid(24);
        let new_column = baseline
            .saturating_add(self.padding)
            .saturating_add(extra)
            .saturating_add(11)
            .div_euclid(24);
        let columns = i32::try_from(new_column.saturating_sub(old_column)).unwrap_or(i32::MAX);
        self.printed_shift = self
            .printed_shift
            .saturating_add(columns.saturating_mul(24));
        if occupied && columns > 0 && self.projection_padding == 0 {
            // An actual same-row field separator is semantic evidence.
            // Keep one word-boundary cell without copying device padding.
            self.projection_padding = 24;
            self.printed_shift = self.printed_shift.saturating_sub(24);
        }
        self
    }

    pub(in crate::mandoc) fn capacity(self, margin: usize) -> usize {
        let remaining = i64::try_from(margin)
            .unwrap_or(i64::MAX)
            .saturating_sub(self.target_consumption)
            .max(0);
        usize::try_from(remaining).unwrap_or(usize::MAX)
    }
}
