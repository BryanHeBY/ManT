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
};
use super::device::NativeFieldDevice;

impl InlineBuilder {
    /// Read the actual HEAD post flush receipt before retiring its IR owner.
    /// `man_term.c::post_IP/post_TP` call `term_flushln` directly; only viscol
    /// left on an open row can make a later `term_newln` flush the HEAD again.
    pub(in crate::mandoc) fn definition_head_row_occupied(&self) -> bool {
        self.native_field_device(false)
            .is_some_and(|device| device.has_occupied_row())
    }
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

    /// A cleared field's actual open row can reach BODY without a separator.
    /// `term_flushln()` retains viscol and assigns minbl from trailspace;
    /// `term_field()` prepends max(offset - viscol, minbl), not a gap derived
    /// from the number of fill passes (term.c:113-116,233-253,389-427).
    pub(in crate::mandoc) fn cleared_field_reaches_body_without_gap(
        &self,
        native: Option<&NativeFieldDevice>,
    ) -> bool {
        let Some(state) = &self.execution.definition else {
            return false;
        };
        if !state.no_break_cleared {
            return false;
        }
        // HEAD post prints using the current stops, offset and margin.
        // term_fill()'s old capacity or cumulative width cannot prove the
        // BODY origin after a .ta or an internal wrap (term.c:113-253).
        // An explicitly measured Some(0) origin is valid (width=-2n plus
        // termp_it_pre's two cells), not a missing-field sentinel.
        let body_origin = state.native_margin_units.unwrap_or_else(|| {
            usize::from(state.cleared_field_capacity_columns).saturating_mul(24)
        });
        native.is_some_and(|field| {
            let column = field.viscol.saturating_mul(24);
            // ascii_advance() prints a cell only beyond half an EN from
            // the current device column (term_ascii.c:299-304). A positive
            // basic-unit remainder can therefore still mean no word gap.
            field.has_occupied_row()
                && field.next_field_gap_cells == 0
                && column.saturating_add(12) >= body_origin
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

    pub(in crate::mandoc) fn definition_field_exited(
        &self,
        native: Option<&NativeFieldDevice>,
    ) -> bool {
        self.execution.definition.as_ref().is_some_and(|state| {
            state.outcome.field_exited()
                || matches!(
                    self.execution
                        .author_execution
                        .as_ref()
                        .map(|author| author.break_effect),
                    Some(AuthorBreakEffect::Field { .. })
                ) && native.is_some_and(|field| field.ends_row)
        })
    }

    pub(in crate::mandoc::inline::flow) fn reset_native_tab_origin(&mut self) {
        // term_newln() resets taboff even if there was no buffer to flush.
        if let Some(state) = &mut self.execution.definition {
            state.field_buffer.set_tab_offset(0);
        }
    }

    pub(in crate::mandoc) fn definition_body_gap_consumed(
        &self,
        native: Option<&NativeFieldDevice>,
    ) -> bool {
        let Some(AuthorBreakEffect::Field {
            body_width_columns,
            gap_cells,
            flags,
            ..
        }) = self
            .execution
            .author_execution
            .as_ref()
            .map(|author| author.break_effect)
        else {
            return self
                .execution
                .definition
                .as_ref()
                .is_some_and(|state| state.outcome.body_gap_consumed());
        };
        if !flags.contains(FieldFlag::Hang) {
            return false;
        }
        // HEAD post flushes the final buffer after enclosing nodes restored
        // offset. Only that receipt locates BODY; an earlier .mc or empty
        // pre-br field cannot permanently consume its eventual gap.
        // At the page origin, the device's common left margin already covers
        // minbl (term.c:113-116); it is not another projected separator.
        // The captured print already applied max(offset - viscol, minbl).
        // Observing its responsive layout must not replace the native minbl
        // register or execute another field pass (term.c:113-116,233-253).
        native.is_some_and(|field| {
            gap_cells == 0 && !field.ends_row && field.viscol >= usize::from(body_width_columns)
        })
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
        // pre_ti always calls pre_br before inspecting its optional operand
        // (roff_term.c:233-236). Numeric temporary offset is a responsive
        // layout policy, never another field-exit or hard-row decision.
        self.control_line_break();
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
    pub(in crate::mandoc::inline::flow) fn commit_definition_row_origin(
        &mut self,
    ) -> Option<super::super::output::row_origins::OutputNodeEdit> {
        if self.has_formatter_cell()
            && let Some(definition) = &self.execution.definition
            && definition.row.has_pending_origin()
            && definition.field_buffer.resume_offset() < definition.field_buffer.cells().len()
        {
            let (_, edit) = super::super::output::row_origins::set_last_break_origin(
                &mut self.nodes,
                definition.row.indent_columns,
            );
            if let Some(edit) = edit {
                self.remap_output_positions(edit);
            }
            self.definition_state_mut().row.retire_row_origin();
            return edit;
        }
        None
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
        super::super::output::push_row_break(&mut self.nodes, row_indent);
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
            let previous_units = definition.field_offset_units;
            definition.field_offset_units = definition
                .field_offset_units
                .saturating_add_signed(isize::try_from(units).unwrap_or_default());
            if let Some(reading) = &mut definition.column_reading_origin {
                let delta = i128::try_from(definition.field_offset_units)
                    .unwrap_or(i128::MAX)
                    .saturating_sub(i128::try_from(previous_units).unwrap_or(i128::MAX));
                reading.apply(units, delta);
            }
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
            .is_some_and(|definition| definition.hang_row.native_row_occupied())
    }

    pub(in crate::mandoc) fn definition_geometry_checkpoint(
        &self,
        node: &libmandoc_rs::Node,
    ) -> Option<super::DefinitionGeometryCheckpoint> {
        // Roff requests return before the geometry restore. Text restores
        // rmargin only; this ledger records the offset relevant to reading.
        if self.macro_set != libmandoc_rs::MacroSet::Mdoc
            || node.kind == libmandoc_rs::NodeKind::Text
            || node.macro_token.as_ref().is_some_and(|token| match token {
                // mdoc_term.c::print_mdoc_node() returns immediately for
                // node-producing roff tokens, before geometry restoration.
                libmandoc_rs::MacroToken::Roff(request) => request.generates_node(),
                // Preserve the fallback for manually extended owned ASTs.
                libmandoc_rs::MacroToken::Man(libmandoc_rs::ManMacro::In) => true,
                libmandoc_rs::MacroToken::Unknown(name) => {
                    name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                }
                _ => false,
            })
        {
            return None;
        }
        self.definition
            .as_ref()
            .map(|definition| super::DefinitionGeometryCheckpoint {
                indent_columns: definition.row.indent_columns,
                field_offset: definition.hang_row.field_offset,
                field_offset_units: definition.field_offset_units,
                column_reading_origin: definition.column_reading_origin,
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
            definition.column_reading_origin = checkpoint.column_reading_origin;
            definition.margin_override = checkpoint.margin_override;
        }
    }
}

#[cfg(test)]
mod row_origin_tests {
    use super::super::super::NativeWordAnchor;
    use super::super::super::field_buffer::FieldWrite;
    use super::super::state::{DefinitionFieldStyle, NoBreakField};
    use super::*;

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.into(),
        }
    }

    #[test]
    fn inserted_carriers_rebase_every_live_output_address_but_not_native_coordinates() {
        let mut builder = InlineBuilder::new();
        builder.nodes = vec![text("X"), Inline::line_break(), text("  "), text("Z")];
        builder.inherit_author_execution(crate::mandoc::formatter::AuthorFlow::Automatic, false);
        builder
            .execution
            .author_execution
            .as_mut()
            .unwrap()
            .field_output_start = 3;
        builder.execution.flush_unit_output_start = 2;
        builder.execution.formatter_column = FormatterColumn::Advanced;
        let definition = builder.definition_state_mut();
        definition.row.indent_columns = 6;
        definition.row.note_row_origin();
        definition
            .field_buffer
            .apply_writes(&FieldWrite::literal("Z"));
        definition.hang_row.provisional_trailing_break = Some(1);
        definition.field_word_anchors.push(NativeWordAnchor {
            start: 7,
            owner: "native-word".into(),
            content: 9,
            projected_device_padding: 2,
            projected_field_prefix: false,
        });
        definition.no_break = Some(NoBreakField {
            flags: super::super::super::native_field::FieldFlags::column(false),
            output_end_before_separator: 2,
            resumed_output_start: 3,
            resumed_execution_epoch: 0,
            field_width: 0,
            body_width: 12,
            field_capacity_columns: 12,
            trailspace_cells: 1,
            separator_cells: 2,
            style: DefinitionFieldStyle::Tag,
        });
        let mut captured_field = definition.no_break.unwrap();
        let edit = builder.commit_definition_row_origin().unwrap();
        edit.remap(&mut captured_field.output_end_before_separator);
        edit.remap(&mut captured_field.resumed_output_start);
        assert_eq!(
            builder
                .execution
                .author_execution
                .unwrap()
                .field_output_start,
            4
        );
        assert_eq!(builder.execution.flush_unit_output_start, 3);
        let definition = builder.execution.definition.as_ref().unwrap();
        assert_eq!(definition.no_break, Some(captured_field));
        assert_eq!(captured_field.output_end_before_separator, 3);
        assert_eq!(captured_field.resumed_output_start, 4);
        assert_eq!(definition.hang_row.provisional_trailing_break, Some(2));
        assert_eq!(definition.field_word_anchors[0].start, 7);
        assert_eq!(definition.field_word_anchors[0].content, 9);
        assert_eq!(definition.field_word_anchors[0].projected_device_padding, 2);
        assert_eq!(definition.field_buffer.cells().len(), 1);
        let definition = builder.definition_state_mut();
        definition.row.indent_columns = 2;
        definition.row.note_row_origin();
        assert!(builder.commit_definition_row_origin().is_none());
        assert_eq!(builder.nodes.len(), 5);
        assert_eq!(
            builder
                .execution
                .author_execution
                .unwrap()
                .field_output_start,
            4
        );
        assert_eq!(
            super::super::super::output::take_inline_layout(&mut builder.nodes).row_indent(1),
            2
        );
    }
}
