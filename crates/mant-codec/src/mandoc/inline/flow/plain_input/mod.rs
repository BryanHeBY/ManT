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

use super::output::{
    CompletedRowOrigin, SectionHandoff,
    native_passes::{NativePassHandoff, project_accepted_native_passes},
    row_origins::OutputNodeEdits,
};
use super::{Inline, InlineBuilder};

mod projection;
use projection::{
    completed_empty_plain_passes, literal_tail_row, project_completed_empty_passes,
    project_rejected_plain_prefix, retain_accepted_unit_output, retain_blank_only_unit_output,
    retire_plain_presentation,
};

#[cfg(test)]
mod tests;

mod unit;
pub(super) use unit::PlainInputUnit;

/// Where this native flush's completed rows belong. Source literal rows
/// publish their own empty TEXT and delimiter. A projected literal close
/// instead keeps an existing reading row and publishes only uncovered rows.
#[derive(Clone, Copy)]
pub(super) enum PlainFlushBoundary {
    Layout,
    ManSectionHeading,
    LiteralLineEnd,
    LiteralSourceRow,
}

impl PlainFlushBoundary {
    const fn row_origin(self) -> CompletedRowOrigin {
        match self {
            Self::Layout | Self::ManSectionHeading | Self::LiteralLineEnd => {
                CompletedRowOrigin::Layout
            }
            Self::LiteralSourceRow => CompletedRowOrigin::LiteralText,
        }
    }

    fn fits_without_ending_heading_row(
        self,
        accepted: super::field_buffer::AcceptedPasses<'_>,
        inherited_printed_row: bool,
        inherited_minbl: usize,
    ) -> bool {
        // pre_SH uses zero offset and trailspace equal to the first
        // field's width. Nonzero minbl consumes at least one cell of
        // its capacity. Without it, a glyphless final term_fill fits
        // within the half-cell tolerance (term.c:113–135,239–253).
        // A rejected final pass returns zero vbr even after earlier ink.
        matches!(self, Self::ManSectionHeading)
            && !inherited_printed_row
            && inherited_minbl == 0
            && (accepted.rejected() || accepted.all().last().is_none_or(|pass| pass.width == 0))
    }
}

struct PlainRetirement {
    outcome: PlainFlushOutcome,
    projection: Option<OutputNodeEdits>,
    fitting_heading_tail: bool,
}

impl PlainRetirement {
    const UNMODELED: Self = Self {
        outcome: PlainFlushOutcome::Ordinary,
        projection: None,
        fitting_heading_tail: false,
    };
}

/// The exceptional row fact of this consumed plain flush, never history.
#[derive(Clone, Copy, Default)]
pub(in crate::mandoc::inline::flow) enum PlainFlushOutcome {
    #[default]
    Ordinary,
    UnprintedTail,
    RepresentedTail,
    Rejected {
        from: Option<usize>,
        after_printed_pass: bool,
        closes_represented_row: bool,
    },
}

impl PlainFlushOutcome {
    fn rejected_prefix(
        buffer: &super::field_buffer::FieldBuffer,
        passes: &[super::field_buffer::FillPass],
        from: usize,
    ) -> Self {
        Self::Rejected {
            from: Some(from),
            after_printed_pass: passes
                .last()
                .is_some_and(|pass| buffer.printed_content_end(0, pass.end) > 0),
            closes_represented_row: false,
        }
    }

    fn with_printed_predecessor(mut self, preceding_row: bool) -> Self {
        // Without an accepted pass, the final term_flushln() endline
        // can still close the preceding field's printed device row.
        if let Self::Rejected {
            closes_represented_row,
            ..
        } = &mut self
        {
            *closes_represented_row = preceding_row;
        }
        self
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
        matches!(self, Self::Rejected { .. })
    }

    pub(in crate::mandoc::inline::flow) const fn rejection_start(self) -> Option<usize> {
        match self {
            Self::Rejected { from, .. } => from,
            _ => None,
        }
    }

    pub(in crate::mandoc::inline::flow) const fn row_origin(
        self,
        fallback: super::output::CompletedRowOrigin,
    ) -> super::output::CompletedRowOrigin {
        use super::output::CompletedRowOrigin;
        match (self, fallback) {
            (
                Self::Rejected {
                    after_printed_pass: true,
                    ..
                },
                CompletedRowOrigin::Layout,
            ) => {
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
        matches!(
            self,
            Self::RepresentedTail
                | Self::Rejected {
                    closes_represented_row: true,
                    ..
                }
        )
    }
}

fn inherited_plain_field(
    execution: &super::InlineExecutionState,
    handoff: Option<&SectionHandoff>,
) -> (bool, usize) {
    if let Some(state) = &execution.definition {
        (state.hang_row.native_row_occupied(), state.hang_row.minbl)
    } else {
        let (printed, minimum) = execution.detached_device_row.map_or((false, 0), |row| {
            (row.page_origin_printed || row.viscol > 0, row.minbl)
        });
        (
            printed || handoff.is_some_and(SectionHandoff::continues_printed_row),
            minimum,
        )
    }
}

fn record_fresh_plain_paint_origins(
    execution: &super::InlineExecutionState,
    buffer: &mut super::field_buffer::FieldBuffer,
    passes: &[super::field_buffer::FillPass],
    (inherited_printed_row, inherited_minbl): (bool, usize),
    authorless_definition: bool,
) {
    if inherited_printed_row
        || authorless_definition
        || execution.table_cell.is_some()
        || execution.macro_set != libmandoc_rs::MacroSet::Mdoc
        || execution.text_alignment != mant_ir::TextAlignment::Left
    {
        return;
    }
    // A fresh row has no predecessor ink to overwrite. On it,
    // term_flushln() releases its actual offset (or minbl) as
    // spaces before term_field() emits the first graph.
    let device_origin = |presentation: super::presentation::RowPresentation, minbl: usize| {
        let units = (i64::from(presentation.parent) + presentation.source_offset_units(0))
            .max(i64::try_from(minbl.saturating_mul(24)).unwrap_or(i64::MAX));
        usize::try_from(units.saturating_add(11).div_euclid(24)).unwrap_or_default()
    };
    let mut presentation = execution.row_presentation;
    let first = device_origin(presentation, inherited_minbl);
    presentation.endline();
    buffer.record_plain_paint_origins(passes, first, device_origin(presentation, 0));
}

fn retire_plain_paint(
    execution: &mut super::InlineExecutionState,
    nodes: &mut Vec<Inline>,
    unit: &mut PlainInputUnit,
    passes: &[super::field_buffer::FillPass],
    inherited: (bool, usize),
    authorless_definition: bool,
    operands: Option<&std::cell::RefCell<super::HeadOperandCapture>>,
) -> OutputNodeEdits {
    let PlainInputUnit {
        buffer,
        anchors,
        output_start,
        ..
    } = unit;
    record_fresh_plain_paint_origins(execution, buffer, passes, inherited, authorless_definition);
    if let Some(operands) = operands {
        operands
            .borrow_mut()
            .record_accepted_covers(buffer, anchors, passes);
    }
    let edits = retire_plain_presentation(execution, nodes, buffer, anchors, passes, *output_start);
    edits.remap(output_start);
    edits
}

// BRNEVER-shaped (term.c:134,143-144). Responsive reflow owns page width.
const PLAIN_FIELD_TARGETS: super::field_buffer::FillTargets = super::field_buffer::FillTargets {
    first: usize::MAX / 2,
    rest: usize::MAX / 2,
    unbounded: true,
};

impl InlineBuilder {
    /// The plain-flow analogue of `project_definition_field_receipt()`
    /// (`term_flushln` over the shared `tcol->buf`, term.c:233-237 reached
    /// through 143-146): a definitively rejected flush unit dies at its
    /// retirement boundary - the unprinted suffix is trimmed from IR back
    /// to the accepted prefix and the zero-advance register is discarded.
    pub(super) fn retire_plain_flush_unit(
        &mut self,
        boundary: PlainFlushBoundary,
    ) -> PlainFlushOutcome {
        self.retire_plain_row(boundary).0
    }

    pub(super) fn retire_plain_row(
        &mut self,
        boundary: PlainFlushBoundary,
    ) -> (PlainFlushOutcome, bool) {
        let Some(retired) = Self::consume_plain_flush_unit(
            &mut self.execution,
            &mut self.nodes,
            boundary,
            self.section_handoff.as_mut(),
            self.head_operand_capture.as_deref(),
        ) else {
            return (PlainFlushOutcome::Ordinary, false);
        };
        // Execution indices were mapped at the actual projection, before
        // retirement reset their cursors. Only live builder checkpoints
        // still need that same original-boundary receipt here.
        if let Some(edits) = retired.projection {
            self.remap_output_projection_checkpoints(&edits);
        }
        self.note_native_input_retired();
        (retired.outcome, retired.fitting_heading_tail)
    }

    /// Execution-state form shared with the no-fill row finisher, which
    /// retires the same native buffer without a live builder.
    pub(super) fn retire_plain_literal_source_row_at(
        execution: &mut super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
    ) -> bool {
        Self::consume_plain_flush_unit(
            execution,
            nodes,
            PlainFlushBoundary::LiteralSourceRow,
            None,
            None,
        )
        .is_some_and(|retired| retired.outcome.rejected())
    }

    fn consume_plain_flush_unit(
        execution: &mut super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
        boundary: PlainFlushBoundary,
        mut handoff: Option<&mut SectionHandoff>,
        operands: Option<&std::cell::RefCell<super::HeadOperandCapture>>,
    ) -> Option<PlainRetirement> {
        use super::field_buffer::FlushReceipt;
        let inherited = inherited_plain_field(execution, handoff.as_deref());
        let (inherited_printed_row, inherited_minbl) = inherited;
        let (authorless_definition, mut unit) = Self::take_plain_flush_unit(execution, nodes)?;
        let receipt = unit.buffer.flush_receipt(PLAIN_FIELD_TARGETS, false);
        let Some(accepted) = receipt.accepted_passes() else {
            Self::retire_unmodeled_plain_flush(execution, nodes, authorless_definition, unit);
            return Some(PlainRetirement::UNMODELED);
        };
        let passes = accepted.all();
        let edits = retire_plain_paint(
            execution,
            nodes,
            &mut unit,
            passes,
            inherited,
            authorless_definition,
            operands,
        );
        let (mut buffer, anchors, output_start) = unit.into_parts();
        if let Some(handoff) = &mut handoff {
            handoff.note_projection_start(output_start);
        }
        let mut empty_rows = completed_empty_plain_passes(
            &buffer,
            accepted,
            inherited_printed_row,
            |start, end| buffer.printed_columns(start, end, 0, 0).is_some(),
            handoff.as_deref_mut(),
        );
        let reprojected_literal_rows = accepted.rejected()
            && empty_rows > 0
            && matches!(boundary, PlainFlushBoundary::LiteralSourceRow);
        // A deferred scanner can accept several authored-marker passes at
        // the actual term_flushln(). Acceptance still carries their row
        // events (term.c:165-220); retirement cannot silently omit them.
        // The same projector handles a complete unit and an accepted
        // prefix whose following pass is rejected.
        if !reprojected_literal_rows {
            project_accepted_native_passes(
                nodes,
                &mut buffer,
                &anchors,
                passes,
                output_start,
                handoff.as_deref().map(NativePassHandoff::Section),
            );
        }
        let outcome = match &receipt {
            FlushReceipt::Accepted { .. } => PlainFlushOutcome::from_unprinted_tail(
                retain_accepted_unit_output(nodes, &buffer, &anchors, passes, output_start),
                inherited_printed_row && passes.len() == 1,
            ),
            FlushReceipt::Rejected {
                definitive: true,
                rejected_from,
                ..
            } => {
                project_rejected_plain_prefix(
                    nodes,
                    &buffer,
                    &anchors,
                    passes,
                    output_start,
                    *rejected_from,
                    reprojected_literal_rows,
                );
                PlainFlushOutcome::rejected_prefix(&buffer, passes, *rejected_from)
            }
            FlushReceipt::Rejected {
                definitive: false, ..
            } => {
                retain_blank_only_unit_output(
                    nodes,
                    &buffer,
                    &anchors,
                    passes,
                    output_start,
                    boundary.row_origin(),
                );
                // nbr=0 over buffered ordinary blanks reaches the same
                // final endline, but does not reject an authored suffix.
                empty_rows = 0;
                PlainFlushOutcome::from_unprinted_tail(true, inherited_printed_row)
            }
            FlushReceipt::UnmodeledRewind => unreachable!("modeled plain pass receipt"),
        }
        .with_printed_predecessor(inherited_printed_row && passes.is_empty());
        if accepted.rejected() {
            // term_flushln() clears both backtracking flags even when
            // term_field() printed nothing (term.c:233–237).
            execution.zero_advance.discard_at_row_end();
        }
        project_completed_empty_passes(execution, nodes, output_start, empty_rows, boundary);
        super::output::deferred_blanks::project(nodes, &buffer, passes, output_start);
        Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
        Some(PlainRetirement {
            outcome,
            projection: Some(edits),
            fitting_heading_tail: boundary.fits_without_ending_heading_row(
                accepted,
                inherited_printed_row,
                inherited_minbl,
            ),
        })
    }

    fn retire_unmodeled_plain_flush(
        execution: &mut super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
        authorless_definition: bool,
        unit: PlainInputUnit,
    ) {
        super::output::deferred_blanks::project_unmodeled(nodes, &unit.buffer, unit.output_start);
        Self::restore_retired_buffer(
            execution,
            nodes,
            authorless_definition,
            unit.buffer,
            unit.anchors,
        );
    }

    /// Move the selected native buffer and its owner interval as one unit;
    /// an output Vec or helper return cannot retire either independently.
    fn take_plain_flush_unit(
        execution: &mut super::InlineExecutionState,
        nodes: &[Inline],
    ) -> Option<(bool, PlainInputUnit)> {
        // An author-less definition has no field geometry and can retire
        // with the same native pass receipt as the plain input buffer.
        let authorless_definition =
            execution.definition.is_some() && execution.author_execution.is_none();
        if execution.definition.is_some() && !authorless_definition {
            return None;
        }
        let (buffer, anchors) = if authorless_definition {
            let definition = execution.definition.as_mut().expect("session");
            let buffer = std::mem::take(&mut definition.field_buffer);
            let anchors = std::mem::take(&mut definition.field_word_anchors);
            definition.owner_replay_dirty_from = None;
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
        Some((
            authorless_definition,
            PlainInputUnit {
                buffer,
                anchors,
                output_start: execution.plain_input.output_start.min(nodes.len()),
                owner_replay_dirty_from: None,
            },
        ))
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
            definition.clear_word_anchors();
        }
        execution.plain_input.clear_input_at(nodes.len());
    }
}
