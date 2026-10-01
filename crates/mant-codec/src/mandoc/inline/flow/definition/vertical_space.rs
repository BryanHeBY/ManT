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

//! Executed vertical rows and skip-space debt, independent of IR drains.

use super::super::InlineBuilder;

impl InlineBuilder {
    /// Execute a visited empty TEXT at its actual node position. Native
    /// `print_man_node()`/`print_mdoc_node()` call `term_newln()` for an active \c;
    /// otherwise `term_vspace()` consumes skipvsp before emitting a blank row.
    pub(in crate::mandoc) fn execute_visited_empty_text(&mut self, no_fill: bool) {
        if self.final_source_continuation_or(false) {
            self.execute_native_newline();
            // term_newln() does not clear TERMP_NONEWLINE. Another empty
            // source TEXT therefore also takes this branch, without adding
            // a vertical row or consuming skipvsp.
            self.continue_source_line(true);
        } else {
            // Empty TEXT calls term_vspace(), not roff_term_pre_sp().
            // Its resolved rows are recorded by that single execution
            // entry; it must not add a second receipt or execute pre_br.
            self.native_vertical_space_with_origin(
                1,
                if no_fill {
                    super::super::CompletedRowOrigin::LiteralText
                } else {
                    super::super::CompletedRowOrigin::Layout
                },
            );
        }
    }

    /// Execute `roff_term_pre_sp()` in source order: every requested vspace
    /// calls `term_newln()` before deciding whether `skipvsp` suppresses endline. The
    /// trailing `pre_br()` runs even for zero and negative requests. Resolving
    /// the emitted row count before execution would erase real field flushes.
    pub(in crate::mandoc) fn execute_spacing_request(&mut self, requested: i32) {
        if requested < 0 {
            self.execution.resolve_vertical_space(requested);
        } else {
            let requested = u16::try_from(requested).unwrap_or(u16::MAX);
            for _ in 0..requested {
                self.native_vertical_space(1);
            }
        }
        self.control_line_break();
    }

    pub(super) fn finish_native_vertical_row(&mut self, rows: usize) {
        if rows == 0 {
            return;
        }
        if let Some(definition) = &mut self.execution.definition {
            // term_vspace() emits an endline after the current field. Unlike
            // a HANG term_newln(), the next word starts a new device row.
            definition.hang_row.endline();
            definition.vertical_started_row = true;
        }
    }

    /// Plain `term_vspace()`, as used by `print_bvspace()`; unlike roff `.sp`,
    /// this does not execute `pre_br` or clear BRIND/NOBREAK afterwards.
    pub(in crate::mandoc) fn native_vertical_space(&mut self, rows: u16) {
        self.native_vertical_space_with_origin(rows, super::super::CompletedRowOrigin::Layout);
    }

    fn native_vertical_space_with_origin(
        &mut self,
        rows: u16,
        origin: super::super::CompletedRowOrigin,
    ) {
        self.execute_native_newline();
        let rows = self.execution.resolve_vertical_space(i32::from(rows));
        // NOBREAK/HANG can leave an already printed device row alive after
        // term_newln(). The first backend endline then closes that graph;
        // only later endlines complete empty rows (term.c:489-497). Observe
        // the device after the flush, not the pre-flush buffer or IR tail.
        let closes_printed_row = rows > 0 && self.execution.has_open_native_device_row();
        let completed_rows = rows.saturating_sub(u16::from(closes_printed_row));
        self.retain_line_breaks(usize::from(rows));
        self.finish_native_vertical_row(usize::from(rows));
        self.asserted_vertical_row |= completed_rows > 0;
        // term_vspace() already emitted these empty rows after resolving
        // skipvsp. They survive an output-owner return independently of
        // the ordinary row end from its leading term_newln().
        self.record_completed_rows(completed_rows, origin);
    }
}
