//! Drain IR ownership after the live formatter's real boundary executes.

use super::super::definition::{NativeFieldDevice, PreservedDefinitionField};
use super::super::{
    FormatterColumn, Inline, InlineBuilder, InlineExecutionState, PendingBoundary,
    PreservedInlineState, WordEndBreak, has_printable_character,
};
use super::projection::{
    finalize_inline_output, has_non_whitespace_glyph, retain_inline_identities,
    trim_output_terminators,
};

impl InlineBuilder {
    pub(in crate::mandoc) fn finish(mut self) -> Vec<Inline> {
        self.flush_zero_advance();
        self.finish_nodes()
    }

    /// A definition HEAD hands its completed source rows to the item layout.
    /// Only a paragraph terminator may discard a trailing `LineBreak`.
    pub(in crate::mandoc) fn finish_preserving_rows(mut self) -> Vec<Inline> {
        self.flush_zero_advance();
        self.drain_ir_nodes()
    }

    /// Drain one IR paragraph without ending the formatter's execution
    /// lifetime.  Only paragraph-local joins and row projections reset; the
    /// source formatter's keep, author, spacing, and vertical-space registers
    /// remain in this same execution state.
    pub(in crate::mandoc) fn take_paragraph_segment(
        &mut self,
        line_request: bool,
    ) -> (Vec<Inline>, bool, u16) {
        // term_newln() consumes first, then the output owner accounts for
        // every row in its receipt (term.c:143-146,220,250-253). Doing this
        // in finish_nodes() is too late: the block row count is already
        // detached, and trimming would erase the rejected pass's endline.
        if self.retire_plain_flush_unit() {
            self.record_completed_vertical_rows(1);
            self.execution.word_end_break = WordEndBreak::Clear;
            self.execution.formatter_column = FormatterColumn::Origin;
        }
        // finish_nodes() trims a trailing break because it normally ends an
        // IR paragraph. If another invisible cell is already active after
        // that break, the break closed a real earlier row (term_newln()) and
        // must remain between the two cells.
        let carry_armed_zero_advance = !self.has_formatter_cell();
        let armed = carry_armed_zero_advance && self.execution.zero_advance.take_armed();
        let empty_word_end_break = self.take_unrepresented_word_end_break();
        self.flush_zero_advance();
        let mut invisible_formatter_cell = self.has_invisible_formatter_cell();
        // .br and the final pre_br() phase of .sp close an occupied native
        // cell before this output owner is drained. A preceding visible row
        // can already end in LineBreak while the current empty-word cell has
        // no glyph of its own; it still needs its own completed row. A bare
        // pending \p is already transferred through empty_word_end_break,
        // so it cannot also claim a completed empty-word row.
        let requested_invisible_row = line_request
            && invisible_formatter_cell
            && !empty_word_end_break
            && !self.external_head_row_pending;
        let trailing_invisible_rows = if requested_invisible_row {
            let mut rows = 0u16;
            let mut row_has_glyph = false;
            for node in &self.nodes {
                if matches!(node, Inline::LineBreak { .. }) {
                    rows = if row_has_glyph {
                        0
                    } else {
                        rows.saturating_add(1)
                    };
                    row_has_glyph = false;
                } else {
                    row_has_glyph |= has_non_whitespace_glyph(std::slice::from_ref(node));
                }
            }
            rows
        } else {
            0
        };
        // Trailing LineBreak nodes own already closed rows; the current
        // invisible cell is a later row. A bare \p was excluded above and
        // is transferred separately through empty_word_end_break.
        let executed_tail_rows = super::projection::trailing_completed_row_receipts(&self.nodes);
        let mut completed_vertical_rows = executed_tail_rows
            .saturating_add(u16::from(requested_invisible_row))
            .saturating_add(trailing_invisible_rows.saturating_sub(executed_tail_rows));
        if completed_vertical_rows > 0 {
            // term_vspace() has already emitted these rows. Remove only their
            // trailing inline projection, including a later invisible word
            // cell; the block owner will carry the completed rows across the
            // structural split. Ordinary trailing term_newln() stays trimable.
            // term_fill() can end a row whose only buffered cells are
            // breakable blanks from a visited whitespace-only TEXT. Those
            // cells are printable to our inline model, yet still form an
            // additional physical row after term_vspace().
            let active_invisible_cell = self.execution.formatter_column
                == FormatterColumn::Advanced
                && !self
                    .nodes
                    .iter()
                    .rev()
                    .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
                    .any(|node| has_non_whitespace_glyph(std::slice::from_ref(node)));
            completed_vertical_rows = completed_vertical_rows
                .saturating_add(u16::from(active_invisible_cell && !requested_invisible_row));
            // The completed-row owner now accounts for this cell. The old
            // invisible-word fallback must not manufacture it a second time.
            invisible_formatter_cell &= !active_invisible_cell;
            let mut identities = Vec::new();
            while self.nodes.last().is_some_and(|node| {
                matches!(node, Inline::LineBreak { .. })
                    || !has_non_whitespace_glyph(std::slice::from_ref(node))
            }) {
                if let Some(node) = self.nodes.pop() {
                    identities.push(node);
                }
            }
            identities.reverse();
            retain_inline_identities(&mut identities);
            self.nodes.extend(identities);
        }
        let completed_invisible_row =
            invisible_formatter_cell && matches!(self.nodes.last(), Some(Inline::LineBreak { .. }));
        let mut children = self.finish_nodes();
        if completed_invisible_row {
            children.push(Inline::line_break());
        }
        if invisible_formatter_cell && !empty_word_end_break && !has_printable_character(&children)
        {
            children.push(Inline::Text {
                value: String::new(),
            });
        }
        self.execution.reset_paragraph_segment(armed);
        (children, empty_word_end_break, completed_vertical_rows)
    }

    /// Finish one native formatter line and return a bare `\\z` request only
    /// when no formatter cell was occupied.
    ///
    /// CVS `term_newln()` calls `term_flushln()` only after text, a pending
    /// glyph, or another formatter event occupied the current cell.  That
    /// flush clears both backtracking flags.  An otherwise empty line leaves
    /// `TERMP_BACKAFTER` live for the next formatter word.  Returning the
    /// surviving flag together with the committed output prevents callers
    /// from exporting state before this boundary has executed.
    pub(in crate::mandoc) fn finish_formatter_line(
        mut self,
        preserve_rows: bool,
    ) -> (Vec<Inline>, InlineExecutionState, bool) {
        self.commit_definition_row_origin();
        // This is the HEAD post's real term_newln()/term_flushln(), not an
        // owner drain. Capture before acceptance advances projection ranges;
        // geometry is restored already, and pending glyphs enter their owner
        // before that receipt projects actual same-row padding and origins.
        let native = self.native_field_device(false);
        // The mdoc HEAD ledger has the list's actual pad/break flags, so
        // its accepted NBRZW field can prove a physical close even without
        // an IR glyph (term_fill():340-349, term_flushln():250-253).
        // Man's detached HEAD uses its established pre/post contract:
        // pre_IP/pre_TP set NOBREAK before post flush (man_term.c:525-668).
        // Its ordinary-line scratch ledger is not evidence of that close.
        let invisible_native_row_end = self.execution.macro_set == libmandoc_rs::MacroSet::Mdoc
            && self
                .execution
                .definition
                .as_ref()
                .is_some_and(|state| !state.field_buffer.is_empty())
            && native.as_ref().is_some_and(NativeFieldDevice::ends_row)
            && !has_printable_character(&self.nodes)
            && !self.execution.zero_advance.has_buffered_glyph();
        // The actual HEAD post receipt also supplies its occupied row to
        // BODY; observing it cannot require a second numeric field sweep.
        let occupied_head_row = native
            .as_ref()
            .is_some_and(NativeFieldDevice::has_occupied_row);
        let native_tail_end = self.discard_unprinted_definition_field_output();
        let surviving_armed = if self.has_formatter_cell() {
            false
        } else {
            self.execution.zero_advance.take_armed()
        };
        self.flush_zero_advance();
        self.retire_native_field_with_device(native.as_ref());
        if native_tail_end || invisible_native_row_end {
            self.nodes.push(Inline::line_break());
            self.note_definition_output_row();
        }
        let output = if preserve_rows {
            self.execution.word_end_break = WordEndBreak::Clear;
            self.drain_ir_nodes()
        } else {
            self.finish_nodes()
        };
        self.execution.reset_paragraph_segment(surviving_armed);
        // This API is an actual term_newln(), unlike an IR owner drain.
        // NOSPACE is selected even when the flushed field left viscol live
        // (term.c:475-481), independently of the preserved column registers.
        self.execution.boundary = PendingBoundary::Tight;
        self.reset_native_tab_origin();
        (output, self.execution, occupied_head_row)
    }

    /// Execute the native BODY post selected by `FormatterRowBoundary::Settle`.
    /// An IR paragraph drain or a Preserve return must not invoke this.
    pub(in crate::mandoc) fn finish_run_in_field_row(&mut self) {
        if self.execution.definition.is_none() && self.execution.detached_device_row.is_some() {
            // A fitting detached HEAD already consumed its input buffer,
            // leaving only viscol/minbl live. The BODY post still executes
            // term_newln() over that occupied device row (term.c:475-481).
            self.hard_break();
            return;
        }
        if !self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.run_in_continuation)
        {
            return;
        }
        // term_newln()477-480 calls term_flushln() only for a buffered cell
        // or an occupied device row. A bare BACKAFTER is neither: retiring
        // this field owner must leave it live for the next formatter word.
        let flushes_buffer =
            self.has_formatter_cell()
                || self.execution.definition.as_ref().is_some_and(|state| {
                    !state.field_buffer.is_empty() || state.hang_row.viscol > 0
                });
        if flushes_buffer {
            // A no-break flush may have committed HEAD glyphs while leaving
            // their device row occupied (term.c:250-253). A graphless BODY
            // tail closes that same represented row, rather than emitting a
            // second blank row in the description's output owner.
            let closes_represented_head = self.external_head_row_pending
                && self
                    .execution
                    .definition
                    .as_ref()
                    .is_some_and(|state| state.hang_row.viscol > 0);
            let native = self.native_field_device(false);
            self.commit_definition_row_origin();
            self.flush_zero_advance();
            let extra_row_end = self.discard_unprinted_definition_field_output();
            self.retire_native_field_with_device(native.as_ref());
            self.record_completed_vertical_rows(u16::from(
                extra_row_end && !closes_represented_head,
            ));
        }
        if let Some(state) = &mut self.execution.definition {
            state.field_buffer.clear();
            state.field_word_anchors.clear();
            state.run_in_continuation = false;
        }
        // term_flushln()235-237 clears the consumed buffer for every field,
        // including NOBREAK fields whose device row remains open. That old
        // buffer is not a new invisible cell owned by the draining BODY.
        if flushes_buffer {
            self.execution.formatter_column = FormatterColumn::Origin;
            self.execution.word_end_break = WordEndBreak::Clear;
            self.execution.zero_advance.discard_at_row_end();
        }
        self.reset_native_tab_origin();
    }

    /// Return an inner scope without forcing a pending `\\z` glyph to become
    /// visible.  CVS mandoc carries its backtracking flags through nested
    /// `term_word()` calls, so the caller must continue the state in the
    /// surrounding inline stream before committing it at a real boundary.
    pub(in crate::mandoc) fn finish_preserving_execution(
        mut self,
    ) -> (Vec<Inline>, PreservedInlineState, InlineExecutionState) {
        self.commit_definition_row_origin();
        self.project_definition_owner_prefix();
        if let Some(definition) = &mut self.execution.definition {
            definition.row.retire_row_origin();
            definition.field_buffer.detach_projection_owner();
        }
        let formatter_cell_occupied = self.has_formatter_cell();
        // term.c keeps one NOBREAK field active across the whole list item
        // (mdoc_term.c::termp_it_pre() through the item's BODY post). When
        // the HEAD session ends while that field is still configured, carry
        // it instead of retiring it with the drained output owner.
        let definition_field = self
            .execution
            .definition
            .take()
            .zip(
                self.execution
                    .author_execution
                    .as_ref()
                    .map(|author| author.break_effect),
            )
            .map(|(state, author_effect)| PreservedDefinitionField {
                state,
                author_effect,
            });
        let mut state = PreservedInlineState {
            zero_advance: std::mem::take(&mut self.execution.zero_advance),
            word_end_break: self.execution.word_end_break == WordEndBreak::Pending,
            definition_field,
            source_continuation: self.execution.final_source_continuation,
            formatter_cell_occupied,
            pending_line_indent: self.execution.pending_line_indent,
            pending_definition_indent: self.pending_definition_indent(),
            last_executed_source_line: self.execution.last_executed_source_line,
        };
        // A scope return is not term_newln(). In particular, an authored
        // br/sp at the end of an inset HEAD closed its last physical row;
        // keep that break in the detached term so the BODY cannot run in.
        self.execution.word_end_break = WordEndBreak::Clear;
        let output = self.drain_ir_nodes();
        if let Some(field) = &mut state.definition_field {
            field.state.field_word_anchors.clear();
        }
        // The plain flush unit carries across the scope return like the
        // preserved field's cells, but its IR anchors and output interval
        // belong to the drained owner and cannot cross it.
        self.execution.flush_unit_anchors.clear();
        self.execution.flush_unit_output_start = 0;
        // The output owner ended, but CVS term_word() still sees the same
        // physical row and word separator after an inset/diag HEAD. Keep its
        // registers, retiring only offsets into the drained node vector.
        self.execution.retire_output_owner();
        (output, state, self.execution)
    }

    fn finish_nodes(&mut self) -> Vec<Inline> {
        self.execution.word_end_break = WordEndBreak::Clear;
        // The paragraph terminator is a term_newln() over the plain flush
        // unit: consume the unit's full receipt before the segment drains -
        // a definitively rejected suffix is trimmed, and its nbr == 0 pass
        // still ended a native row (term.c:143-146 with 233-237 and
        // 250-253). That row event must survive the terminator trim: the
        // empty row precedes whatever the next flow emits.
        let retired_rejected_row = self.retire_plain_flush_unit();
        if self.execution.definition.is_none() {
            self.execution.flush_unit.clear();
            self.execution.flush_unit_anchors.clear();
            self.execution.flush_unit_output_start = 0;
        }
        if !super::projection::trailing_device_row_end_receipt(&self.nodes) {
            trim_output_terminators(&mut self.nodes);
        }
        let _ = retired_rejected_row;
        self.drain_ir_nodes()
    }

    fn drain_ir_nodes(&mut self) -> Vec<Inline> {
        if let Some(definition) = &mut self.execution.definition {
            definition.row.retire_row_origin();
        }
        let mut nodes = std::mem::take(&mut self.nodes);

        finalize_inline_output(&mut nodes);
        nodes
    }
}

#[cfg(test)]
#[path = "drain/tests.rs"]
mod tests;
