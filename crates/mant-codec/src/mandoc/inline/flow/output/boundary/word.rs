//! Append one recorded word, keeping its separators and earlier owners.

use super::{Inline, InlineBuilder, PendingBoundary, push_text, trim_trailing_breakable_spaces};

use super::super::native_passes;
use super::super::record::{SeparatorDisposition, WordPassProjection};

struct ReplayedMotionPadding {
    device: usize,
    separator: usize,
}

impl InlineBuilder {
    pub(super) fn append_recorded_word_boundary(
        &mut self,
        boundary: PendingBoundary,
        word: bool,
        empty_word: bool,
        add_space: bool,
        projection: &mut super::super::record::WordPassProjection,
        payload: &mut Vec<Inline>,
    ) -> usize {
        let accepted_row_break = !projection.closes_before.is_empty();
        let native_separator = projection.native_separator;
        let empty_word = empty_word && !self.pending_word_prints_device_gap(projection);
        let transferred_automatic =
            self.transfer_automatic_padding_to_empty_word(empty_word, native_separator);
        // Plain words anchor into the same native flush unit the field
        // path uses; the marker routes by session exactly like the cell
        // recording in `record_native_word`.
        let native_anchor_marker = self
            .execution
            .word_input()
            .1
            .last()
            .map(|anchor| anchor.owner.clone());
        if !accepted_row_break {
            // The new word's owner starts after earlier buffered glyphs,
            // even on a tight join. A deferred term_fill() pass can restart
            // exactly at the current word's first slot (term.c:205-220).
            self.nodes.extend(take_borrowed_word_prefix(
                payload,
                native_anchor_marker.as_deref(),
            ));
        }
        let layout_start = self.nodes.len();
        let earlier_spaces = if !accepted_row_break && !empty_word {
            self.materialize_pending_word_spaces()
        } else {
            0
        };
        if word
            && (self.execution.pending_field_spaces == 0
                || accepted_row_break
                || !projection.prints_padding)
            && let Some(marker) = native_anchor_marker.as_ref()
        {
            // An accepted marker pass consumes incoming blank positioning
            // at its real endline (term.c:205-217). It bypasses the padding
            // branch below, but still needs this word's stable output owner.
            self.nodes.push(Inline::anchor(marker.clone()));
        }
        if accepted_row_break {
            self.append_restarted_word_padding(
                projection,
                empty_word,
                native_anchor_marker.as_deref(),
                payload,
            )
        } else {
            // Literal rows preserve the actual buffered separator even
            // after a zero-width or empty word. An IR character predicate
            // cannot establish NOSPACE (term.c:573-589); BACKBEFORE may
            // already have consumed the blank. Filled responsive words keep
            // their projected zero-advance joins, but authored leading blanks
            // do not replace term_word()'s separate automatic separator.
            let (spacing_boundary, add_space) = if transferred_automatic {
                (boundary, false)
            } else {
                self.recorded_spacing_boundary(boundary, word, empty_word, add_space, projection)
            };
            // term.c::term_field() skips NBRZW, markers and tab/blank
            // positioning before it writes vbl at an actual Graph. An
            // invisible accepted word must not print and then retire the
            // same minbl again at the next pre_br (term.c:389-427).
            let field_padding =
                self.execution.pending_field_spaces > 0 && projection.prints_padding;
            let add_space = add_space && (native_separator > 0 || earlier_spaces == 0);
            let projects_spacing = self.projects_automatic_space(spacing_boundary, add_space);
            let virtual_padding = !empty_word && native_separator == 0 && projects_spacing;
            let replayed_gap = self.locate_replayed_entry_gap(
                projection,
                word && !empty_word && !field_padding,
                projects_spacing,
            );
            let authored_in_payload = native_separator > 0
                && !field_padding
                && projects_spacing
                && replayed_gap.is_none()
                && reattach_authored_separator(
                    payload,
                    native_anchor_marker.as_deref(),
                    projection.authored_separator.take(),
                );
            let motion_padding = (!field_padding && projection.retained_motion_width > 0)
                .then(|| self.replayed_motion_padding(projection, payload, replayed_gap));
            let prefix_prints = motion_padding
                .as_ref()
                .map_or(virtual_padding || replayed_gap.is_some(), |padding| {
                    padding.device > 0
                });
            let prefix_marker =
                native_anchor_marker.filter(|_| word && (field_padding || prefix_prints));
            self.begin_word_padding(
                prefix_marker.as_deref(),
                replayed_gap,
                projection,
                field_padding,
            );
            if field_padding {
                self.append_device_field_padding();
            } else if let Some(padding) = &motion_padding {
                if padding.device > 0 {
                    self.push_field_owned_breakable_space(" ".repeat(padding.device));
                }
            } else {
                self.append_boundary_spacing(
                    spacing_boundary,
                    add_space,
                    empty_word,
                    if transferred_automatic {
                        0
                    } else {
                        native_separator
                    },
                    authored_in_payload,
                );
            }
            self.end_word_padding(prefix_marker, motion_padding);
            layout_start
        }
    }

    fn begin_word_padding(
        &mut self,
        owner: Option<&str>,
        replayed_gap: Option<usize>,
        projection: &WordPassProjection,
        field_padding: bool,
    ) {
        if let Some(owner) = owner {
            self.mark_native_field_prefix(
                owner,
                replayed_gap.or(projection.projected_gap(field_padding)),
            );
        }
    }

    fn end_word_padding(&mut self, owner: Option<String>, motion: Option<ReplayedMotionPadding>) {
        if let Some(owner) = owner {
            // The word's scalar interval starts after device positioning.
            self.nodes.push(Inline::anchor(owner));
        }
        if let Some(padding) = motion.filter(|padding| padding.separator > 0) {
            self.push_field_owned_breakable_space(" ".repeat(padding.separator));
        }
    }

    fn replayed_motion_padding(
        &mut self,
        projection: &WordPassProjection,
        payload: &[Inline],
        replayed_gap: Option<usize>,
    ) -> ReplayedMotionPadding {
        let width = projection.retained_motion_width;
        // A replay can move an earlier blank into the leading payload.
        // Count both sides of the seam before projecting the remaining advance.
        let mut before_ink = true;
        let mut leading = 0;
        mant_ir::visit_inline_plain_text(payload, |text| {
            if before_ink {
                let spaces = text.len() - text.trim_start_matches(' ').len();
                leading += spaces;
                before_ink = spaces == text.len();
            }
        });
        let trailing = super::super::trailing_ascii_spaces(&self.nodes);
        let represented = trailing.saturating_add(leading);
        if represented > width {
            trim_trailing_breakable_spaces(&mut self.nodes, trailing.min(represented - width));
        }
        let remaining = width.saturating_sub(represented);
        // The prefix's geometric advance is outside the source scalar
        // stream. A contiguous separator with a proved slot in the
        // incoming word still needs its own scalar when neither a prior
        // replay receipt nor the payload already accounts for it.
        let separator = if leading == 0 && replayed_gap.is_none() {
            projection.first_payload_cell.map_or(0, |first| {
                use super::super::super::field_buffer::FieldCell;
                let (buffer, owners) = self.execution.word_input();
                owners.last().map_or(0, |owner| {
                    let prefix = owner.content.min(first)..first;
                    let writes = &buffer.cells()[prefix.clone()];
                    // A leading HORIZ can be part of the current word's
                    // prefix. Keep that device run cohesive. Likewise an
                    // internal pending break with no preceding projected
                    // space cannot yet divide its entry advance.
                    let owns_motion = trailing > 0
                        && writes.iter().any(|cell| {
                            matches!(
                                cell,
                                FieldCell::FixedDrawingAdvance(_) | FieldCell::DrawnSpaces(_)
                            )
                        });
                    let owns_break = trailing == 0 && buffer.has_break_marker_at_or_after(first);
                    if owns_motion || owns_break {
                        0
                    } else {
                        buffer
                            .projection_length(prefix.start, prefix.end)
                            .min(projection.native_separator)
                            .min(remaining)
                    }
                })
            })
        } else {
            0
        };
        ReplayedMotionPadding {
            device: remaining - separator,
            separator,
        }
    }

    fn pending_word_prints_device_gap(&self, projection: &WordPassProjection) -> bool {
        // Geometry can prove an advance before the glyph it has left in
        // BACKBEFORE. Project that gap before the delayed glyph even
        // while this word's current inline payload is empty.
        matches!(projection.separator, SeparatorDisposition::DeviceAdvance(_))
            && self
                .execution
                .zero_advance
                .buffered_glyph_belongs_to_current_word()
    }

    pub(super) fn append_restarted_word_padding(
        &mut self,
        projection: &mut super::super::record::WordPassProjection,
        empty_word: bool,
        native_anchor_marker: Option<&str>,
        payload: &mut Vec<Inline>,
    ) -> usize {
        // A consumed \p separator closes the already accepted prefix.
        // Its blank is part of the break, not a new formatter word cell.
        trim_trailing_breakable_spaces(&mut self.nodes, usize::MAX);
        for pass in &projection.closes_before {
            let row_indent = self.take_definition_row_indent();
            if !self.route_section_loop_end(pass.clone(), row_indent) {
                super::super::push_row_break(&mut self.nodes, row_indent);
            }
            self.execution.acknowledge_word_input_pass(pass.end);
        }
        if matches!(
            projection.leading_padding,
            super::super::record::RestartedPadding::WithEarlierOwners { .. }
        ) && let Some(marker) = native_anchor_marker
        {
            // The borrowed payload is older than the queued empty
            // words. Its held source blanks and recovery text retain
            // their place before those independently owned separators.
            self.nodes.extend(take_word_prefix(payload, marker));
        }
        let layout_start = self.nodes.len();
        // The pass can stop skipping ordinary blanks at TABREF.
        // Only cells before the restarted cursor were consumed.
        let earlier_cells = if empty_word {
            0
        } else {
            self.materialize_pending_word_spaces()
        };
        // A borrowed glyph can put older separators in the same native
        // range. The deferred receipts already printed their owners.
        let leading_cells = projection.leading_padding.remaining(earlier_cells);
        if (earlier_cells > 0 || projection.leading_padding.current_word_cells() > 0)
            && let Some(marker) = native_anchor_marker
        {
            self.nodes.push(Inline::anchor(marker));
        }
        if leading_cells > 0 {
            let authored_in_payload = projection.leading_padding.current_word_cells() > 0
                && reattach_authored_separator(
                    payload,
                    native_anchor_marker,
                    projection.authored_separator.take(),
                );
            let generated = leading_cells - usize::from(authored_in_payload);
            if generated > 0 {
                push_text(&mut self.nodes, " ".repeat(generated));
            }
        }
        layout_start
    }

    pub(super) fn recorded_spacing_boundary(
        &self,
        boundary: PendingBoundary,
        word: bool,
        empty_word: bool,
        add_space: bool,
        projection: &super::super::record::WordPassProjection,
    ) -> (PendingBoundary, bool) {
        let native_separator = projection.native_separator;
        if projection.separator == SeparatorDisposition::Consumed {
            // The word wrote a blank, but an accepted pass or BACKBEFORE
            // consumed it. A presentation fallback cannot count it again.
            (boundary, false)
        } else if word
            && !empty_word
            && matches!(projection.separator, SeparatorDisposition::DeviceAdvance(_))
            && (projection.retained_motion_width > 0
                || self
                    .execution
                    .last_visible_character
                    .is_some_and(|character| !character.is_whitespace()))
        {
            (PendingBoundary::Preserved, true)
        } else if word
            && self.execution.no_fill_word_active
            && boundary == PendingBoundary::CommittedField
        {
            // The committed field already represents its positioning
            // and the following ordinary word blank. term_word() must
            // still buffer that blank after MC cleared NOSPACE, but
            // no-fill cannot project the same cell a second time
            // (roff_term.c:147-150; term.c:573-589,389-427).
            (boundary, false)
        } else if word && self.execution.no_fill_word_active {
            (
                if native_separator > 0 {
                    PendingBoundary::Preserved
                } else {
                    PendingBoundary::Tight
                },
                native_separator > 0,
            )
        } else if empty_word && self.execution.pending_field_spaces > 0 && native_separator > 0 {
            // A graphless word cannot print deferred vbl, but its
            // term_word() automatic blank is a distinct native cell
            // (term.c:573-589,389-427). Preserve it until a later graph
            // prints the field; pending positioning cannot consume it.
            (PendingBoundary::Preserved, true)
        } else if word
            && native_separator > 0
            && ((self.execution.settled_boundary_glyph
                && (empty_word || !projection.prints_padding))
                || !boundary.is_tight()
                || self.execution.zero_advance_joined
                || projection.separator == SeparatorDisposition::Keep)
        {
            // term_word()573-589 buffers this separator independently
            // of projected glyphs and authored leading/trailing blanks.
            // Its write receipt, rather than the last visible IR scalar,
            // proves the cell survived BACKBEFORE. Settling a prior glyph
            // provisionally selected Tight. A graphless word cannot yet
            // execute the actual retreat, but it can leave a later authored
            // blank as its target (term.c:901-908).
            (PendingBoundary::Preserved, true)
        } else {
            (boundary, add_space)
        }
    }
}

fn take_borrowed_word_prefix(payload: &mut Vec<Inline>, owner: Option<&str>) -> Vec<Inline> {
    if let (Some(first), Some(owner)) = (
        payload.first().and_then(native_passes::native_word_marker),
        owner,
    ) && first != owner
    {
        return take_word_prefix(payload, owner);
    }
    Vec::new()
}

fn take_word_prefix(payload: &mut Vec<Inline>, owner: &str) -> Vec<Inline> {
    let Some(index) = payload
        .iter()
        .position(|node| matches!(node, Inline::Anchor { id, .. } if id.as_str() == owner))
    else {
        return Vec::new();
    };
    let suffix = payload.split_off(index);
    std::mem::replace(payload, suffix)
}

/// An authored surrogate must enter with the word's payload, after the
/// annotation opens and any older borrowed owner has been emitted.
fn reattach_authored_separator(
    payload: &mut Vec<Inline>,
    owner: Option<&str>,
    separator: Option<Inline>,
) -> bool {
    let Some(separator) = separator else {
        return false;
    };
    let position = owner
        .and_then(|owner| {
            payload
                .iter()
                .position(|node| native_passes::native_word_marker(node) == Some(owner))
        })
        .map_or(0, |position| position + 1);
    payload.insert(position, separator);
    true
}
