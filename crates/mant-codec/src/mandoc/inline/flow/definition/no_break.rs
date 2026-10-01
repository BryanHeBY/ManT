//! Persistent no-break fields and their generated separator ownership.
//! The native field stays live across `.mc`, source controls and output owners;
//! these methods consume its existing receipts without another interpreter.

use super::super::{
    AuthorBreakEffect, FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput,
    WordEndBreak, has_printable_character, last_visible_character, trim_trailing_breakable_spaces,
};
use super::device::{NativeFieldDevice, NativeFieldEmission};
use super::state::{DefinitionFieldStyle, NoBreakField};

impl InlineBuilder {
    /// A pending device separator has entered the active IR owner. Capture
    /// that precise range before the next word, without writing native cells
    /// or reviving a previously consumed `NoBreakField`.
    pub(in crate::mandoc::inline::flow) fn bind_materialized_field_separator(
        &mut self,
        start: usize,
        count: usize,
    ) {
        if let Some(field) = self
            .execution
            .definition
            .as_mut()
            .and_then(|state| state.no_break.as_mut())
        {
            field.output_end_before_separator = start;
            field.resumed_output_start = self.nodes.len();
            field.separator_cells = count;
        }
    }

    /// A prior NOBREAK flush left actual flags and future positioning behind.
    /// `term_newln()` consumes the next buffer under those same native flags;
    /// an output owner's Line effect does not bypass that execution.
    pub(super) fn begin_resumed_native_line(&mut self) -> Option<NativeFieldDevice> {
        if !self
            .execution
            .author_execution
            .as_ref()
            .is_some_and(|author| matches!(author.break_effect, AuthorBreakEffect::Line))
        {
            return None;
        }
        let field = self.execution.definition.as_ref()?.no_break?;
        let mut device =
            self.native_field_device_at(false, Some(field), field.resumed_output_start)?;
        self.retire_unprinted_no_break_separator(
            device.emission,
            device.separator_retention,
            device.separator_field,
            &mut device.output_start,
        );
        Some(device)
    }

    pub(super) fn retire_unprinted_no_break_separator(
        &mut self,
        emission: NativeFieldEmission,
        separator_retention: Option<usize>,
        consumed_field: Option<NoBreakField>,
        output_start: &mut usize,
    ) {
        let active_field = self
            .execution
            .definition
            .as_ref()
            .and_then(|state| state.no_break);
        let Some(mut field) = active_field.or(consumed_field) else {
            return;
        };
        if field.separator_cells == 0 {
            return;
        }
        let retained = if emission == NativeFieldEmission::Unprinted {
            0
        } else {
            separator_retention.unwrap_or(field.separator_cells)
        };
        let unprinted = field.separator_cells.saturating_sub(retained);
        if unprinted == 0 {
            return;
        }
        // term_field() keeps positioning blank until an accepted graph
        // actually prints (term.c:389-427). Only this generated separator's
        // saved prefix is provisional; authored suffix and identities stay.
        let old_start = field.resumed_output_start.min(self.nodes.len());
        let mut suffix = self.nodes.split_off(old_start);
        let separator_start = field.output_end_before_separator.min(self.nodes.len());
        let mut separator = self.nodes.split_off(separator_start);
        trim_trailing_breakable_spaces(&mut separator, unprinted);
        self.nodes.append(&mut separator);
        let new_start = self.nodes.len();
        let removed = old_start.saturating_sub(new_start);
        self.nodes.append(&mut suffix);
        field.output_end_before_separator = field.output_end_before_separator.min(new_start);
        field.resumed_output_start = new_start;
        field.separator_cells = retained;
        if active_field.is_some() {
            self.definition_state_mut().no_break = Some(field);
        }
        if let Some(author) = &mut self.execution.author_execution
            && author.field_output_start >= old_start
        {
            author.field_output_start = author.field_output_start.saturating_sub(removed);
        }
        if *output_start >= old_start {
            *output_start = output_start.saturating_sub(removed);
        }
    }

    pub(super) fn finish_resumed_native_line(&mut self, device: &NativeFieldDevice) {
        self.retire_native_field_with_device(Some(device));
        let start = self.nodes.len();
        let epoch = self.execution.execution_epoch;
        if let Some(field) = self.definition_state_mut().no_break.as_mut() {
            field.output_end_before_separator = start;
            field.resumed_output_start = start;
            field.resumed_execution_epoch = epoch;
            field.field_width = 0;
            field.separator_cells = 0;
        }
        // term_newln() sets NOSPACE even when both lastcol and viscol were
        // zero, so an empty flush cannot re-arm the next word's auto blank.
        self.execution.boundary = PendingBoundary::Tight;
    }

    /// Commit the current formatter cell without ending its visual row.
    ///
    /// The pinned CVS renderer uses this for `.mc`: pending `\z` content is
    /// materialized, while `TERMP_NOBREAK` keeps the next source word on the
    /// same line and clears `TERMP_NOSPACE`. Device margin geometry is outside
    /// the IR, so the next word observes one ordinary boundary.
    pub(in crate::mandoc) fn no_break_flush(&mut self) {
        // roff_term_pre_mc() only calls term_flushln() after the formatter
        // has advanced the current output column. A completed `\zX` glyph
        // has entered the buffer; a bare armed `\z` has not.
        let executed_field_word = self
            .definition
            .as_ref()
            .and_then(|state| state.no_break)
            .is_some_and(|field| self.execution.execution_epoch != field.resumed_execution_epoch);
        if !self.has_formatter_cell() && !executed_field_word {
            // A definitive rejection still occupied the native buffer:
            // roff_term_pre_mc() calls term_flushln() with NOBREAK held;
            // its reset (term.c:233-237) retires that unit even when no
            // printable cell survives in this output owner. Registers for
            // the dead buffer must not carry into the next formatter word.
            if self.execution.wipe_remainder {
                self.execution.zero_advance.discard_at_row_end();
                self.execution.wipe_remainder = false;
                self.execution.word_end_break = WordEndBreak::Clear;
                self.execution.word_end_break_separated = false;
                self.execution.row_zero_graph = false;
                self.execution.empty_word = false;
                self.execution.trailing_output = TrailingOutput::None;
                self.execution.pending_breakable_spaces = 0;
                self.execution.pending_field_spaces = 0;
                self.execution.formatter_column = FormatterColumn::Origin;
                self.execution.boundary = PendingBoundary::Ordinary;
            }
            return;
        }
        // term_fill() may accept a prefix and retire the rejected suffix's
        // cursor. Keep the input owner boundary of this actual flush before
        // either projection; accepted graph/origin receipts still belong to it.
        let consumed_output_start = self.native_field_output_start();
        if let Some(field) = self.take_no_break_field() {
            self.continue_no_break_definition_field(field);
            return;
        }
        // The first .mc flush is not yet a NoBreakField, but it still runs
        // term_fill() before changing NOBREAK/NOSPACE.
        // The plain unit dies with this flush as well (term.c:233-237):
        // `roff_term_pre_mc()` resets `tcol->buf` under NOBREAK, but the
        // reset consumes the unit's receipt first - a definitively
        // rejected suffix stays dead and its row event stays real.
        let _plain_flush_rejection = self.retire_plain_flush_unit();
        self.discard_unprinted_definition_field_output_no_break();
        if self.no_break_definition_field(consumed_output_start) {
            return;
        }
        let native = self.native_field_device_at(true, None, consumed_output_start);
        let device_row_continues = native.as_ref().is_none_or(|field| !field.ends_row);
        self.flush_zero_advance();
        // term.c:233-237 retires accepted and rejected buffers alike. Keep
        // the receipt's device position, but never re-feed consumed cells
        // or their projection ranges into the next formatter word. The
        // native reset also clears BACKAFTER armed after an accepted glyph;
        // only the empty-buffer early return above preserves a bare \z.
        self.execution.zero_advance.discard_at_row_end();
        if let Some(native) = &native {
            let row = &mut self.definition_state_mut().hang_row;
            row.flush(0);
            row.viscol = native.viscol;
            row.margin_flush_seen = true;
        }
        self.retire_native_field_with_device(native.as_ref());
        if !device_row_continues {
            // term.c:220 with 250-253 under NOBREAK: the overrun field ends
            // its device row inside this flush; the next word starts a new
            // one at the list offset.
            let row_indent = self.take_definition_row_indent();
            if !super::super::output::ends_with_executed_line_break(&self.nodes) {
                self.force_output_line_break();
            }
            if let Some(Inline::LineBreak { indent_columns }) = self.nodes.last_mut() {
                *indent_columns = row_indent;
            }
            self.note_definition_output_row();
            if let Some(definition) = &mut self.execution.definition {
                definition.hang_row.endline();
            }
            self.execution.last_visible_character = Some('\n');
        }
        if let Some(author) = &mut self.execution.author_execution {
            author.field_output_start = self.nodes.len();
        }
        self.execution.boundary = PendingBoundary::Ordinary;
        self.execution.empty_word = false;
        if let TrailingOutput::BreakableBlank(count) = self.execution.trailing_output {
            trim_trailing_breakable_spaces(&mut self.nodes, count);
        }
        self.execution.trailing_output = TrailingOutput::None;
        // An invisible formatter word still advanced `p->col`. Under `.mc`'s
        // `TERMP_NOBREAK` flush, a continued following word starts after that
        // cell even though there is no glyph to carry the distance in IR.
        // A `TERMP_NOBREAK` field owns one committed separator before the next
        // field.  It is formatter geometry, not a cancellable word boundary:
        // `.Sm off`, `.Ns`, and delimiter flags may suppress an additional
        // automatic blank but cannot move the already flushed field back.
        // `term_fill()` and `term_field()` discard trailing breakable word
        // padding before committing a NOBREAK field.  The field separator is
        // different formatter geometry: it survives `.Sm off`, `.Ns`, and
        // delimiter flags and replaces the next word's automatic boundary.
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 1;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.wipe_remainder = false;
        self.execution.row_zero_graph = false;
        self.execution.formatter_column = FormatterColumn::Origin;
        // `TERMP_NOBREAK` only changes this flush; it does not create
        // TERMP_NONEWLINE.  Preserve any already-executed `\c` continuation,
        // while releasing its tight word boundary like CVS clears NOSPACE.
        self.execution.final_word_join = Some(false);
    }

    /// Flush another formatter cell while a prior `.mc` field remains live.
    ///
    /// CVS clears NOBREAK and NOSPACE after each request, but BRIND/HANG,
    /// trailspace, and the list field geometry survive.  Consequently a later
    /// `.mc` must not fall back to the ordinary one-cell path merely because
    /// `AuthorBreakEffect` changed after the first flush.
    pub(super) fn continue_no_break_definition_field(&mut self, mut field: NoBreakField) {
        let native = self.native_field_device_at(true, Some(field), field.resumed_output_start);
        let resumed_has_cell = self.restore_no_break_field_projection(field, true);
        if let Some(native) = &native {
            let row = &mut self.definition_state_mut().hang_row;
            row.flush(field.trailspace_cells);
            row.viscol = native.viscol;
            row.margin_flush_seen = true;
        }
        if !resumed_has_cell {
            // Whitespace-only and zero-width formatter words make
            // `term_flushln()` run, but `term_fill()` commits no field:
            // the pass rejects with `nbr == 0` (term.c:143-146) and the
            // reset drops the whole unprinted remainder (term.c:233-237).
            // A pending `\p` marker cell must not survive that rejection
            // to arm a row break under the next word.
            let definition = self.definition_state_mut();
            if definition.field_buffer.only_ignorable_remainder(false) {
                definition.field_buffer.clear();
                definition.field_word_anchors.clear();
            }
            // Restore the one separator that was waiting for the next real
            // field instead of consuming it or manufacturing a second one.
            field.output_end_before_separator = self.nodes.len();
            self.append_field_separator(field.separator_cells);
            self.execution.boundary = PendingBoundary::CommittedField;
            field.resumed_output_start = self.nodes.len();
            field.resumed_execution_epoch = self.execution.execution_epoch;
            self.definition_state_mut().no_break = Some(field);
            self.reset_after_no_break_field();
            // The complete native field boundary is already represented by
            // the retained separator.  Do not let the block/source handoff
            // append a second ordinary word blank before the next field.
            self.execution.final_word_join = Some(true);
            return;
        }

        let resumed = self
            .nodes
            .get(field.resumed_output_start..)
            .unwrap_or_default();
        let resumed_width = native.as_ref().map_or_else(
            || mant_ir::geometry::text_width(&super::super::super::plain_text(resumed)),
            |field| field.width,
        );
        let overrun = native.as_ref().map_or_else(
            || {
                self.current_formatter_row_width()
                    .saturating_add(field.trailspace_cells)
                    > field.body_width
            },
            |field| field.overruns,
        );
        let output_end_before_separator = self.nodes.len();

        let separator_cells = if field.style == DefinitionFieldStyle::Tag && overrun {
            self.hard_break();
            // `roff_term_pre_mc()` clears NOSPACE after the NOBREAK flush,
            // so the first word on the new device row owns one ordinary
            // boundary. No word has written that cell yet: HEAD post may
            // close the empty buffer without printing another row.
            1
        } else if overrun {
            1
        } else {
            field.trailspace_cells.saturating_add(1)
        };
        if field.style == DefinitionFieldStyle::Tag && overrun {
            self.execution.pending_field_spaces = separator_cells;
        } else {
            self.append_field_separator(separator_cells);
        }
        self.execution.boundary = PendingBoundary::CommittedField;

        field.output_end_before_separator = output_end_before_separator;
        field.resumed_output_start = self.nodes.len();
        field.resumed_execution_epoch = self.execution.execution_epoch;
        field.field_width = resumed_width;
        field.separator_cells = separator_cells;
        self.definition_state_mut().no_break = Some(field);
        self.reset_after_no_break_field();
        if field.style == DefinitionFieldStyle::Tag && overrun {
            // reset_after_no_break_field() clears the previous field's
            // buffered geometry. The new row's separator belongs to its
            // *next* term_word(), so carry only that new pending cell on.
            self.execution.pending_field_spaces = separator_cells;
        }
    }

    pub(super) fn reset_after_no_break_field(&mut self) {
        self.execution.empty_word = false;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.wipe_remainder = false;
        self.execution.row_zero_graph = false;
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.final_word_join = Some(false);
    }

    pub(super) fn current_formatter_row_width(&self) -> usize {
        let start = self
            .nodes
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak { .. }))
            .map_or(0, |index| index + 1);
        mant_ir::geometry::text_width(&super::super::super::plain_text(&self.nodes[start..]))
    }

    pub(super) fn append_field_separator(&mut self, count: usize) {
        self.append_fixed_cells(count);
        if count > 0 {
            self.execution.trailing_output = TrailingOutput::FieldBlank(count);
        }
    }

    pub(super) fn no_break_definition_field(&mut self, consumed_output_start: usize) -> bool {
        let Some((start, gap, body, field_width_columns, flags)) = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            })
        else {
            return false;
        };
        let native = self.native_field_device_at(true, None, consumed_output_start);
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        let width = native.as_ref().map_or_else(
            || mant_ir::geometry::text_width(&super::super::super::plain_text(field)),
            |field| field.width,
        );
        if let Some(native) = &native {
            let row = &mut self.definition_state_mut().hang_row;
            row.flush(usize::from(gap));
            row.viscol = native.viscol;
            row.margin_flush_seen = true;
        }
        let overrun = native.as_ref().map_or(
            width.saturating_add(usize::from(gap)) > usize::from(body),
            |field| field.overruns,
        );
        // The consumed buffer and its row-origin annotations precede the
        // next field. Establish future owner cursors only after that exact
        // retirement, so private annotation insertion cannot move them.
        self.retire_native_field_with_device(native.as_ref());
        let output_end_before_separator = self.nodes.len();
        let mut materialized_separator_cells = 0;
        if flags.wraps() {
            materialized_separator_cells =
                self.position_no_break_separator(gap, overrun, native.as_ref());
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_field_exited();
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
                execution.break_effect = AuthorBreakEffect::Line;
            }
        } else {
            // HANG prevents the flush from ending the physical row.  A field
            // that still has room retains trailspace plus the ordinary next
            // word boundary; an overrun field retains only that boundary.
            self.execution.boundary = PendingBoundary::Ordinary;
            self.execution.pending_field_spaces = if overrun {
                1
            } else {
                usize::from(gap).saturating_add(1)
            };
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
            }
        }
        self.definition_state_mut().no_break = Some(NoBreakField {
            flags,
            output_end_before_separator,
            resumed_output_start: self.nodes.len(),
            resumed_execution_epoch: self.execution.execution_epoch,
            field_width: width,
            field_capacity_columns: field_width_columns,
            body_width: usize::from(body),
            trailspace_cells: usize::from(gap),
            separator_cells: if flags.wraps() {
                materialized_separator_cells
            } else if overrun {
                1
            } else {
                usize::from(gap).saturating_add(1)
            },
            style: if flags.wraps() {
                DefinitionFieldStyle::Tag
            } else {
                DefinitionFieldStyle::Hang
            },
        });
        self.execution.empty_word = false;
        self.execution.pending_breakable_spaces = 0;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.wipe_remainder = false;
        self.execution.row_zero_graph = false;
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.final_word_join = Some(false);
        true
    }

    /// minbl positions the next accepted graph, not an otherwise empty row.
    fn position_no_break_separator(
        &mut self,
        gap: u8,
        overrun: bool,
        native: Option<&NativeFieldDevice>,
    ) -> usize {
        let cells = if overrun {
            1
        } else {
            usize::from(gap).saturating_add(1)
        };
        if overrun || native.is_some_and(|field| field.viscol == 0) {
            if overrun {
                self.hard_break();
            }
            // Clearing NOSPACE leaves a pending boundary for term_word(),
            // not an occupied row before HEAD post. A marker can leave
            // viscol zero after accepting a prefix; term_field prints its
            // future minbl only when graph follows (term.c:397-427).
            self.execution.pending_field_spaces = cells;
            self.execution.boundary = PendingBoundary::CommittedField;
            0
        } else {
            self.append_field_separator(cells);
            self.execution.boundary = PendingBoundary::CommittedField;
            cells
        }
    }

    pub(super) fn take_no_break_field(&mut self) -> Option<NoBreakField> {
        if self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.no_break.is_some())
        {
            // Every request that flushes the field after .mc uses this
            // entrypoint. Consume term_fill()'s accepted prefix and reject
            // its pending suffix before .br/.sp/.ce or another .mc can move
            // the output owner or reset field flags.
            self.discard_unprinted_definition_field_output();
        }
        // A request consumes this buffer, not the item's BODY lifetime.
        // roff_term.c::roff_term_pre_mc() can leave viscol occupied, and
        // later words may arm BACKAFTER in a fresh buffer. The BODY post
        // still executes its own conditional term_newln().
        self.execution
            .definition
            .as_mut()
            .and_then(|state| state.no_break.take())
    }

    pub(super) fn restore_no_break_field_projection(
        &mut self,
        field: NoBreakField,
        force_no_break: bool,
    ) -> bool {
        // A prior `.mc` can leave the device row occupied (`viscol > 0`) even
        // after the current field buffer was reset.  Every control reaching
        // this path therefore executes a real `term_flushln()`: settle a
        // completed zero-advance glyph and, critically, clear a bare
        // BACKAFTER request before the next word runs.
        let native =
            self.native_field_device_at(force_no_break, Some(field), field.resumed_output_start);
        self.flush_zero_advance();
        let resumed = self
            .nodes
            .get(field.resumed_output_start..)
            .unwrap_or_default();
        let resumed_text = super::super::super::plain_text(resumed);
        // `term_fill()` commits a fixed/non-breaking blank glyph, but drops
        // an ordinary whitespace-only formatter word.  Both occupy the Rust
        // projection, so printable text alone cannot distinguish them.
        let resumed_has_cell = resumed_text.chars().any(|ch| !ch.is_whitespace())
            || (has_printable_character(resumed)
                && self.execution.trailing_output == TrailingOutput::FixedBlank);
        if !resumed_has_cell {
            // A pending-only next-word cell has not entered this Vec. Never
            // let its count trim a blank authored in the accepted prefix.
            let separator_start = field.output_end_before_separator.min(self.nodes.len());
            let mut provisional = self.nodes.split_off(separator_start);
            trim_trailing_breakable_spaces(&mut provisional, field.separator_cells);
            self.nodes.append(&mut provisional);
            let mut index = self.nodes.len();
            while index > field.output_end_before_separator {
                index -= 1;
                if matches!(&self.nodes[index], Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } if value.chars().all(char::is_whitespace))
                {
                    self.nodes.remove(index);
                }
            }
            self.execution.last_visible_character = last_visible_character(&self.nodes);
            self.execution.trailing_output = if self.execution.last_visible_character.is_some() {
                TrailingOutput::NonBlank
            } else {
                TrailingOutput::None
            };
        }
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        if native.is_some() {
            let row = &mut self.definition_state_mut().hang_row;
            row.flush(field.trailspace_cells);
        }
        self.retire_native_field_with_device(native.as_ref());
        resumed_has_cell
    }

    pub(super) fn settle_no_break_field_line(&mut self, field: NoBreakField, row_indent: u16) {
        // Sample before the restore: its flush settles a pending
        // zero-advance glyph and thereby closes the row (mdoc_term.c:1085).
        self.restore_no_break_field_projection(field, false);
        match field.style {
            DefinitionFieldStyle::Tag => {
                // roff_term.c:73-75: a fill-mode boundary (`.nf`/`.fi`, the
                // same pre_br dispatch, roff_term.c:52) moves the row origin
                // to the field's right margin; the `.br` family already
                // materializes it through the pending-indent arm below.
                if row_indent > 0 {
                    self.definition_state_mut().row.indent_columns = row_indent;
                }
                self.force_output_line_break();
                self.definition_state_mut().pending_indent = None;
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_field_exited();
            }
            DefinitionFieldStyle::Hang => {
                // HANG keeps this physical row open. pre_br changes its
                // offset; the next accepted print's receipt determines the
                // actual pad after all enclosing node geometry restores.
                self.execution.boundary = PendingBoundary::Tight;
            }
        }
        self.finish_definition_field_control(field, 0, true);
    }
}
