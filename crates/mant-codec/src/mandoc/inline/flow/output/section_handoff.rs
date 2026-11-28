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
    shared_row: Option<SharedRow>,
    preceding_row_already_printed: bool,
    heading: HeadingKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SharedRow {
    PrintedTail,
    UnprintedTail,
    HeadingPrefix,
    PreviousPadding,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HeadingKind {
    Section,
    ManSubsection,
}

impl SectionHandoff {
    pub(in crate::mandoc::inline::flow) const fn continues_printed_row(&self) -> bool {
        self.preceding_row_already_printed
    }

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

    pub(super) fn closes_at_section_seam(
        &self,
        buffer: &FieldBuffer,
        pass: Range<usize>,
        next_owner_start: usize,
    ) -> bool {
        next_owner_start >= self.previous_cell_end && self.owns_loop_end(buffer, pass)
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
        let old_end = pass.end.min(self.previous_cell_end);
        let continues_detached = std::mem::take(&mut self.preceding_row_already_printed);
        let continues_previous = continues_detached
            || pass.start < old_end && buffer.printed_content_end(pass.start, old_end) > pass.start;
        let shared_padding = if self.heading == HeadingKind::ManSubsection
            && continues_previous
            && pass.end > self.previous_cell_end
        {
            // man_term.c::pre_SS can leave the previous field live at
            // PD=0. If only its fixed padding shares a later heading's
            // accepted pass, the heading supplies the reading row.
            has_accepted_ink(buffer, self.previous_cell_end..pass.end, covered)
                && (continues_detached
                    || pass.start < old_end && {
                        let previous = &buffer.cells()[pass.start..old_end];
                        !has_accepted_ink(buffer, pass.start..old_end, covered)
                            && previous.iter().any(fixed_padding)
                    })
        } else {
            false
        };
        if shared_padding {
            self.shared_row = Some(SharedRow::PreviousPadding);
        } else if !closes_previous
            && continues_previous
            && pass.end > self.previous_cell_end
            && !has_accepted_ink(buffer, self.previous_cell_end..pass.end, covered)
        {
            // term_field() printed the heading's fixed padding on the
            // preceding owner's row. A later pass may carry heading ink
            // or another independent blank row.
            self.shared_row = Some(if ends_line {
                SharedRow::HeadingPrefix
            } else {
                SharedRow::PrintedTail
            });
        } else if !ends_line
            && !matches!(
                self.shared_row,
                Some(SharedRow::HeadingPrefix | SharedRow::PreviousPadding)
            )
        {
            self.shared_row = if pass.start >= old_end {
                None
            } else if printed_row {
                (buffer.printed_content_end(pass.start, old_end) > pass.start)
                    .then_some(SharedRow::PrintedTail)
            } else {
                (buffer.projection_length(pass.start, old_end) > 0)
                    .then_some(SharedRow::UnprintedTail)
            };
        }
        if self.previous_empty_passes > 0
            && !shared_padding
            && self.has_surviving_previous_glyph(buffer, pass, covered)
        {
            // Earlier empty rows are now interior; their inline breaks
            // already represent them. HEAD's own ink, including a seam
            // where it borrows only fixed old padding, cannot do this.
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

    pub(in crate::mandoc) fn mark_section_handoff(
        &mut self,
        previous_output: bool,
        previous_literal: bool,
        subsection: bool,
    ) -> bool {
        let previous_cell_end = self.execution.plain_input.buffer.cells().len();
        let preceding_row_already_printed =
            subsection && previous_cell_end == 0 && self.execution.has_detached_printed_row();
        if !previous_output && !preceding_row_already_printed {
            return false;
        }
        // post_HP can drain its BODY buffer under NOBREAK while leaving
        // viscol live. An SS at PD=0 can finish that already projected row
        // as well as one whose earlier words are still in the input buffer.
        self.section_handoff = Some(SectionHandoff {
            previous_cell_end,
            previous_word_serial: self.execution.native_owner_serial,
            rejected_previous: false,
            previous_empty_passes: 0,
            previous_literal,
            projection_start: 0,
            shared_row: None,
            preceding_row_already_printed,
            heading: if subsection {
                HeadingKind::ManSubsection
            } else {
                HeadingKind::Section
            },
        });
        self.nodes.push(Inline::anchor(BOUNDARY));
        true
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
        if handoff.shared_row == Some(SharedRow::PreviousPadding) {
            // A detached row was already projected before the new buffer's
            // output_start. The caller lends exactly that row's payload.
            let start = if handoff.previous_cell_end == 0 {
                0
            } else {
                handoff.projection_start
            };
            move_previous_padding(&mut previous, &mut head, start);
        }
        if handoff.shared_row == Some(SharedRow::HeadingPrefix)
            || handoff.shared_row == Some(SharedRow::PrintedTail) && heading_has_only_padding(&head)
        {
            // `term_field()` printed this prefix on the live BODY row.
            // Move its surviving source padding, consuming the redundant
            // delimiter and leaving generated layout advances implicit.
            previous.extend(take_shared_heading_padding(
                &mut head,
                &mut SharedHeadingPadding::default(),
                false,
            ));
        }
        let shares_literal_tail = handoff.shared_row == Some(SharedRow::UnprintedTail);
        if handoff.previous_literal && (rows > 0 || shares_literal_tail) {
            let start = handoff.projection_start.min(previous.len());
            let projected = super::ProjectedTail::of(&previous[start..]);
            rows = rows.saturating_sub(projected.literal_owned_rows());
            if shares_literal_tail && projected.has_open_reading_row() {
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

impl super::super::InlineExecutionState {
    pub(in crate::mandoc) fn has_detached_printed_row(&self) -> bool {
        self.detached_device_row
            .is_some_and(|row| row.page_origin_printed || row.viscol > 0)
    }
}

#[derive(Default)]
struct SharedHeadingPadding {
    ended: bool,
    deferred_blank: bool,
}

/// Transfer authored whitespace from the accepted shared row. Style and
/// link wrappers are transparent to the delimiter, but their contents
/// keep the wrapper when they change output owners.
fn take_shared_heading_padding(
    nodes: &mut Vec<Inline>,
    state: &mut SharedHeadingPadding,
    inside_label: bool,
) -> Vec<Inline> {
    let mut padding = Vec::new();
    nodes.retain_mut(|node| {
        if state.ended {
            return true;
        }
        match node {
            Inline::LineBreak { .. } => {
                state.ended = true;
                false
            }
            Inline::Strong { children } => {
                let moved = take_shared_heading_padding(children, state, inside_label);
                if !moved.is_empty() {
                    padding.push(Inline::Strong { children: moved });
                }
                !children.is_empty()
            }
            Inline::Emphasis { children } => {
                let moved = take_shared_heading_padding(children, state, inside_label);
                if !moved.is_empty() {
                    padding.push(Inline::Emphasis { children: moved });
                }
                !children.is_empty()
            }
            Inline::Link {
                target,
                title,
                children,
            } => {
                let moved = take_shared_heading_padding(children, state, true);
                if moved.is_empty() {
                    return true;
                }
                padding.push(Inline::Link {
                    target: target.clone(),
                    title: title.clone(),
                    children: moved,
                });
                !children.is_empty()
            }
            Inline::Anchor { .. } => {
                if super::deferred_blanks::projected_blank_count(node).is_some() {
                    state.deferred_blank = true;
                }
                !super::is_private_output_marker(node)
            }
            Inline::Text { value } | Inline::Code { value }
                if value.chars().all(char::is_whitespace) =>
            {
                let deferred_blank = std::mem::take(&mut state.deferred_blank);
                // Unicode whitespace here is an accepted glyph. Device
                // padding uses ASCII cells; deferred runs carry an
                // explicit source witness for authored ASCII blanks.
                if !value.is_empty() && (deferred_blank || inside_label || !value.is_ascii()) {
                    padding.push(node.clone());
                }
                false
            }
            _ => {
                // A source recovery spelling may carry reading content
                // without native ink. Leave its projection intact.
                state.ended = true;
                true
            }
        }
    });
    padding
}

fn heading_has_only_padding(head: &[Inline]) -> bool {
    let mut only_padding = true;
    mant_ir::visit_inline_plain_text(head, |text| {
        only_padding &= text.chars().all(char::is_whitespace);
    });
    only_padding
}

fn ink(cell: &FieldCell) -> bool {
    matches!(cell, FieldCell::Hyphen)
        || matches!(cell, FieldCell::Graph { text, .. } if !text.is_whitespace())
}

fn has_accepted_ink(buffer: &FieldBuffer, cells: Range<usize>, covered: &[usize]) -> bool {
    let end = cells.end;
    cells.into_iter().any(|index| {
        let cell = &buffer.cells()[index];
        if !ink(cell) || covered.binary_search(&index).is_ok() {
            return false;
        }
        // encode1() writes font decorations as extra native bytes. A
        // decoration without an author scalar that is followed by its
        // backspace and a replacement graph cannot supply visible ink.
        if index + 2 < end
            && buffer.projection_length(index, index + 1) == 0
            && matches!(buffer.cells()[index + 1], FieldCell::Backline)
            && let (FieldCell::Graph { width: paint, .. }, FieldCell::Graph { width: next, .. }) =
                (cell, &buffer.cells()[index + 2])
        {
            return next < paint;
        }
        true
    })
}

fn fixed_padding(cell: &FieldCell) -> bool {
    matches!(cell, FieldCell::Graph { text, .. } if text.is_whitespace())
        || matches!(cell, FieldCell::NonBreakingBlank)
}

fn move_previous_padding(previous: &mut Vec<Inline>, heading: &mut Vec<Inline>, start: usize) {
    let start = start.min(previous.len());
    // A previous authored hard row, even an empty one, is not part of
    // this live tail. Never transfer its boundary or anything before it.
    let row = previous[start..]
        .iter()
        .rposition(|node| matches!(node, Inline::LineBreak {}))
        .map_or(start, |index| start + index + 1);
    let tail = &previous[row..];
    if heading_has_only_padding(tail) && projection::has_rendered_formatter_glyph(tail) {
        heading.splice(0..0, previous.drain(row..));
    }
}
