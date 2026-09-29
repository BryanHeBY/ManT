use super::super::{Inline, InlineBuilder, PendingBoundary, WordEndBreak};
use super::feed::feed_field_inline_with_breakpoints;
use super::split::split_word_at_row_boundaries;

impl InlineBuilder {
    // Word accounting plus the native buffer feed; the length is the price
    // of keeping term.c's word-level and cell-level records side by side.
    #[allow(clippy::too_many_lines)]
    pub(super) fn record_hang_word(
        &mut self,
        incoming: &[Inline],
        add_space: bool,
        boundary: PendingBoundary,
        empty_word: bool,
    ) -> (Option<usize>, bool, Option<Vec<Inline>>) {
        if self.execution.definition.is_none() || self.execution.author_execution.is_none() {
            return (None, false, None);
        }
        let Some(definition) = &mut self.execution.definition else {
            return (None, false, None);
        };
        let rejected_prefix =
            if let Some(prefix) = definition.hang_row.field_break_before_graph_prefix.take() {
                // term.c:293-295 with 143-146: a \p from an earlier word
                // left term_fill() resumed at this word's leading blank with
                // no accepted graph in that pass. The blank rejects the pass
                // before this word's own glyphs, so the in-word prefix is
                // unprinted buffer, not an accepted one.
                let cross_word_rejection = definition.hang_row.field_armed_at_word_start
                    && !definition.hang_row.field_native_graph_at_word_start;
                let accepted =
                    (prefix > 0 || definition.hang_row.field_native_graph) && !cross_word_rejection;
                definition.hang_row.field_discarded = true;
                definition.suffix_discarded_seen = true;
                if cross_word_rejection {
                    // The pass boundary predates this word: term_fill()
                    // resumed at the earlier word's armed blank. Keep the
                    // field output range recorded when that word armed
                    // (request_word_end_break), which already precedes
                    // every piece of this word.
                    None
                } else {
                    Some(if prefix > 0 {
                        prefix
                    } else if accepted {
                        incoming
                            .iter()
                            .position(|node| matches!(node, Inline::LineBreak { .. }))
                            .map_or(0, |index| index + 1)
                    } else {
                        0
                    })
                }
            } else {
                None
            };
        // term_word() buffers its separator and glyph in the native field.
        // Generated IR padding is excluded from that field.
        let native_word_space = (add_space || self.execution.empty_word)
            && (self.execution.spacing.enabled()
                || matches!(
                    boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued | PendingBoundary::Kept
                ))
            && !boundary.is_tight()
            && self.execution.pending_field_spaces == 0;
        let separator = if self.execution.pending_field_spaces > 0 {
            self.execution
                .pending_field_spaces
                .saturating_sub(definition.hang_row.minbl)
        } else {
            usize::from(native_word_space && !definition.hang_row.suppress_next_auto_space)
        };
        // Feed the native input buffer in source order (term.c's one flat
        // `tcol->buf`): the word's separator, a completed `\z` glyph that
        // is still pending in IR, then this word's cells with marker and
        // blank positions taken from the emitted IR itself.
        let anchor_ir_start = self.nodes.len();
        definition
            .field_word_anchors
            .push((definition.field_buffer.cells().len(), anchor_ir_start));
        let marker_fed_this_word = definition.trailing_marker_unfed;
        if definition.trailing_marker_unfed {
            // The PREVIOUS word's trailing \p deferred into the field:
            // its '\n' cell precedes this word's separator (term.c writes
            // it during that word's term_word(), term.c:657-658).
            definition.field_buffer.push_break_marker();
            definition.trailing_marker_unfed = false;
        }
        if empty_word {
            // term_word("") runs its head blank only (term.c:574-576): one
            // ordinary cell, never two.
            definition.field_buffer.push_separator_blank();
        } else if separator > 0 {
            definition.field_buffer.push_separator_blank();
        }
        let breakpoints = std::mem::take(&mut self.execution.word_zero_break_prefixes);
        let mut graph_count = 0;
        let mut next_breakpoint = 0;
        feed_field_inline_with_breakpoints(
            &mut definition.field_buffer,
            incoming,
            &mut graph_count,
            &mut next_breakpoint,
            &breakpoints,
        );
        // A pending `\z` glyph enters at its own word's tail
        // (term.c:886-929: encode1() writes it at the current buffer
        // position when the word ends, arming BACKBEFORE for the next
        // word's separator — term.c:924-927), never at the next word's
        // head.
        if let Some((text, width)) = self.execution.zero_advance.printable_pending_glyph_text()
            && !definition.pending_glyph_fed
        {
            let first = text.chars().next().unwrap_or(' ');
            definition.field_buffer.push_graph(first, width);
            definition.field_buffer.arm_backbefore();
            definition.pending_glyph_fed = true;
        }
        if self.execution.word_end_break == WordEndBreak::Pending {
            // This word's own trailing \p returned as the pending
            // word-end break; its '\n' cell follows the word's cells.
            definition.field_buffer.push_break_marker();
        }
        let accepted_prior_field = !definition.hang_row.field_discarded
            && separator > 0
            && definition.hang_row.field_pending_word_end_break
            && definition.hang_row.field_native_graph;
        definition.hang_row.accepted_prefix_before_rejection |= accepted_prior_field;
        let projected = super::super::super::plain_text(incoming);
        let trimmed = projected.trim_end_matches(' ');
        let trailing_spaces = projected.len().saturating_sub(trimmed.len());
        let width = mant_ir::geometry::text_width(trimmed);
        definition.hang_row.field_breakable |= trimmed.contains(' ');
        definition.hang_row.field_unproven_break |= trimmed.contains('\n');
        // term_fill() discards a field containing only ordinary breakable
        // blanks. A nonbreaking blank from \~ or \0 is a printable cell.
        let printable = trimmed
            .chars()
            .any(|ch| !super::super::super::is_formatter_word_blank(ch) && ch != '\n');
        definition
            .hang_row
            .word(separator, width, trailing_spaces, printable);
        // `word()` may itself discard the field (a \p-armed separator met
        // no graph); latch that rejection for the run-in BODY wipe.
        definition.suffix_discarded_seen |= definition.hang_row.field_discarded;
        if printable {
            definition.hang_row.field_last_unbreakable_width = trimmed
                .rsplit([' ', '\n'])
                .find(|part| !part.is_empty())
                .map_or(0, mant_ir::geometry::text_width);
        }
        let marker_row_break = definition.hang_row.accepted_prefix_before_rejection
            && printable
            && !definition.hang_row.field_discarded;
        if marker_row_break {
            definition.hang_row.accepted_prefix_before_rejection = false;
        }
        // term.c:134-136: with TERMP_NOBREAK cleared by a request, every
        // term_fill() pass targets `vfield` — the field's own capacity
        // `rmargin - offset` (constant across the head's passes: each
        // continuation restarts at the offset, term.c:229-230). A pass
        // boundary ends the device row (term.c:220); the blanks around it
        // belong to the break (term.c:205-207). Simulate the pass chain
        // over the fed buffer and map the boundaries into this word: one
        // at or before its first content cell closes the row before it
        // (the `accepted_row_break` channel); one inside it splits the
        // operand at the consumed blanks into separate device rows.
        let anchor_count = definition.field_word_anchors.len();
        let mut inside_splits: Vec<usize> = Vec::new();
        let width_row_break = definition.no_break_cleared
            && printable
            && !definition.hang_row.field_discarded
            && !self.execution.no_fill_word_active
            && anchor_count > 0
            && definition.cleared_field_capacity_columns > 0
            && {
                // term.c:113-116,124-125: the pass target subtracts the
                // flushed prefix at `viscol` and the row's `vbl`, which the
                // `minbl = trailspace` floor supplies (term.c:236; the hang
                // head leaves trailspace 1, mdoc_term.c:804-805). The
                // pinned W/WB probes put both flushes at viscol=1 column
                // with vbl=1: target = capacity - (viscol + 1).
                let prefix_used = if definition.hang_row.viscol > 0 {
                    definition.hang_row.viscol + 1
                } else {
                    0
                };
                let first_vtarget = usize::from(definition.cleared_field_capacity_columns)
                    .saturating_sub(prefix_used);
                // Continuation rows restart at the field offset with the
                // request-cleared trailspace (roff_term.c:77): no minbl, no
                // prefix, so the plain capacity (term.c:124-125, 229-230).
                let rest_vtarget = usize::from(definition.cleared_field_capacity_columns);
                {
                    let word_anchor = definition.field_word_anchors[anchor_count - 1].0;
                    let word_first_cell = word_anchor
                        + usize::from(marker_fed_this_word)
                        + usize::from(separator > 0 || empty_word);
                    let word_end = definition.field_buffer.cells().len();
                    let mut simulation = definition.field_buffer.clone();
                    let mut closes_before = false;
                    let mut first_pass = true;
                    while let Some(pass) = simulation.fill_pass(if first_pass {
                        first_vtarget
                    } else {
                        rest_vtarget
                    }) {
                        let accepted_end = pass.accepted_end;
                        simulation.advance_past(accepted_end);
                        simulation.consume_break_blanks();
                        let boundary = simulation.resume_offset();
                        if boundary > word_anchor && boundary <= word_first_cell {
                            closes_before = true;
                        } else if boundary > word_first_cell && boundary < word_end {
                            inside_splits.push(boundary - word_first_cell);
                        }
                        if boundary >= word_end {
                            break;
                        }
                        first_pass = false;
                    }
                    closes_before
                }
            };
        let accepted_row_break = width_row_break || marker_row_break;
        let split_word = (!inside_splits.is_empty())
            .then(|| split_word_at_row_boundaries(incoming, &mut inside_splits));
        (
            rejected_prefix.or_else(|| {
                marker_row_break
                    .then_some(incoming.len())
                    .or(accepted_prior_field.then_some(0))
            }),
            accepted_row_break,
            split_word,
        )
    }
}
