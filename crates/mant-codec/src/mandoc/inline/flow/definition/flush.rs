use super::super::native_field::{FieldFlags, row_continues};
use super::super::{
    AuthorBreakEffect, FormatterColumn, InlineBuilder, PendingBoundary, TrailingOutput,
    WordEndBreak, has_printable_character, trim_trailing_breakable_spaces,
};
use super::device::NativeFieldDevice;
use super::state::{HangRowTransition, PendingFieldGapOrigin};

pub(super) use super::super::output::retain_inline_identities as retain_unprinted_field_targets;

/// A real source/request/post flush delivers its row boundary to the active
/// owner. The final column field tail instead has table-wide placement.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FieldFlushBoundary {
    Continue,
    ExitField,
    ColumnPost,
}

/// Configuration of one real field flush, before pre-br changes its flags.
#[derive(Clone, Copy)]
pub(super) struct FieldFlush {
    pub(super) start: usize,
    pub(super) gap: u8,
    pub(super) body: u16,
    pub(super) width: u16,
    pub(super) flags: FieldFlags,
    pub(super) boundary: FieldFlushBoundary,
}

/// `minbl` survives a graphless flush (term.c:235), but `term_field()` only
/// prints it with a later graph (397-434). Within an already printed row it
/// is a real word separator; at an unprinted origin the responsive layout
/// already represents positioning and must not add authored-looking cells.
fn projected_next_field_gap(device: &NativeFieldDevice) -> usize {
    if device.has_occupied_row() {
        device.next_field_gap_cells
    } else {
        0
    }
}

impl InlineBuilder {
    // The NOBREAK flush commits field, row, and BRIND state in native order.
    pub(super) fn flush_definition_field_at(
        &mut self,
        field_output_start: usize,
        gap_cells: u8,
        body_width_columns: u16,
        field_width_columns: u16,
        flags: FieldFlags,
        boundary: FieldFlushBoundary,
    ) -> bool {
        let native = self.native_field_device(false);
        self.flush_captured_definition_field(
            FieldFlush {
                start: field_output_start,
                gap: gap_cells,
                body: body_width_columns,
                width: field_width_columns,
                flags,
                boundary,
            },
            native,
        )
    }

    /// Consume one old-state receipt. This method owns it and cannot obtain
    /// a second tail decision after flags or the projection have changed.
    pub(super) fn flush_captured_definition_field(
        &mut self,
        field: FieldFlush,
        mut native: Option<NativeFieldDevice>,
    ) -> bool {
        let FieldFlush {
            start: mut field_output_start,
            gap: gap_cells,
            body: body_width_columns,
            width: field_width_columns,
            flags,
            boundary,
        } = field;
        let exit_field = boundary == FieldFlushBoundary::ExitField;
        if let Some(edit) = self.commit_definition_row_origin() {
            edit.remap(&mut field_output_start);
            if let Some(device) = &mut native {
                device.remap_output_positions(edit);
            }
        }
        if let Some(device) = &native {
            self.project_captured_definition_field(device);
        } else {
            self.discard_unprinted_definition_field_output();
        }
        let had_open_device_row = self.execution.has_open_native_device_row();
        if let Some(device) = &mut native {
            // Rejection projection may have advanced the author cursor to
            // its accepted tail. This real flush still owns its original
            // stable-word interval, captured by the entry's argument.
            device.output_start = field_output_start;
        }
        let had_marker_passes = self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.transition != HangRowTransition::Initial);
        let native_field_discarded = self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.field_discarded);
        let native_field_printable = !flags.wraps()
            && !native_field_discarded
            && (self
                .execution
                .definition
                .as_ref()
                .is_some_and(|state| state.hang_row.field_printable)
                || self.pending_hang_glyph_width().is_some());
        self.flush_native_hang_field(gap_cells, body_width_columns, exit_field);
        if let Some(native) = &native {
            self.definition_state_mut().hang_row.viscol = native.viscol;
        }
        if !self.has_formatter_cell() {
            if had_open_device_row {
                self.settle_empty_device_field(native.as_ref(), boundary);
            }
            // `roff_term_pre_br()` applies BRIND even when `term_newln()` had
            // no tcol bytes or device row to flush.  It moves the next word
            // to the body margin and clears NOBREAK/BRIND.  HANG itself
            // survives, so a hang head and its body remain on that same row;
            // a tag head instead finishes as an ordinary line field.
            if exit_field {
                self.exit_empty_definition_field(body_width_columns, field_width_columns, flags);
            }
            return false;
        }
        let (field_is_printable, field_width) = self.observe_flushed_definition_field(
            field_output_start,
            native.as_ref(),
            native_field_discarded,
            native_field_printable,
            flags,
        );
        let body_width = usize::from(body_width_columns);
        let overruns = flags.wraps()
            && native.as_ref().map_or(
                field_is_printable
                    && field_width.saturating_add(usize::from(gap_cells)) > body_width,
                |field| field.overruns,
            );

        let mut deferred_field_cells = 0;
        // NOBREAK can have been cleared by a preceding roff request while
        // the same HEAD remains active. term_flushln() then closes even a
        // fitting TAG field (term.c:250-253); overrun alone is insufficient.
        let ends_row = native.as_ref().map_or(overruns, |field| field.ends_row);
        if ends_row {
            self.hard_break_after_field_projection(native.as_ref());
        } else if field_is_printable {
            if !exit_field {
                // term_flushln() retains trailspace as minbl. A following
                // formatter word materializes it, while roff_term_pre_br()
                // or an overrun empty column post can retire it first. An
                // output-owner drain cannot eagerly print these cells.
                deferred_field_cells = usize::from(gap_cells);
            }
        } else if exit_field {
            self.exit_unprinted_definition_field(had_marker_passes, flags);
        } else {
            // term_flushln() restores minbl from trailspace even when
            // term_fill() accepted no graph. Only a later formatter word or
            // roff_term_pre_br() decides whether those device cells print.
            deferred_field_cells = native.as_ref().map_or(0, projected_next_field_gap);
            self.execution.trailing_output = TrailingOutput::None;
        }

        self.reset_after_definition_field_flush(
            deferred_field_cells,
            body_width_columns,
            field_width_columns,
            flags,
            boundary,
        );
        self.finish_definition_field_flush(native.as_ref(), boundary);
        overruns
    }

    fn exit_unprinted_definition_field(&mut self, had_marker_passes: bool, flags: FieldFlags) {
        // An explicit empty word and `\&` still execute the NOBREAK
        // field.  There is no row to close, but `roff_term_pre_br()`
        // applies BRIND before the following word.
        //
        // term.c:250-252: when an earlier pass already restarted at the
        // right margin (BRIND), the remaining field budget is zero and a
        // NOBREAK field without HANG closes the row before BODY: the
        // accepted prefixes stay on their own rows and the body starts a
        // new one. HANG ignores the overrun and keeps the shared row.
        // `hang_row.viscol` already carries the `field_offset` floor
        // from this field's own flush, so the restart signal is the
        // pre-flush transition: only accepted or rejected in-word
        // `term_fill()` passes leave it non-initial.
        if had_marker_passes && !row_continues(flags, 0, 0) {
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_field_exited();
            if let Some(execution) = &mut self.execution.author_execution {
                execution.break_effect = AuthorBreakEffect::Line;
            }
        } else {
            // pre_br updates offset without printing padding.
            // A later real flush may run after Xo restored that offset;
            // only its accepted print receipt can establish the row origin.
            self.definition_state_mut().pending_indent = None;
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_body_gap_consumed();
        }
    }

    fn finish_definition_field_flush(
        &mut self,
        native: Option<&NativeFieldDevice>,
        boundary: FieldFlushBoundary,
    ) {
        // term.c:233-237: the committed flush ends the field; the input
        // buffer restarts empty for whatever follows this row.
        self.retire_native_field_with_device_at(native, boundary);
    }

    fn observe_flushed_definition_field(
        &mut self,
        field_output_start: usize,
        native: Option<&NativeFieldDevice>,
        native_field_discarded: bool,
        native_field_printable: bool,
        flags: FieldFlags,
    ) -> (bool, usize) {
        self.flush_zero_advance();
        let field = self.nodes.get(field_output_start..).unwrap_or_default();
        // term_fill() returns nbr=0 for a HANG field containing only \p,
        // ordinary breakable blanks, or invisible controls. Its IR padding
        // may look printable, but term_flushln() keeps the device row open.
        let field_is_printable = if native_field_discarded {
            false
        } else if flags.wraps() {
            has_printable_character(field)
        } else {
            native_field_printable
        };
        let field_width = native.map_or_else(
            || mant_ir::geometry::text_width(&super::super::super::plain_text(field)),
            |field| field.width,
        );
        if !flags.wraps() && !field_is_printable {
            // term_fill() returns nbr=0 for a HANG field with only ordinary
            // blanks and controls. Drop only this field's breakable padding:
            // the fixed cells from the preceding field still position BODY.
            let mut unprinted = self
                .nodes
                .split_off(field_output_start.min(self.nodes.len()));
            trim_trailing_breakable_spaces(&mut unprinted, usize::MAX);
            self.nodes.extend(unprinted);
        }
        (field_is_printable, field_width)
    }

    fn reset_after_definition_field_flush(
        &mut self,
        deferred_field_cells: usize,
        body_width_columns: u16,
        field_width_columns: u16,
        flags: FieldFlags,
        boundary: FieldFlushBoundary,
    ) {
        let exit_field = boundary == FieldFlushBoundary::ExitField;
        self.execution.boundary = PendingBoundary::Tight;
        self.execution.empty_word = false;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = deferred_field_cells;
        self.execution.pending_field_gap_origin = PendingFieldGapOrigin::Other;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.wipe_remainder = false;
        self.execution.row_zero_graph = false;
        self.execution.formatter_column = FormatterColumn::Origin;
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            if exit_field && flags.wraps() {
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_field_exited();
                execution.break_effect = AuthorBreakEffect::Line;
            } else if exit_field {
                // TERMP_HANG survives term_newln; pre_br changes its future
                // offset and clears trailspace. Only a later accepted print
                // receipt can turn that positioning into output cells.
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_body_gap_consumed();
                execution.break_effect = AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns,
                    field_width_columns,
                    flags,
                };
            }
        }
    }

    pub(super) fn exit_empty_definition_field(
        &mut self,
        body_width_columns: u16,
        field_width_columns: u16,
        flags: FieldFlags,
    ) {
        // A source-line term_newln() may have already flushed this
        // field and left its trailspace for the next word. An
        // explicit .br consumes that pending gap while BRIND moves
        // the offset; it does not print another field's padding.
        self.definition_state_mut().pending_indent = None;
        self.execution.pending_field_spaces = 0;
        // `roff_term_pre_br()` changes the device offset even for
        // an empty field. It does not advance `p->viscol`; the
        // offset was recorded in `hang_row.field_offset` above.
        self.execution.boundary = PendingBoundary::Tight;
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            if flags.wraps() {
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_field_exited();
                execution.break_effect = AuthorBreakEffect::Line;
            } else {
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_body_gap_consumed();
                execution.break_effect = AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns,
                    field_width_columns,
                    flags,
                };
            }
        }
    }

    fn settle_empty_device_field(
        &mut self,
        native: Option<&NativeFieldDevice>,
        boundary: FieldFlushBoundary,
    ) {
        let exit_field = boundary == FieldFlushBoundary::ExitField;
        // term_newln(lastcol || viscol) flushes an empty buffer
        // against the occupied device row. Its tail can close that
        // row even under NOBREAK; the following term_vspace then
        // owns a genuinely empty endline (term.c:475-497).
        if native.is_some_and(|device| device.ends_row) {
            self.force_output_line_break();
            self.execution.pending_field_spaces = 0;
        }
        self.execution.zero_advance.discard_at_row_end();
        self.retire_native_field_with_device_at(native, boundary);
        if !exit_field {
            // term_flushln() still restored minbl from trailspace,
            // even though lastcol was empty (term.c:233-253).
            self.execution.pending_field_spaces = native.map_or(0, projected_next_field_gap);
        }
    }

    pub(super) fn flush_native_hang_field(&mut self, gap: u8, body_width: u16, exit_field: bool) {
        let had_cell = self.has_formatter_cell();
        let pending_glyph_width = self.pending_hang_glyph_width();
        if let Some(definition) = &mut self.execution.definition {
            if let Some(width) = pending_glyph_width {
                definition.hang_row.pending_glyph(width);
            }
            if had_cell
                || pending_glyph_width.is_some()
                || definition.hang_row.native_row_occupied()
            {
                definition.hang_row.flush(usize::from(gap));
            }
            if exit_field {
                definition.hang_row.field_offset = usize::from(body_width);
                definition.field_offset_units = definition
                    .native_margin_units
                    .unwrap_or_else(|| usize::from(body_width).saturating_mul(24));
            }
        }
    }
}
