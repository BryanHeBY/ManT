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

//! Plain flush-unit retirement without resetting persistent execution state.

use super::super::{Inline, InlineBuilder};
use super::flush::retain_unprinted_field_targets;

impl InlineBuilder {
    /// The plain-flow analogue of `project_definition_field_receipt()`
    /// (`term_flushln` over the shared `tcol->buf`, term.c:233-237 reached
    /// through 143-146): a definitively rejected flush unit dies at its
    /// retirement boundary - the unprinted suffix is trimmed from IR back
    /// to the accepted prefix and the zero-advance register is discarded.
    /// Returns whether the retirement ended a native row.
    pub(in crate::mandoc) fn retire_plain_flush_unit(&mut self) -> bool {
        Self::retire_plain_flush_unit_at(&mut self.execution, &mut self.nodes)
    }

    /// Execution-state form shared with the no-fill row finisher, which
    /// retires the same native buffer without a live builder.
    pub(in crate::mandoc) fn retire_plain_flush_unit_at(
        execution: &mut super::super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
    ) -> bool {
        use super::super::field_buffer::{FillTargets, FlushReceipt};
        // An author-less definition session has no field geometry: its
        // buffer is the same native `tcol->buf` as the plain unit (term.c
        // runs one term_fill() regardless of authorship) and retires with
        // the same receipt. Borrow whichever buffer is live.
        let authorless_definition =
            execution.definition.is_some() && execution.author_execution.is_none();
        if execution.definition.is_some() && !authorless_definition {
            return false;
        }
        let buffer;
        let anchors;
        let output_start;
        if authorless_definition {
            let definition = execution.definition.as_mut().expect("session");
            buffer = std::mem::take(&mut definition.field_buffer);
            anchors = std::mem::take(&mut definition.field_word_anchors);
            output_start = execution.flush_unit_output_start.min(nodes.len());
            if buffer.is_empty() {
                Self::clear_authorless_definition_at(execution, nodes);
                return false;
            }
        } else {
            if execution.flush_unit.is_empty() {
                return false;
            }
            buffer = std::mem::take(&mut execution.flush_unit);
            anchors = std::mem::take(&mut execution.flush_unit_anchors);
            output_start = execution.flush_unit_output_start.min(nodes.len());
        }
        // BRNEVER-shaped (term.c:134,143-144): responsive reflow owns the
        // device width, so a plain pass only ever ends at authored markers.
        let targets = FillTargets {
            first: usize::MAX / 2,
            rest: usize::MAX / 2,
            unbounded: true,
        };
        let receipt = buffer.flush_receipt(targets, false);
        let passes = match &receipt {
            FlushReceipt::Accepted { passes } | FlushReceipt::Rejected { passes, .. } => passes,
        };
        // A deferred scanner can accept several authored-marker passes at
        // the actual term_flushln(). Acceptance still carries their row
        // events (term.c:165-220); retirement cannot silently omit them.
        // The same projector handles a complete unit and an accepted
        // prefix whose following pass is rejected.
        super::super::output::native_passes::project_accepted_native_passes(
            nodes,
            &buffer,
            &anchors,
            passes,
            0,
            output_start,
        );
        let FlushReceipt::Rejected {
            passes,
            rejected_from,
            definitive,
        } = receipt
        else {
            // The flushed row prints the whole unit; nothing is unprinted.
            Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
            return false;
        };
        if !definitive {
            // term_flushln() still reset the buffer (term.c:235-237); a
            // non-definitive stop leaves no unprinted suffix to trim.
            Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
            return false;
        }

        let anchor = anchors
            .iter()
            .rev()
            .find(|(cell, _, _)| *cell <= rejected_from)
            .cloned();
        let (marker, prefix_cells) = anchor.map_or((None, 0), |(_, marker, content)| {
            let length = super::super::output::native_passes::accepted_owner_prefix_length(
                &buffer, &passes, content,
            );
            (Some(marker), length)
        });
        let accepted_owned_prefix =
            !passes.is_empty() && anchors.iter().any(|(cell, _, _)| *cell < rejected_from);
        let mut pending_output = nodes.split_off(output_start);
        let owned = marker.as_deref().is_some_and(|marker| {
            crate::mandoc::inline::flow::output::split::retain_native_field_prefix(
                &mut pending_output,
                marker,
                prefix_cells,
            )
        });
        if !owned {
            retain_unprinted_field_targets(&mut pending_output);
        }
        nodes.extend(pending_output);
        if accepted_owned_prefix
            && !crate::mandoc::inline::flow::output::ends_with_executed_line_break(nodes)
        {
            // term_flushln() ended the last accepted pass before discovering
            // nbr=0; the retirement boundary must expose that native event.
            nodes.push(Inline::line_break());
        }
        // term.c::term_flushln() clears both backtracking flags; a rejected
        // unit dies whole, including a still-buffered `\z` glyph.
        execution.zero_advance.discard_at_row_end();
        drop(buffer);
        drop(anchors);
        if authorless_definition {
            Self::clear_authorless_definition_at(execution, nodes);
        } else {
            Self::clear_plain_flush_unit_at(execution, nodes);
        }
        true
    }

    /// `term_flushln()` clears the consumed buffer at every retirement
    /// (`term.c`:235-237); the next word starts a fresh flush unit whose
    /// output interval begins at the current IR end.
    /// Row-boundary reset for flows whose marker semantics live in the text
    /// executor: only the native buffer dies with the row (term.c:235-237).
    pub(in crate::mandoc) fn clear_plain_flush_unit_for_row(
        execution: &mut super::super::InlineExecutionState,
    ) {
        execution.flush_unit.clear();
        execution.flush_unit.set_tab_offset(0);
        execution.flush_unit_anchors.clear();
        execution.flush_unit_output_start = 0;
    }

    pub(in crate::mandoc::inline::flow) fn clear_plain_flush_unit_at(
        execution: &mut super::super::InlineExecutionState,
        nodes: &[Inline],
    ) {
        execution.flush_unit.clear();
        execution.flush_unit.set_tab_offset(0);
        execution.flush_unit_anchors.clear();
        execution.flush_unit_output_start = nodes.len();
    }

    /// Drop a borrowed retirement buffer after its receipt was consumed:
    /// term.c:235-237 clears it either way; the borrowed form must not
    /// re-enter the session.
    fn restore_retired_buffer(
        execution: &mut super::super::InlineExecutionState,
        nodes: &mut [Inline],
        authorless_definition: bool,
        buffer: super::super::field_buffer::FieldBuffer,
        anchors: Vec<(usize, String, usize)>,
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
        execution: &mut super::super::InlineExecutionState,
        nodes: &[Inline],
    ) {
        if let Some(definition) = &mut execution.definition {
            definition.field_buffer.clear();
            definition.field_buffer.set_tab_offset(0);
            definition.field_word_anchors.clear();
        }
        execution.flush_unit.clear();
        execution.flush_unit_anchors.clear();
        execution.flush_unit_output_start = nodes.len();
    }
}
