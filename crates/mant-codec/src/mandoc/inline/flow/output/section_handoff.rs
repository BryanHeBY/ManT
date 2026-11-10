//! A plain native input interval can span two source section owners.

use super::super::field_buffer::{FieldBuffer, FieldCell};
use super::{Inline, InlineBuilder, projection};
use std::ops::Range;

pub(super) const BOUNDARY: &str = "\0mant:output-scope:section-handoff";

/// The old source owner's final input coordinate. Its provisional projection
/// remains in the same Vec until HEAD's real `term_newln` consumes both owners.
pub(in crate::mandoc::inline::flow) struct SectionHandoff {
    previous_cell_end: usize,
    previous_word_serial: u64,
    rejected_previous: bool,
    previous_empty_passes: u16,
    previous_literal: bool,
    projection_start: usize,
    last_pass_shares_unprinted_row: bool,
}

impl SectionHandoff {
    pub(in crate::mandoc::inline::flow) fn note_projection_start(&mut self, start: usize) {
        self.projection_start = start;
    }

    fn owns_loop_end(&self, buffer: &FieldBuffer, pass: Range<usize>) -> bool {
        pass.end <= self.previous_cell_end
            || pass.start < self.previous_cell_end
                && buffer.printed_content_end(pass.start, self.previous_cell_end) > pass.start
                && buffer.printed_content_end(self.previous_cell_end, pass.end)
                    == self.previous_cell_end
    }

    pub(in crate::mandoc::inline::flow) fn note_heading_word(&mut self, start: usize) {
        // Use the stable word range, as in accepted_owner_lengths(). A
        // heading's first write can land before the original seam after
        // BACKBEFORE or a supported rewind (term.c:678–714,901–908).
        self.previous_cell_end = self.previous_cell_end.min(start);
    }

    /// Update the old owner's tail from accepted input, independently of
    /// where the device row ends. Return whether the old owner owns the
    /// actual loop endline (term.c:143–217).
    pub(in crate::mandoc::inline::flow) fn observe_accepted_pass(
        &mut self,
        buffer: &FieldBuffer,
        pass: Range<usize>,
        ends_line: bool,
        printed_row: bool,
        covered: &mut &[usize],
    ) -> bool {
        let closes_previous = ends_line && self.owns_loop_end(buffer, pass.clone());
        if !ends_line && !printed_row {
            self.last_pass_shares_unprinted_row = pass.start < self.previous_cell_end
                && buffer.projection_length(pass.start, pass.end.min(self.previous_cell_end)) > 0;
        }
        if self.previous_empty_passes > 0
            && self.has_surviving_previous_glyph(buffer, pass, covered)
        {
            // Earlier empty rows are now interior; their inline breaks
            // already represent them. HEAD's own glyphs cannot do this.
            self.previous_empty_passes = 0;
        }
        if closes_previous && !printed_row {
            self.previous_empty_passes = self.previous_empty_passes.saturating_add(1);
        }
        closes_previous
    }

    fn has_surviving_previous_glyph(
        &self,
        buffer: &FieldBuffer,
        pass: Range<usize>,
        covered: &mut &[usize],
    ) -> bool {
        let end = pass.end.min(self.previous_cell_end);
        (pass.start..end).any(|cell| {
            // encode1() writes a \z glyph into its original word's cells.
            // It may later lose its projection to an overstrike. For
            // covers that depend on the accepted pass, use the same
            // deferred cover evidence as the word-acceptance projector.
            while covered.first().is_some_and(|&first| first < cell) {
                *covered = &covered[1..];
            }
            matches!(
                buffer.cells()[cell],
                FieldCell::Graph { .. } | FieldCell::Hyphen
            ) && buffer.projection_length(cell, cell + 1) > 0
                && covered.first() != Some(&cell)
        })
    }
}

impl InlineBuilder {
    pub(in crate::mandoc) fn has_live_section_input(
        execution: &super::super::InlineExecutionState,
        nodes: &[Inline],
    ) -> bool {
        let input = &execution.plain_input;
        !nodes.is_empty()
            && execution.definition.is_none()
            && input.buffer.resume_offset() < input.buffer.cells().len()
            && (projection::has_rendered_formatter_glyph(
                &nodes[input.output_start.min(nodes.len())..],
            ) || execution.zero_advance.has_buffered_glyph())
    }

    pub(in crate::mandoc) fn mark_section_handoff(&mut self, previous_literal: bool) {
        self.section_handoff = Some(SectionHandoff {
            previous_cell_end: self.execution.plain_input.buffer.cells().len(),
            previous_word_serial: self.execution.native_owner_serial,
            rejected_previous: false,
            previous_empty_passes: 0,
            previous_literal,
            projection_start: 0,
            last_pass_shares_unprinted_row: false,
        });
        self.nodes.push(Inline::anchor(BOUNDARY));
    }

    pub(super) fn route_section_loop_end(&mut self, pass: Range<usize>, indent: u16) -> bool {
        if !self
            .section_handoff
            .as_ref()
            .is_some_and(|handoff| handoff.owns_loop_end(&self.execution.plain_input.buffer, pass))
        {
            return false;
        }
        // An endline whose accepted pass ends within the old input closes
        // the old owner. A generated separator before HEAD is not a new row.
        let boundary = self.section_handoff_boundary();
        let head = self.nodes.split_off(boundary);
        super::push_row_break(&mut self.nodes, indent);
        self.nodes.extend(head);
        true
    }

    /// `encode1()` writes a zero-advance glyph to its original word before
    /// the next word can overwrite or release it (term.c:901-920).
    /// Its projection can arrive with HEAD, but must precede the old
    /// word's loop end and keep its own word/annotation anchor.
    pub(super) fn route_section_borrowed_prefix(&mut self, payload: &mut Vec<Inline>) {
        let Some(handoff) = &self.section_handoff else {
            return;
        };
        let markers = payload.iter().enumerate().filter_map(|(index, node)| {
            super::native_passes::native_word_marker(node)
                .and_then(|marker| marker.strip_prefix(super::INTERNAL_FIELD_WORD))
                .and_then(|serial| serial.parse::<u64>().ok())
                .map(|serial| (index, serial))
        });
        let mut markers = markers.peekable();
        if markers
            .peek()
            .is_none_or(|(_, serial)| *serial > handoff.previous_word_serial)
        {
            return;
        }
        let end = markers
            .find(|(_, serial)| *serial > handoff.previous_word_serial)
            .map_or(payload.len(), |(index, _)| index);
        let boundary = self.section_handoff_boundary();
        self.nodes.splice(boundary..boundary, payload.drain(..end));
    }

    fn section_handoff_boundary(&self) -> usize {
        self.nodes
            .iter()
            .rposition(|node| matches!(node, Inline::Anchor { id, .. } if id.as_str() == BOUNDARY))
            .expect("section owner boundary is live")
    }

    pub(super) fn note_section_rejection(&mut self, pass_start: usize) {
        if let Some(handoff) = &mut self.section_handoff {
            handoff.rejected_previous |= pass_start < handoff.previous_cell_end;
        }
    }

    pub(in crate::mandoc) fn take_section_handoff_prefix(&mut self) -> Vec<Inline> {
        let handoff = self.section_handoff.take().expect("active section handoff");
        let (mut previous, mut head, found) =
            projection::split_output_scope_prefix(std::mem::take(&mut self.nodes), BOUNDARY);
        assert!(found, "section owner boundary survives native acceptance");
        let mut rows = handoff.previous_empty_passes;
        if handoff.previous_literal && (rows > 0 || handoff.last_pass_shares_unprinted_row) {
            let start = handoff.projection_start.min(previous.len());
            let projected = super::ProjectedTail::of(&previous[start..]);
            rows = rows.saturating_sub(projected.literal_owned_rows());
            if handoff.last_pass_shares_unprinted_row && projected.has_open_reading_row() {
                // The last accepted empty pass still spans the seam.
                // The old literal's retained payload already supplies
                // this final row; HEAD's endline is not another blank.
                projection::retain_inline_identities(&mut head);
            }
        }
        previous.extend(std::iter::repeat_n(
            super::RowReceipt::Completed(super::CompletedRowOrigin::Layout).anchor(),
            usize::from(rows),
        ));
        if handoff.rejected_previous && !projection::has_rendered_formatter_glyph(&head) {
            // The graphless pass began in the old owner's still-buffered
            // suffix (term.c:143-146). It produced one final empty row,
            // while accepting no HEAD glyph. Publish that row with its
            // original input rather than as a phantom heading or BODY row.
            previous.push(super::RowReceipt::Completed(super::CompletedRowOrigin::Layout).anchor());
            projection::retain_inline_identities(&mut head);
        }
        self.nodes = head;
        previous
    }
}
