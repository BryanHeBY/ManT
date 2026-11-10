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

//! Commit consumed device positions and retire the field's input ownership.

use super::super::{device::NativeFieldDevice, flush::FieldFlushBoundary};
use crate::mandoc::inline::flow::{InlineBuilder, output::row_origins};

impl InlineBuilder {
    /// Project resolved positions before their scalar coordinates retire.
    pub(in crate::mandoc::inline::flow) fn project_consumed_row_layout(
        &mut self,
        device: &NativeFieldDevice,
    ) -> [row_origins::OutputNodeEdits; 2] {
        self.execution.row_presentation = device.presentation;
        self.execution.partial_row_alignment |= device.partial_alignment;
        if let Some(receiver) = self
            .execution
            .definition
            .as_ref()
            .and_then(|state| state.deferred_head_projection.as_ref())
        {
            receiver.accept(&device.row_presentations);
            if let Some(receipt) = &device.cleared_alignment {
                receiver.accept(std::slice::from_ref(receipt));
            }
        }
        let cleared = if let Some(receipt) = &device.cleared_alignment {
            // One bounded old-owner metadata revocation per device row.
            // Text rejection still uses the new field's output_start.
            let edits = row_origins::project_row_presentations(
                &mut self.nodes,
                std::slice::from_ref(receipt),
                0,
            );
            self.remap_output_projection(&edits);
            edits
        } else {
            row_origins::OutputNodeEdits::default()
        };
        let mut output_start = device.output_start;
        cleared.remap(&mut output_start);
        let presented = row_origins::project_row_presentations(
            &mut self.nodes,
            &device.row_presentations,
            output_start,
        );
        self.remap_output_projection(&presented);
        [cleared, presented]
    }

    pub(in crate::mandoc::inline::flow) fn retire_native_field_with_device(
        &mut self,
        device: Option<&NativeFieldDevice>,
    ) {
        self.retire_native_field_with_device_at(device, FieldFlushBoundary::Continue);
    }

    /// A real `term_flushln()` commits this input field irreversibly. Keep
    /// device viscol/minbl and the enclosing BODY lifetime, but retire its
    /// cells and projection ranges before another formatter word executes.
    pub(in crate::mandoc::inline::flow::definition) fn retire_native_field_with_device_at(
        &mut self,
        device: Option<&NativeFieldDevice>,
        boundary: FieldFlushBoundary,
    ) {
        #[cfg(test)]
        super::super::control_trace::retired();
        if let Some(device) = device {
            self.capture_consumed_run_in_boundaries(device);
            let tabs = self.capture_consumed_native_tabs(device);
            let layout_edits = self.project_consumed_row_layout(device);
            let mut output_start = device.output_start;
            let mut separator_field = device.separator_field;
            for edits in layout_edits {
                edits.remap(&mut output_start);
                if let Some(field) = &mut separator_field {
                    field.remap_output_projection(&edits);
                }
            }
            self.retire_unprinted_no_break_separator(
                device.emission,
                device.separator_retention,
                separator_field,
                &mut output_start,
            );
            let edits = row_origins::project_native_positions_with_tabs(
                &mut self.nodes,
                &device.row_origins,
                &device.field_padding,
                tabs,
                output_start,
                self.execution
                    .definition
                    .as_ref()
                    .is_some_and(|state| state.column_origin_units.is_none()),
            );
            self.remap_output_projection(&edits);
            // All native acceptance and layout receipts still address the
            // original scalar stream. Resolve TAB text only after those
            // receipts have been consumed, at this real field retirement.
            if let Some(state) = &self.execution.definition
                && let Some(receiver) = &state.deferred_head_projection
            {
                receiver.accept_tabs(tabs);
            }
            if let Some(author) = &mut self.execution.author_execution {
                author.field_output_start = self.nodes.len();
            }
        }
        if self.execution.has_column_output_scope()
            && boundary != FieldFlushBoundary::ColumnPost
            && device.is_some_and(|device| device.ends_row && device.printed_row.is_some())
        {
            self.record_device_row_end();
        }
        if self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| !state.field_buffer.is_empty())
        {
            self.note_native_input_retired();
        }
        if let Some(state) = &mut self.execution.definition {
            if let Some(device) = device {
                state.field_buffer.set_tab_offset(device.tab_offset);
                // Every real flush owns the final device position, including
                // a detached HEAD post. NOBREAK may leave that printed row
                // occupied after the input cells have retired (term.c:250-253).
                state.hang_row.viscol = device.viscol;
                state.hang_row.page_origin_printed =
                    !device.ends_row && device.printed_row.is_some();
                state.hang_row.minbl = device.next_field_gap_cells;
                // Geometry return retires no printed device content. Real
                // flushes atomically transfer their row receipt, while a
                // real endline consumes the current row's origin advances.
                state.hang_row.unprojected_origin_units = if device.ends_row {
                    0
                } else {
                    device.unprojected_origin_units
                };
            }
            state.field_buffer.clear_consumed_field();
            state.no_pad = false;
            state.field_word_anchors.clear();
        }
    }
}
