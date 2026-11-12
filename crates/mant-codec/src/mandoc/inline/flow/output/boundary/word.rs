//! Append one recorded word, keeping its separators and earlier owners.

use super::{Inline, InlineBuilder, PendingBoundary, push_text, trim_trailing_breakable_spaces};

use super::super::native_passes;

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
        // Plain MC's one projected next-word blank is an advance reservation,
        // unlike a definition field's minbl. Once an empty word actually
        // buffers that blank, transfer ownership to the word cells. The
        // following word then obeys its own NOSPACE/NONOSPACE registers
        // (roff_term.c:147-150; term.c:573-589), including .Sm off.
        let transferred_automatic = empty_word
            && native_separator > 0
            && self.execution.pending_field_gap_origin
                == super::super::super::definition::PendingFieldGapOrigin::AutomaticWord
            && self.execution.pending_field_spaces > 0;
        if transferred_automatic {
            let spaces = std::mem::take(&mut self.execution.pending_field_spaces);
            self.defer_current_word_spaces(spaces);
            self.execution.pending_field_gap_origin =
                super::super::super::definition::PendingFieldGapOrigin::Other;
        }
        // Plain words anchor into the same native flush unit the field
        // path uses; the marker routes by session exactly like the cell
        // recording in `record_native_word`.
        let native_anchor_marker = if self.in_definition_field() {
            self.execution
                .definition
                .as_ref()
                .and_then(|state| state.field_word_anchors.last())
        } else {
            self.execution.plain_input.anchors.last()
        }
        .map(|anchor| anchor.owner.clone());
        if !accepted_row_break && native_separator > 0 {
            // term_word() appends this word's separator after earlier
            // buffered glyphs, even when their projection arrives late.
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
            let virtual_padding = !empty_word
                && native_separator == 0
                && self.projects_automatic_space(spacing_boundary, add_space);
            let authored_in_payload = native_separator > 0
                && !field_padding
                && self.projects_automatic_space(spacing_boundary, add_space)
                && reattach_authored_separator(
                    payload,
                    native_anchor_marker.as_deref(),
                    projection.authored_separator.take(),
                );
            if word
                && (field_padding || virtual_padding)
                && let Some(marker) = native_anchor_marker.as_ref()
            {
                self.mark_native_field_prefix(marker);
            }
            if field_padding {
                self.append_device_field_padding();
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
            if word
                && (field_padding || virtual_padding)
                && let Some(marker) = native_anchor_marker
            {
                // Device minbl is field geometry, not a native buffer cell.
                // The word's accepted scalar interval begins after that pad.
                self.nodes.push(Inline::anchor(marker));
            }
            layout_start
        }
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
        if projection.consumed_initial_separator {
            // The word wrote a blank, but an accepted pass or BACKBEFORE
            // consumed it. A presentation fallback cannot count it again.
            (boundary, false)
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
                || projection.preserve_native_separator)
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
