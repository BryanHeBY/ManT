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

//! Ordinary native input and the provisional semantic output it owns.

use super::super::{NativeWordAnchor, field_buffer::FieldBuffer};

/// The same ordered cell ledger used by definition fields, without field
/// geometry. A real `term_flushln()` consumes it; an IR owner drain does not.
#[derive(Clone, Default)]
pub(in crate::mandoc::inline::flow) struct PlainInputUnit {
    pub(in crate::mandoc::inline::flow) buffer: FieldBuffer,
    pub(in crate::mandoc::inline::flow) anchors: Vec<NativeWordAnchor>,
    pub(in crate::mandoc::inline::flow) output_start: usize,
}

impl PlainInputUnit {
    /// Establish a fresh output interval before the first word writes cells.
    pub(in crate::mandoc::inline::flow) fn begin_output_at(&mut self, start: usize) {
        if self.buffer.is_empty() && self.anchors.is_empty() {
            self.output_start = start;
        }
    }

    /// Move native input to a field or retirement without losing its cursor.
    /// Authorless fields still retire against this ordinary output interval.
    pub(in crate::mandoc::inline::flow) fn take_input(
        &mut self,
    ) -> (FieldBuffer, Vec<NativeWordAnchor>) {
        (
            std::mem::take(&mut self.buffer),
            std::mem::take(&mut self.anchors),
        )
    }

    /// Output indices cannot cross owners; the native cells and tab origin can.
    pub(in crate::mandoc::inline::flow) fn detach_output_owner(&mut self) {
        self.anchors.clear();
        self.output_start = 0;
    }

    /// Clear consumed input without changing its separately scoped tab origin.
    pub(in crate::mandoc::inline::flow) fn clear_input_at(&mut self, start: usize) {
        self.buffer.clear();
        self.anchors.clear();
        self.output_start = start;
    }

    /// An ordinary physical row retirement also resets the native tab origin.
    pub(in crate::mandoc::inline::flow) fn retire_row_at(&mut self, start: usize) {
        self.clear_input_at(start);
        self.buffer.set_tab_offset(0);
    }
}
