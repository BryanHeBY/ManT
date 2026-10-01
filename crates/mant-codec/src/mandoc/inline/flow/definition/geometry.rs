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

//! Definition geometry checkpoints and HEAD/BODY row-origin projection.

use super::super::native_field::FieldFlag;
use super::super::{
    AuthorBreakEffect, FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput,
    has_printable_character,
};
use super::state::{DefinitionFieldStyle, HangRowTransition};

impl InlineBuilder {
    /// `term.c::encode1()` buffers the character following `\\z` in the
    /// current field. It has not reached an IR node yet, but `term_fill()` and
    /// `term_field()` still count its printed width at a field boundary.
    pub(super) fn pending_hang_glyph_width(&self) -> Option<usize> {
        let mut zero_advance = self.execution.zero_advance.clone();
        let mut pending = Vec::new();
        zero_advance.finish_into(&mut pending);
        let text = super::super::super::plain_text(&pending);
        text.chars()
            .any(|ch| !ch.is_whitespace() || ch == '\u{a0}')
            .then(|| mant_ir::geometry::text_width(&text))
    }

    /// A HANG head that filled its capacity while a request had cleared
    /// `TERMP_NOBREAK` reaches the body column with no trailspace
    /// (term.c:250-253 with 205-207): the body's first word concatenates.
    pub(in crate::mandoc) fn cleared_field_filled_capacity(&self) -> bool {
        let Some(state) = &self.execution.definition else {
            return false;
        };
        if !state.no_break_cleared
            || state.cleared_field_capacity_columns == 0
            || state.hang_row.field_discretionary_break
        {
            return false;
        }
        // HEAD post prints using the current stops, offset and margin.
        // term_fill()'s old capacity or cumulative width cannot prove the
        // BODY origin after a .ta or an internal wrap (term.c:113-253).
        let body_origin = state.native_margin_units.unwrap_or_else(|| {
            usize::from(state.cleared_field_capacity_columns).saturating_mul(24)
        });
        self.native_field_device(false).is_some_and(|field| {
            let column = field.viscol.saturating_mul(24);
            !field.ends_row
                && (column > body_origin || column == body_origin && field.final_pass_continued)
        })
    }

    /// Whether an author-split row end restarted the head field (its
    /// breaks do not close the head before BODY).
    pub(in crate::mandoc) fn definition_author_restarted(&self) -> bool {
        self.execution
            .definition
            .as_ref()
            .is_some_and(|state| state.outcome.is_field_restarted())
    }

    pub(in crate::mandoc) fn definition_field_exited(&self) -> bool {
        self.execution.definition.as_ref().is_some_and(|state| {
            state.outcome.field_exited()
                || matches!(
                    self.execution
                        .author_execution
                        .as_ref()
                        .map(|author| author.break_effect),
                    Some(AuthorBreakEffect::Field { .. })
                ) && self
                    .native_field_device(false)
                    .is_some_and(|field| field.ends_row)
        })
    }

    pub(in crate::mandoc::inline::flow) fn reset_native_tab_origin(&mut self) {
        // term_newln() resets taboff even if there was no buffer to flush.
        if let Some(state) = &mut self.execution.definition {
            state.field_buffer.set_tab_offset(0);
        }
    }

    pub(in crate::mandoc) fn definition_body_gap_consumed(&self) -> bool {
        let Some(state) = &self.execution.definition else {
            return false;
        };
        if !state.hang_row.margin_flush_seen
            && !state.field_buffer.is_empty()
            && (state.hang_row.transition == HangRowTransition::WordAfterFlush
                || state.no_break_cleared)
            && let Some(AuthorBreakEffect::Field {
                body_width_columns,
                gap_cells,
                flags,
                ..
            }) = self
                .execution
                .author_execution
                .as_ref()
                .map(|author| author.break_effect)
            && flags.contains(FieldFlag::Hang)
        {
            // At HEAD post, term_newln() flushes the final HANG field. Only
            let mut final_row = state.hang_row.clone();
            if let Some(width) = self.pending_hang_glyph_width() {
                final_row.pending_glyph(width);
            }
            let body_column = usize::from(body_width_columns);
            if final_row.field_discarded {
                // No new glyph reached the device. The preceding field's
                // viscol and minbl still locate BODY; source text inside the
                // discarded field cannot create a soft-wrap uncertainty.
                // A rejected final field produced no new device graph and
                // cannot supersede a separator already represented by the
                // preceding committed field. Node geometry restoration
                // may have reset offset before this final flush.
                return state.outcome.body_gap_consumed()
                    || final_row.final_column() >= body_column;
            }
            // term.c:156-229 prints the unconsumed field using the current
            // tab stops and may finish on a later physical row. Its actual
            // device column, not the widths accumulated before a .ta or a
            // field wrap, is the only column that can prove BODY's origin.
            let cumulative_column = self
                .native_field_device(false)
                .map_or_else(|| final_row.final_column(), |field| field.viscol);
            // CVS term.c::term_fill() may wrap at a breakable cell *inside*
            // this final field. Its summed width then says nothing about the
            // last physical row. Retain the word boundary unless the field
            // provably stayed on one row (or had no breakable cell).
            let final_row_proven = !final_row.field_unproven_break
                && (cumulative_column <= body_column
                    || (!final_row.field_discretionary_break
                        && (final_row.field_last_unbreakable_width >= body_column
                            || !final_row.field_breakable)));
            return gap_cells == 0 && final_row_proven && cumulative_column >= body_column;
        }
        state.outcome.body_gap_consumed()
    }

    pub(in crate::mandoc::inline::flow) fn append_fixed_cells(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.append_projected(vec![Inline::Text {
            value: " ".repeat(count),
        }]);
        self.execution.trailing_output = TrailingOutput::FixedBlank;
    }

    /// Execute `.ti` through its preceding `roff_term_pre_br()` boundary.
    ///
    /// `ManT` deliberately omits the device-specific temporary offset.  In
    /// particular, the numeric operand is not printable padding: the pinned
    /// renderer applies it to `p->ti` and `tcol->offset` only after flushing
    /// the current field.  Retain the field's own trailspace and boundary,
    /// then discard the temporary device position as documented.
    pub(in crate::mandoc) fn temporary_indent(&mut self) {
        if let Some(field) = self.take_no_break_field() {
            self.restore_no_break_field_projection(field, false);
            match field.style {
                DefinitionFieldStyle::Tag => {
                    self.force_output_line_break();
                    self.execution
                        .definition
                        .as_mut()
                        .expect("definition field session")
                        .outcome
                        .mark_field_exited();
                }
                DefinitionFieldStyle::Hang => {
                    // `term_newln()` leaves NOSPACE set.  HANG keeps the
                    // device row alive, so a following control-only author
                    // transition must not synthesize an ordinary word blank
                    // between the prior field and its eventual head text.
                    self.execution.boundary = PendingBoundary::Tight;
                    self.execution.pending_field_spaces = 1;
                }
            }
            self.finish_definition_field_control(field, 0, false);
            return;
        }
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
            self.control_line_break();
            return;
        };
        if !self.has_formatter_cell() {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            return;
        }
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        if !has_printable_character(field) {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            return;
        }
        let field_width = mant_ir::geometry::text_width(&super::super::super::plain_text(field));
        let overruns =
            flags.wraps() && field_width.saturating_add(usize::from(gap)) > usize::from(body);
        if overruns {
            self.hard_break();
        } else {
            self.append_fixed_cells(usize::from(gap));
        }
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
                    body_width_columns: body,
                    field_width_columns,
                    flags,
                };
            }
        }
        self.execution.boundary = PendingBoundary::Tight;
    }

    /// Collapse a jump still uncommitted at the item post: the element
    /// restore already zeroed the offset (mdoc_term.c:437-439), so the
    /// buffered word prints with `vbl = 0`.
    pub(in crate::mandoc) fn retract_head_close_jump(&mut self) {
        if let Some(definition) = &mut self.execution.definition
            && let Some(jump_node) = definition.row.retract_on_head_close()
            && let Some(Inline::Text { value }) = self.nodes.get_mut(jump_node)
        {
            value.clear();
        }
    }

    /// The row origin survives each flush until a document scope restores it.
    /// CVS `mdoc_term.c::print_mdoc_node()` saves offset after the source-line
    /// event and restores it at non-roff node exit (329, 393-397, 437-439).
    pub(in crate::mandoc) fn take_definition_row_indent(&mut self) -> u16 {
        self.execution
            .definition
            .as_ref()
            .map_or(0, |definition| definition.row.indent_columns)
    }

    pub(in crate::mandoc) fn definition_geometry_checkpoint(
        &self,
        node: &libmandoc_rs::Node,
    ) -> Option<super::DefinitionGeometryCheckpoint> {
        self.execution.definition_geometry_checkpoint(node)
    }

    pub(in crate::mandoc) fn restore_definition_geometry(
        &mut self,
        checkpoint: Option<super::DefinitionGeometryCheckpoint>,
    ) {
        self.execution.restore_definition_geometry(checkpoint);
    }

    /// Assign origin when the buffered row actually prints. The last word
    /// can print after its enclosing scope restored offset, while earlier
    /// rows have already been committed by source-line events.
    pub(in crate::mandoc) fn commit_definition_row_origin(&mut self) {
        fn set_last_break(nodes: &mut [Inline], origin: u16) -> bool {
            for node in nodes.iter_mut().rev() {
                match node {
                    Inline::LineBreak { indent_columns } => {
                        *indent_columns = origin;
                        return true;
                    }
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => {
                        if set_last_break(children, origin) {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            false
        }
        if self.has_formatter_cell()
            && let Some(definition) = &self.execution.definition
            && definition.row.has_pending_origin()
            && definition.field_buffer.resume_offset() < definition.field_buffer.cells().len()
        {
            set_last_break(&mut self.nodes, definition.row.indent_columns);
            self.definition_state_mut().row.retire_row_origin();
        }
    }

    pub(in crate::mandoc) fn note_definition_output_row(&mut self) {
        if let Some(definition) = &mut self.execution.definition {
            definition.row.note_row_origin();
        }
    }

    pub(in crate::mandoc) fn force_output_line_break(&mut self) {
        // Consume the pending row indent even when a break already sits at
        // the tail: the boundary moved the upstream row origin regardless
        // (roff_term.c:73-75), and a leaked indent would misplace a later
        // row.
        let row_indent = self.take_definition_row_indent();
        if matches!(self.nodes.last(), Some(Inline::LineBreak { .. })) {
            return;
        }
        self.flush_zero_advance();
        self.nodes.push(Inline::line_break_indented(row_indent));
        self.note_definition_output_row();
        self.execution.last_visible_character = Some('\n');
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.boundary = PendingBoundary::Ordinary;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        self.execution.formatter_column = FormatterColumn::Origin;
    }
}

impl super::super::InlineExecutionState {
    /// Apply the native origin selected by a display pre-handler. The IR
    /// display indentation is independent: only field consumption uses this
    /// offset, and the node checkpoint restores it after the real post.
    pub(in crate::mandoc) fn add_native_display_offset(&mut self, columns: usize) {
        self.add_native_display_offset_units(
            i32::try_from(columns.saturating_mul(24)).unwrap_or(i32::MAX),
        );
    }

    /// Bd BODY uses a signed basic-unit offset; negative values stop at the
    /// native page floor (mdoc_term.c:1449-1455). No projected width is read.
    pub(in crate::mandoc) fn add_native_display_offset_units(&mut self, units: i32) {
        if let Some(definition) = &mut self.definition {
            definition.field_offset_units = definition
                .field_offset_units
                .saturating_add_signed(isize::try_from(units).unwrap_or_default());
            definition.hang_row.field_offset =
                definition.field_offset_units.saturating_add(11) / 24;
        }
    }

    /// A consumed field may leave its physical device row open under
    /// NOBREAK/HANG (term.c:233-253). This is independent of IR ownership
    /// and of whether the next source node is filled or no-fill.
    pub(in crate::mandoc) fn has_open_native_device_row(&self) -> bool {
        self.definition
            .as_ref()
            .is_some_and(|definition| definition.hang_row.viscol > 0)
    }

    pub(in crate::mandoc) fn definition_geometry_checkpoint(
        &self,
        node: &libmandoc_rs::Node,
    ) -> Option<super::DefinitionGeometryCheckpoint> {
        // Roff requests return before the geometry restore. Text restores
        // rmargin only; this ledger records the offset relevant to reading.
        if self.macro_set != libmandoc_rs::MacroSet::Mdoc
            || node.kind == libmandoc_rs::NodeKind::Text
            || node
                .macro_name
                .as_deref()
                .is_some_and(|name| name.as_bytes().first().is_some_and(u8::is_ascii_lowercase))
        {
            return None;
        }
        self.definition
            .as_ref()
            .map(|definition| super::DefinitionGeometryCheckpoint {
                indent_columns: definition.row.indent_columns,
                field_offset: definition.hang_row.field_offset,
                field_offset_units: definition.field_offset_units,
                margin_override: definition.margin_override,
            })
    }

    pub(in crate::mandoc) fn restore_definition_geometry(
        &mut self,
        checkpoint: Option<super::DefinitionGeometryCheckpoint>,
    ) {
        if let Some(checkpoint) = checkpoint
            && let Some(definition) = &mut self.definition
        {
            definition.row.indent_columns = checkpoint.indent_columns;
            definition.hang_row.field_offset = checkpoint.field_offset;
            definition.field_offset_units = checkpoint.field_offset_units;
            definition.margin_override = checkpoint.margin_override;
        }
    }
}
