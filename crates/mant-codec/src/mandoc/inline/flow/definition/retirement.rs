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
        Self::retire_plain_flush_unit_at(
            &mut self.execution,
            &mut self.nodes,
            super::super::output::CompletedRowOrigin::Layout,
        )
    }

    /// Execution-state form shared with the no-fill row finisher, which
    /// retires the same native buffer without a live builder.
    pub(in crate::mandoc) fn retire_plain_flush_unit_at(
        execution: &mut super::super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
        row_origin: super::super::output::CompletedRowOrigin,
    ) -> bool {
        use super::super::field_buffer::{FillTargets, FlushReceipt};
        // An author-less definition session has no field geometry: its
        // buffer is the same native `tcol->buf` as the plain unit (term.c
        // runs one term_fill() regardless of authorship) and retires with
        // the same receipt. Borrow whichever buffer is live.
        let authorless_definition =
            execution.definition.is_some() && execution.author_execution.is_none();
        let inherited_printed_row = execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.viscol > 0)
            || execution
                .detached_device_row
                .is_some_and(|row| row.viscol > 0);
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
        let empty_rows = completed_empty_plain_passes(
            &buffer,
            passes,
            matches!(&receipt, FlushReceipt::Rejected { .. }),
            inherited_printed_row,
            |start, end| buffer.printed_columns(start, end, 0, 0).is_some(),
        );
        let reprojected_literal_rows = matches!(&receipt, FlushReceipt::Rejected { .. })
            && empty_rows > 0
            && row_origin == super::super::output::CompletedRowOrigin::LiteralText;
        // A deferred scanner can accept several authored-marker passes at
        // the actual term_flushln(). Acceptance still carries their row
        // events (term.c:165-220); retirement cannot silently omit them.
        // The same projector handles a complete unit and an accepted
        // prefix whose following pass is rejected.
        if !reprojected_literal_rows {
            super::super::output::native_passes::project_accepted_native_passes(
                nodes,
                &buffer,
                &anchors,
                passes,
                0,
                output_start,
            );
        }
        let FlushReceipt::Rejected {
            passes,
            rejected_from,
            definitive,
        } = receipt
        else {
            retain_accepted_unit_output(nodes, &buffer, &anchors, passes, output_start);
            project_retired_empty_rows(execution, nodes, empty_rows, row_origin);
            Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
            return false;
        };
        if !definitive {
            // term_flushln() still reset the buffer (term.c:235-237); a
            // non-definitive stop leaves no unprinted suffix to trim.
            Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
            return false;
        }

        let accepted_owned_prefix =
            !passes.is_empty() && anchors.iter().any(|anchor| anchor.start < rejected_from);
        retain_unit_owner_ranges(nodes, &buffer, &anchors, &passes, output_start);
        if reprojected_literal_rows {
            reproject_plain_literal_loops(nodes, &buffer, &anchors, &passes, output_start);
        } else if accepted_owned_prefix
            && !crate::mandoc::inline::flow::output::ends_with_executed_line_break(nodes)
        {
            // term_flushln() ended the last accepted pass before discovering
            // nbr=0; the retirement boundary must expose that native event.
            nodes.push(Inline::line_break());
        }
        // term.c::term_flushln() clears both backtracking flags; a rejected
        // unit dies whole, including a still-buffered `\z` glyph.
        execution.zero_advance.discard_at_row_end();
        if !reprojected_literal_rows {
            project_retired_empty_rows(execution, nodes, empty_rows, row_origin);
        }
        drop(buffer);
        drop(anchors);
        if authorless_definition {
            Self::clear_authorless_definition_at(execution, nodes);
        } else {
            Self::clear_plain_flush_unit_at(execution, nodes);
        }
        true
    }

    /// Output ownership of the current physical row, including a native
    /// empty-row witness. Earlier rows and nonvisible identities do not
    /// represent this row merely because they remain in the same IR Vec.
    pub(in crate::mandoc) fn has_literal_tail_row(nodes: &[Inline]) -> bool {
        literal_tail_row(nodes).unwrap_or(false)
    }

    /// Retire the actual consumed native buffer, not an output fragment.
    pub(in crate::mandoc::inline::flow) fn clear_plain_flush_unit_at(
        execution: &mut super::super::InlineExecutionState,
        nodes: &[Inline],
    ) {
        execution.detached_device_row = None;
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
        anchors: Vec<super::super::NativeWordAnchor>,
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

/// The output destination chooses the representation of completed rows,
/// never their execution. Literal rows can precede another source word in
/// the same owner; a marker only read at its tail would lose those rows.
fn project_retired_empty_rows(
    execution: &mut super::super::InlineExecutionState,
    nodes: &mut Vec<Inline>,
    rows: u16,
    origin: super::super::output::CompletedRowOrigin,
) {
    match origin {
        super::super::output::CompletedRowOrigin::Layout => {
            InlineBuilder::record_completed_rows_at(execution, nodes, rows, origin);
        }
        super::super::output::CompletedRowOrigin::LiteralText => {
            for _ in 0..rows {
                nodes.push(Inline::Text {
                    value: String::new(),
                });
                nodes.push(Inline::line_break());
            }
        }
    }
}

/// Each real loop endline closes its accepted native interval, including
/// graphful NBRZW intervals with no printed scalar (term.c:340-349,397,217).
/// Only trailing empty rows move to layout; earlier rows retain their ordered
/// inline boundaries before a later accepted printed pass.
fn completed_empty_plain_passes(
    buffer: &super::super::field_buffer::FieldBuffer,
    passes: &[super::super::field_buffer::FillPass],
    rejected: bool,
    mut printed_row: bool,
    mut interval_printed: impl FnMut(usize, usize) -> bool,
) -> u16 {
    // One accepted pass never reaches loop endline (term.c:200-217).
    // Its possibly empty final row remains with retain_accepted_unit_output;
    // there are no completed empty loop rows to classify or print.
    if !rejected && passes.len() <= 1 {
        return 0;
    }
    let mut rows = 0u16;
    let mut start = 0;
    for (index, pass) in passes.iter().enumerate() {
        let printed = interval_printed(start, pass.end);
        printed_row |= printed;
        if printed_row {
            rows = 0;
        } else if index + 1 < passes.len() || rejected {
            rows = rows.saturating_add(1);
        }
        if index + 1 < passes.len() || rejected {
            printed_row = false;
        }
        start = pass.end;
        while matches!(
            buffer.cells().get(start),
            Some(super::super::field_buffer::FieldCell::BreakableBlank)
        ) {
            start += 1;
        }
    }
    rows
}

/// A rejected literal unit's accepted pass loop has one endline per pass
/// (term.c:143-217). Rebuild those events from native owner positions once;
/// word-time hints cannot add a second representation of its empty rows.
/// Real source/request flushes retired earlier units before `output_start`.
fn reproject_plain_literal_loops(
    nodes: &mut Vec<Inline>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    output_start: usize,
) {
    use super::super::field_buffer::FieldCell;
    use std::collections::BTreeMap;
    let mut boundaries = BTreeMap::<String, Vec<usize>>::new();
    let mut accepted_scalars = vec![0; anchors.len()];
    let mut owner = 0;
    let mut start = 0;
    for pass in passes {
        let mut cell = start;
        while cell < pass.end && owner < anchors.len() {
            while anchors
                .get(owner + 1)
                .is_some_and(|next| next.start.min(next.content) <= cell)
            {
                owner += 1;
            }
            let anchor = &anchors[owner];
            let end = anchors
                .get(owner + 1)
                .map_or(pass.end, |next| pass.end.min(next.start.min(next.content)));
            let content = cell.max(anchor.content);
            if content < end {
                accepted_scalars[owner] += buffer.projection_length(content, end);
            }
            cell = end.max(cell + 1);
        }
        start = pass.end;
        while matches!(buffer.cells().get(start), Some(FieldCell::BreakableBlank)) {
            start += 1;
        }
        // term_flushln()205-207 consumes the blank run at this loop
        // boundary. It contributes neither an accepted owner scalar nor
        // an offset into its filtered IR; rejected whitespace must not move
        // the event beyond the retained prefix (term_fill()299-312).
        while anchors
            .get(owner + 1)
            .is_some_and(|next| next.content <= start)
        {
            owner += 1;
        }
        if let Some(anchor) = anchors.get(owner).filter(|anchor| anchor.content <= start) {
            boundaries
                .entry(anchor.owner.clone())
                .or_default()
                .push(accepted_scalars[owner]);
        }
    }
    let mut pending = nodes.split_off(output_start.min(nodes.len()));
    remove_pending_row_hints(&mut pending);
    nodes.extend(
        super::super::output::native_passes::split_retired_native_field_passes(
            &pending,
            &boundaries,
        ),
    );
}

fn remove_pending_row_hints(nodes: &mut Vec<Inline>) {
    nodes.retain_mut(|node| match node {
        Inline::LineBreak { .. } => false,
        Inline::Text { value } | Inline::Code { value } if value.is_empty() => false,
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::PortableDisplay { children, .. }
        | Inline::Link { children, .. } => {
            remove_pending_row_hints(children);
            true
        }
        _ => true,
    });
}

/// Stop at the most recent physical-row witness or delimiter. The live
/// literal owner can contain many earlier rows; visiting that complete
/// history for each invisible source row would make retirement quadratic.
fn literal_tail_row(nodes: &[Inline]) -> Option<bool> {
    find_literal_tail_row(nodes, &mut || {})
}

fn find_literal_tail_row(nodes: &[Inline], visit: &mut impl FnMut()) -> Option<bool> {
    nodes.iter().rev().find_map(|node| match node {
        Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
            visit();
            Some(!value.ends_with('\n'))
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::PortableDisplay { children, .. }
        | Inline::Link { children, .. } => {
            visit();
            find_literal_tail_row(children, visit)
        }
        Inline::LineBreak { .. } => {
            visit();
            Some(false)
        }
        Inline::Anchor { .. } => {
            visit();
            None
        }
    })
}

/// The final accepted pass owns its physical row even if `term_field()`
/// emitted no characters: `ASCII_NBRZW` sets graph in `term_fill()`340-349,
/// but `term_field()` skips it and trailing blanks (389-427). Decide row
/// occupancy after the same receipt has trimmed the unprinted source tail.
fn retain_accepted_unit_output(
    nodes: &mut Vec<Inline>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    output_start: usize,
) {
    // An accepted unit can still leave ordinary trailing blanks outside
    // its final nbr. Use exactly the same native ownership intervals as
    // rejection; provisional IR spaces do not prove printed native content.
    if passes
        .last()
        .is_some_and(|pass| buffer.projection_length(pass.end, buffer.cells().len()) > 0)
    {
        retain_unit_owner_ranges(nodes, buffer, anchors, passes, output_start);
    }
    let pending_start = buffer.resume_offset();
    let accepted_empty_tail = passes.last().is_some_and(|pass| {
        let start = passes
            .len()
            .checked_sub(2)
            .map_or(pending_start, |previous| {
                passes[previous].end.max(pending_start)
            });
        pass.end > start && buffer.printed_columns(start, pass.end, 0, 0).is_none()
    });
    if accepted_empty_tail
        && !InlineBuilder::has_literal_tail_row(&nodes[output_start.min(nodes.len())..])
    {
        nodes.push(Inline::Text {
            value: String::new(),
        });
    }
}

/// Apply one native receipt to its active output interval. Acceptance and
/// rejection share this ownership operation; earlier committed output is
/// outside the supplied interval and cannot be revoked by a later unit.
fn retain_unit_owner_ranges(
    nodes: &mut Vec<Inline>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    output_start: usize,
) {
    let accepted_owners =
        super::super::output::native_passes::accepted_owner_lengths(buffer, anchors, passes);
    let mut pending_output = nodes.split_off(output_start);
    let owned = crate::mandoc::inline::flow::output::split::retain_native_field_owners(
        &mut pending_output,
        &accepted_owners,
    );
    if !owned {
        retain_unprinted_field_targets(&mut pending_output);
    }
    nodes.extend(pending_output);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_loop_classification_scans_each_accepted_cell_once() {
        use super::super::super::field_buffer::{
            FieldBuffer, FieldCell, FieldWrite, FillTargets, FlushReceipt,
        };

        // All complete NBRZW/p/refused-tail sources ran the five pristine
        // profiles before this counter. term_field() visits accepted native
        // intervals forward once (term.c:389-444), even when its zero-width
        // cells do not print and a previously printed row remains occupied.
        for count in [1, 2, 64, 1024, 2048, 4096, 8192] {
            for (buffered_prefix, inherited_row) in [(false, false), (true, false), (false, true)] {
                let mut buffer = FieldBuffer::default();
                let mut writes = if buffered_prefix {
                    FieldWrite::literal("BEFORE")
                } else {
                    Vec::new()
                };
                for _ in 0..count {
                    writes.extend([
                        FieldWrite::Cell(FieldCell::ZeroWidthGraph),
                        FieldWrite::Cell(FieldCell::BreakMarker),
                        FieldWrite::UnprojectedBlank,
                    ]);
                }
                writes.push(FieldWrite::Cell(FieldCell::BreakMarker));
                writes.push(FieldWrite::UnprojectedBlank);
                writes.extend(FieldWrite::literal("REJECTED"));
                buffer.apply_writes(&writes);
                let receipt = buffer.flush_receipt(
                    FillTargets {
                        first: usize::MAX / 2,
                        rest: usize::MAX / 2,
                        unbounded: true,
                    },
                    false,
                );
                let FlushReceipt::Rejected { passes, .. } = receipt else {
                    panic!("a graphless leading marker rejects the suffix");
                };
                assert_eq!(passes.len(), count);
                let mut visits = 0;
                let rows = completed_empty_plain_passes(
                    &buffer,
                    &passes,
                    true,
                    inherited_row,
                    |start, end| {
                        visits += end.saturating_sub(start);
                        buffer.printed_columns(start, end, 0, 0).is_some()
                    },
                );
                assert_eq!(
                    usize::from(rows),
                    count - usize::from(buffered_prefix || inherited_row)
                );
                assert_eq!(visits, count * 2 + usize::from(buffered_prefix) * 6);
                assert!(
                    visits <= buffer.cells().len(),
                    "{count} passes revisited {visits} cells"
                );
            }
        }
    }

    #[test]
    fn accepted_single_pass_never_scans_for_completed_loop_rows() {
        use super::super::super::field_buffer::{
            FieldBuffer, FieldCell, FieldWrite, FillTargets, FlushReceipt,
        };

        // Exact ACCEPTED and NBRZW-only words ran pristine first. A lone
        // accepted pass exits before endline (term.c:200-217); its empty
        // final row remains handled by retain_accepted_unit_output().
        for writes in [
            FieldWrite::literal("ACCEPTED"),
            vec![FieldWrite::Cell(FieldCell::ZeroWidthGraph)],
        ] {
            let mut buffer = FieldBuffer::default();
            buffer.apply_writes(&writes);
            let FlushReceipt::Accepted { passes } = buffer.flush_receipt(
                FillTargets {
                    first: usize::MAX / 2,
                    rest: usize::MAX / 2,
                    unbounded: true,
                },
                false,
            ) else {
                panic!("one graphful pass is accepted");
            };
            assert_eq!(passes.len(), 1);
            assert_eq!(
                completed_empty_plain_passes(&buffer, &passes, false, false, |_, _| {
                    panic!("there is no loop row to scan")
                }),
                0
            );
        }
    }

    #[test]
    fn literal_tail_queries_stop_before_completed_row_history() {
        for count in [64, 1024, 4096] {
            let mut nodes = Vec::new();
            for _ in 0..count {
                nodes.push(Inline::Text {
                    value: String::new(),
                });
                nodes.push(Inline::line_break());
            }
            let mut visits = 0;
            assert_eq!(
                find_literal_tail_row(&nodes, &mut || visits += 1),
                Some(false)
            );
            assert_eq!(visits, 1);
            nodes.push(Inline::Strong {
                children: vec![Inline::Text {
                    value: String::new(),
                }],
            });
            nodes.push(Inline::anchor("identity"));
            visits = 0;
            assert_eq!(
                find_literal_tail_row(&nodes, &mut || visits += 1),
                Some(true)
            );
            assert_eq!(visits, 3);
        }
    }
}
