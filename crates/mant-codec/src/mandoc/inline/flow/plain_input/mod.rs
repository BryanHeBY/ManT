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

//! Plain native input retirement without resetting persistent execution state.

use super::{Inline, InlineBuilder};

mod projection;
use projection::{
    completed_empty_plain_passes, literal_tail_row, project_rejected_plain_prefix,
    project_retired_empty_rows, retain_accepted_unit_output, retain_blank_only_unit_output,
    retire_plain_presentation,
};

#[cfg(test)]
mod tests;

mod unit;
pub(super) use unit::PlainInputUnit;

/// The exceptional row fact of this consumed plain flush, never history.
#[derive(Clone, Copy, Default)]
pub(in crate::mandoc::inline::flow) enum PlainFlushOutcome {
    #[default]
    Ordinary,
    UnprintedTail,
    RepresentedTail,
    Rejected,
    RejectedAfterPrintedPass,
}

impl PlainFlushOutcome {
    fn rejected_prefix(
        buffer: &super::field_buffer::FieldBuffer,
        passes: &[super::field_buffer::FillPass],
    ) -> Self {
        if passes
            .last()
            .is_some_and(|pass| buffer.printed_content_end(0, pass.end) > 0)
        {
            Self::RejectedAfterPrintedPass
        } else {
            Self::Rejected
        }
    }

    const fn from_unprinted_tail(unprinted: bool, inherited: bool) -> Self {
        if !unprinted {
            Self::Ordinary
        } else if inherited {
            Self::RepresentedTail
        } else {
            Self::UnprintedTail
        }
    }

    pub(in crate::mandoc::inline::flow) const fn rejected(self) -> bool {
        matches!(self, Self::Rejected | Self::RejectedAfterPrintedPass)
    }

    pub(in crate::mandoc::inline::flow) const fn row_origin(
        self,
        fallback: super::output::CompletedRowOrigin,
    ) -> super::output::CompletedRowOrigin {
        use super::output::CompletedRowOrigin;
        match (self, fallback) {
            (Self::RejectedAfterPrintedPass, CompletedRowOrigin::Layout) => {
                // The same prose unit already printed its accepted prefix.
                // Its rejected tail reaches a separate unconditional
                // endline (term.c:143-146,250-253), before any term_vspace.
                CompletedRowOrigin::HardLine
            }
            _ => fallback,
        }
    }

    pub(in crate::mandoc::inline::flow) const fn closes_unprinted_row(self) -> bool {
        matches!(self, Self::UnprintedTail)
    }

    pub(in crate::mandoc::inline::flow) const fn closes_represented_row(self) -> bool {
        matches!(self, Self::RepresentedTail)
    }
}

fn plain_unit_inherits_printed_row(execution: &super::InlineExecutionState) -> bool {
    execution
        .definition
        .as_ref()
        .is_some_and(|state| state.hang_row.native_row_occupied())
        || execution
            .detached_device_row
            .is_some_and(|row| row.page_origin_printed || row.viscol > 0)
}

impl InlineBuilder {
    /// The plain-flow analogue of `project_definition_field_receipt()`
    /// (`term_flushln` over the shared `tcol->buf`, term.c:233-237 reached
    /// through 143-146): a definitively rejected flush unit dies at its
    /// retirement boundary - the unprinted suffix is trimmed from IR back
    /// to the accepted prefix and the zero-advance register is discarded.
    /// Returns whether the native buffer was definitively rejected.
    pub(in crate::mandoc) fn retire_plain_flush_unit(&mut self) -> bool {
        self.retire_plain_flush_unit_with_tail().rejected()
    }

    pub(in crate::mandoc::inline::flow) fn retire_plain_flush_unit_with_tail(
        &mut self,
    ) -> PlainFlushOutcome {
        let mut projections = Vec::new();
        let ends_row = Self::retire_plain_flush_unit_with_projections(
            &mut self.execution,
            &mut self.nodes,
            super::output::CompletedRowOrigin::Layout,
            Some(&mut projections),
        );
        // Execution indices were mapped at the actual projection, before
        // retirement reset their cursors. Only live builder checkpoints
        // still need that same original-boundary receipt here.
        for edits in projections {
            self.remap_output_projection_checkpoints(&edits);
        }
        ends_row
    }

    /// Execution-state form shared with the no-fill row finisher, which
    /// retires the same native buffer without a live builder.
    pub(in crate::mandoc) fn retire_plain_flush_unit_at(
        execution: &mut super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
        row_origin: super::output::CompletedRowOrigin,
    ) -> bool {
        Self::retire_plain_flush_unit_with_projections(execution, nodes, row_origin, None)
            .rejected()
    }

    fn retire_plain_flush_unit_with_projections(
        execution: &mut super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
        row_origin: super::output::CompletedRowOrigin,
        projections: Option<&mut Vec<super::output::row_origins::OutputNodeEdits>>,
    ) -> PlainFlushOutcome {
        use super::field_buffer::{FillTargets, FlushReceipt};
        // An author-less definition session has no field geometry: its
        // buffer is the same native `tcol->buf` as the plain unit (term.c
        // runs one term_fill() regardless of authorship) and retires with
        // the same receipt. Borrow whichever buffer is live.
        let authorless_definition =
            execution.definition.is_some() && execution.author_execution.is_none();
        let inherited_printed_row = plain_unit_inherits_printed_row(execution);
        if execution.definition.is_some() && !authorless_definition {
            return PlainFlushOutcome::Ordinary;
        }
        let Some(PlainInputUnit {
            mut buffer,
            anchors,
            mut output_start,
        }) = Self::take_plain_flush_unit(execution, nodes, authorless_definition)
        else {
            return PlainFlushOutcome::Ordinary;
        };
        // BRNEVER-shaped (term.c:134,143-144): responsive reflow owns the
        // device width, so a plain pass only ever ends at authored markers.
        let targets = FillTargets {
            first: usize::MAX / 2,
            rest: usize::MAX / 2,
            unbounded: true,
        };
        let receipt = buffer.flush_receipt(targets, false);
        let passes = match &receipt {
            FlushReceipt::Accepted { passes, .. } | FlushReceipt::Rejected { passes, .. } => passes,
        };
        let edits =
            retire_plain_presentation(execution, nodes, &buffer, &anchors, passes, output_start);
        edits.remap(&mut output_start);
        if let Some(projections) = projections {
            projections.push(edits);
        }
        let empty_rows = completed_empty_plain_passes(
            &buffer,
            passes,
            matches!(&receipt, FlushReceipt::Rejected { .. }),
            inherited_printed_row,
            |start, end| buffer.printed_columns(start, end, 0, 0).is_some(),
        );
        let reprojected_literal_rows = matches!(&receipt, FlushReceipt::Rejected { .. })
            && empty_rows > 0
            && row_origin == super::output::CompletedRowOrigin::LiteralText;
        // A deferred scanner can accept several authored-marker passes at
        // the actual term_flushln(). Acceptance still carries their row
        // events (term.c:165-220); retirement cannot silently omit them.
        // The same projector handles a complete unit and an accepted
        // prefix whose following pass is rejected.
        if !reprojected_literal_rows {
            super::output::native_passes::project_accepted_native_passes(
                nodes,
                &mut buffer,
                &anchors,
                passes,
                output_start,
            );
        }
        let FlushReceipt::Rejected {
            passes,
            rejected_from,
            definitive,
            ..
        } = receipt
        else {
            let empty_tail =
                retain_accepted_unit_output(nodes, &buffer, &anchors, passes, output_start);
            project_retired_empty_rows(execution, nodes, empty_rows, row_origin);
            Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
            return PlainFlushOutcome::from_unprinted_tail(
                empty_tail,
                inherited_printed_row && passes.len() == 1,
            );
        };
        if !definitive {
            retain_blank_only_unit_output(
                nodes,
                &buffer,
                &anchors,
                &passes,
                output_start,
                row_origin,
            );
            // The buffer was nonempty, so term_flushln() retires BACKAFTER
            // along with it even though no term_field() ran (235-237).
            execution.zero_advance.discard_at_row_end();
            Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
            // nbr=0 over buffered ordinary blanks still reaches the
            // ordinary term_flushln tail endline. It is not a definitive
            // rejection and must not depend on earlier completed rows.
            return PlainFlushOutcome::from_unprinted_tail(true, inherited_printed_row);
        }

        project_rejected_plain_prefix(
            nodes,
            &buffer,
            &anchors,
            &passes,
            output_start,
            rejected_from,
            reprojected_literal_rows,
        );
        // term.c::term_flushln() clears both backtracking flags; a rejected
        // unit dies whole, including a still-buffered `\z` glyph.
        execution.zero_advance.discard_at_row_end();
        if !reprojected_literal_rows {
            project_retired_empty_rows(execution, nodes, empty_rows, row_origin);
        }
        let outcome = PlainFlushOutcome::rejected_prefix(&buffer, &passes);
        Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
        outcome
    }

    /// Move the selected native buffer and its owner interval as one unit;
    /// an output Vec or helper return cannot retire either independently.
    fn take_plain_flush_unit(
        execution: &mut super::InlineExecutionState,
        nodes: &[Inline],
        authorless_definition: bool,
    ) -> Option<PlainInputUnit> {
        let (buffer, anchors) = if authorless_definition {
            let definition = execution.definition.as_mut().expect("session");
            let buffer = std::mem::take(&mut definition.field_buffer);
            let anchors = std::mem::take(&mut definition.field_word_anchors);
            if buffer.is_empty() {
                Self::clear_authorless_definition_at(execution, nodes);
                return None;
            }
            (buffer, anchors)
        } else {
            if execution.plain_input.buffer.is_empty() {
                return None;
            }
            execution.plain_input.take_input()
        };
        Some(PlainInputUnit {
            buffer,
            anchors,
            output_start: execution.plain_input.output_start.min(nodes.len()),
        })
    }

    /// Output ownership of the current physical row, including a native
    /// empty-row witness. Earlier rows and nonvisible identities do not
    /// represent this row merely because they remain in the same IR Vec.
    pub(in crate::mandoc) fn has_literal_tail_row(nodes: &[Inline]) -> bool {
        literal_tail_row(nodes).unwrap_or(false)
    }

    /// Retire the actual consumed native buffer, not an output fragment.
    pub(in crate::mandoc::inline::flow) fn clear_plain_flush_unit_at(
        execution: &mut super::InlineExecutionState,
        nodes: &[Inline],
    ) {
        execution.detached_device_row = None;
        execution.plain_input.retire_row_at(nodes.len());
    }

    /// Drop a borrowed retirement buffer after its receipt was consumed:
    /// term.c:235-237 clears it either way; the borrowed form must not
    /// re-enter the session.
    fn restore_retired_buffer(
        execution: &mut super::InlineExecutionState,
        nodes: &mut [Inline],
        authorless_definition: bool,
        buffer: super::field_buffer::FieldBuffer,
        anchors: Vec<super::NativeWordAnchor>,
    ) {
        drop(anchors);
        drop(buffer);
        if authorless_definition {
            Self::clear_authorless_definition_at(execution, nodes);
        } else {
            Self::clear_plain_flush_unit_at(execution, nodes);
        }
    }

    fn clear_authorless_definition_at(
        execution: &mut super::InlineExecutionState,
        nodes: &[Inline],
    ) {
        if let Some(definition) = &mut execution.definition {
            definition.field_buffer.clear();
            definition.field_buffer.set_tab_offset(0);
            definition.field_word_anchors.clear();
        }
        execution.plain_input.clear_input_at(nodes.len());
    }
}
