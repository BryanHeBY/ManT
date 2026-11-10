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

//! Project accepted plain native input into its current output owner.

use super::super::output::retain_inline_identities as retain_unprinted_field_targets;
use super::super::{Inline, InlineBuilder};

/// Bind resolved layout to accepted source cells before output edits retire
/// their stable owner coordinates. An IR drain is not this native flush.
pub(super) fn retire_plain_presentation(
    execution: &mut super::super::InlineExecutionState,
    nodes: &mut Vec<Inline>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    output_start: usize,
) -> super::super::output::row_origins::OutputNodeEdits {
    let offset = execution
        .definition
        .as_ref()
        .map_or(0, |state| state.field_offset_units);
    let mut presentation = execution.row_presentation;
    let mut start = 0;
    let mut row_presentations = Vec::new();
    for pass in passes {
        if let Some(receipt) = super::super::presentation::pass_receipt(
            buffer,
            anchors,
            start,
            pass.end,
            presentation.hint_at(execution.text_alignment, offset),
        ) {
            row_presentations.push(receipt);
        }
        start = pass.end;
        presentation.endline_at(offset);
    }
    // Even nbr=0 reaches the ordinary tail endline over this nonempty
    // plain buffer (term.c:143-146,250-253). The absent-buffer early return
    // in retirement never reaches this helper or consumes a pending ti.
    presentation.endline_at(offset);
    execution.row_presentation = presentation;
    let edits = super::super::output::row_origins::project_row_presentations(
        nodes,
        &row_presentations,
        output_start,
    );
    execution.remap_output_projection(&edits);
    edits
}

pub(super) fn project_rejected_plain_prefix(
    nodes: &mut Vec<Inline>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    output_start: usize,
    rejected_from: usize,
    literal_rows: bool,
) {
    let accepted_owned_prefix =
        !passes.is_empty() && anchors.iter().any(|anchor| anchor.start < rejected_from);
    retain_unit_owner_ranges(nodes, buffer, anchors, passes, output_start, false);
    if literal_rows {
        reproject_plain_literal_loops(nodes, buffer, anchors, passes, output_start);
    } else if accepted_owned_prefix && accepted_prefix_endline(&nodes[output_start..]) != Some(true)
    {
        // term_flushln() ended the last accepted pass before discovering
        // nbr=0; expose that native event if it has no delimiter yet.
        nodes.push(Inline::line_break());
    }
}

fn accepted_prefix_endline(nodes: &[Inline]) -> Option<bool> {
    // An accepted invisible cell may keep an empty projection after an
    // already emitted boundary. The receipt counts additional empty loops
    // separately in project_completed_empty_passes().
    nodes.iter().rev().find_map(|node| match node {
        Inline::Anchor { .. } => None,
        Inline::Text { value } | Inline::Code { value } if value.is_empty() => None,
        Inline::LineBreak { .. } => Some(true),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => accepted_prefix_endline(children),
        _ => Some(false),
    })
}

/// A blank-only buffer reaches nbr=0 without a rejecting marker
/// (`term_fill()`, term.c:289-312,360-365). Its ordinary blanks were never
/// printed. Filter its precise owner interval while retaining the one
/// occupied-but-empty literal row independently of provisional blank text.
pub(super) fn retain_blank_only_unit_output(
    nodes: &mut Vec<Inline>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    output_start: usize,
    row_origin: super::super::output::CompletedRowOrigin,
) {
    retain_unit_owner_ranges(nodes, buffer, anchors, passes, output_start, false);
    if row_origin == super::super::output::CompletedRowOrigin::LiteralText
        && !InlineBuilder::has_literal_tail_row(&nodes[output_start.min(nodes.len())..])
    {
        nodes.push(Inline::Text {
            value: String::new(),
        });
    }
}

/// An accepted `term_flushln()` loop endline (term.c:205-220) closes a
/// hard row even if `term_field()` printed no glyph. Keep its delimiter
/// when a later word joins this owner and its receipt for a terminal drain.
pub(super) fn project_completed_empty_passes(
    execution: &mut super::super::InlineExecutionState,
    nodes: &mut Vec<Inline>,
    output_start: usize,
    rows: u16,
    origin: super::super::output::CompletedRowOrigin,
) {
    use super::super::output::CompletedRowOrigin;
    if rows == 0 {
        return;
    }
    // Reconcile only the surviving projection of this accepted suffix.
    // Acknowledged delimiters can be removed by later owner acceptance.
    let mut delimiters = 0;
    let visible = count_projected_tail_delimiters(&nodes[output_start..], &mut delimiters);
    if visible && delimiters == 0 {
        nodes.push(Inline::line_break());
    }
    let projected_empty = delimiters.saturating_sub(usize::from(visible));
    let missing = usize::from(rows).saturating_sub(projected_empty);
    if origin == CompletedRowOrigin::LiteralText {
        for _ in 0..missing {
            nodes.push(Inline::Text {
                value: String::new(),
            });
            nodes.push(Inline::line_break());
        }
        return;
    }
    nodes.extend(std::iter::repeat_n(Inline::line_break(), missing));
    InlineBuilder::record_completed_rows_at(execution, nodes, rows, CompletedRowOrigin::HardLine);
}

fn count_projected_tail_delimiters(nodes: &[Inline], delimiters: &mut usize) -> bool {
    nodes.iter().rev().any(|node| match node {
        Inline::LineBreak { .. } => {
            *delimiters += 1;
            false
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => count_projected_tail_delimiters(children, delimiters),
        _ => super::super::output::has_rendered_formatter_glyph(std::slice::from_ref(node)),
    })
}

/// Each real loop endline closes its accepted native interval, including
/// graphful NBRZW intervals with no printed scalar (term.c:340-349,397,217).
/// Only trailing empty rows are transferred at drain; earlier rows retain
/// their ordered inline boundaries before a later accepted printed pass.
pub(super) fn completed_empty_plain_passes(
    buffer: &super::super::field_buffer::FieldBuffer,
    passes: &[super::super::field_buffer::FillPass],
    rejected: bool,
    mut printed_row: bool,
    mut interval_printed: impl FnMut(usize, usize) -> bool,
    mut handoff: Option<&mut super::super::output::SectionHandoff>,
) -> u16 {
    // One accepted pass never reaches loop endline (term.c:200-217).
    // Its possibly empty final row remains with retain_accepted_unit_output;
    // there are no completed empty loop rows to classify or print.
    if !rejected && passes.len() <= 1 {
        return 0;
    }
    let covered = if handoff.is_some() {
        buffer.covered_zero_width_retreats(passes)
    } else {
        Vec::new()
    };
    let mut covered = covered.as_slice();
    let mut rows = 0u16;
    let mut start = 0;
    for (index, pass) in passes.iter().enumerate() {
        let printed = interval_printed(start, pass.end);
        printed_row |= printed;
        let ends_line = index + 1 < passes.len() || rejected;
        let closes_previous = handoff.as_deref_mut().is_some_and(|handoff| {
            handoff.observe_accepted_pass(
                buffer,
                start..pass.end,
                ends_line,
                printed_row,
                &mut covered,
            )
        });
        if !closes_previous && printed_row {
            rows = 0;
        } else if !closes_previous && ends_line {
            rows = rows.saturating_add(1);
        }
        if ends_line {
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
pub(super) fn literal_tail_row(nodes: &[Inline]) -> Option<bool> {
    find_literal_tail_row(nodes, &mut || {})
}

pub(super) fn find_literal_tail_row(nodes: &[Inline], visit: &mut impl FnMut()) -> Option<bool> {
    nodes.iter().rev().find_map(|node| match node {
        Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
            visit();
            Some(!value.ends_with('\n'))
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
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
pub(super) fn retain_accepted_unit_output(
    nodes: &mut Vec<Inline>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    output_start: usize,
) -> bool {
    // An accepted unit can still leave ordinary trailing blanks outside
    // its final nbr. Use exactly the same native ownership intervals as
    // rejection; provisional IR spaces do not prove printed native content.
    let mut pass_start = buffer.resume_offset();
    let unprinted_padding = passes.iter().any(|pass| {
        let unprinted = buffer.has_unprinted_padding(pass_start, pass.end);
        pass_start = pass.end;
        unprinted
    });
    if unprinted_padding
        || buffer.has_zero_width_retreats()
        || passes
            .last()
            .is_some_and(|pass| buffer.projection_length(pass.end, buffer.cells().len()) > 0)
    {
        retain_unit_owner_ranges(nodes, buffer, anchors, passes, output_start, true);
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
    accepted_empty_tail
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
    preserve_literal_whitespace: bool,
) {
    let accepted_owners = super::super::output::native_passes::accepted_owner_lengths(
        buffer,
        anchors,
        passes,
        preserve_literal_whitespace,
    );
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
