use super::super::{Inline, InlineBuilder, PendingBoundary};
use super::split::split_word_at_row_boundaries;

impl InlineBuilder {
    // Word accounting plus the native buffer feed; the length is the price
    // of keeping term.c's word-level and cell-level records side by side.
    #[allow(clippy::too_many_lines)]
    pub(super) fn record_hang_word(
        &mut self,
        incoming: &[Inline],
        boundary: PendingBoundary,
    ) -> (usize, Option<Vec<Inline>>) {
        let native_writes = self.execution.native_word_writes.take();
        let native_boundary = self
            .execution
            .native_word_boundary
            .take()
            .unwrap_or(boundary);
        if self.execution.definition.is_none() || self.execution.author_execution.is_none() {
            return (0, None);
        }
        let Some(definition) = &mut self.execution.definition else {
            return (0, None);
        };
        // term_word() buffers its separator and glyph in the native field.
        // Generated IR padding is excluded from that field.
        let native_word_start = definition.field_buffer.cells().len();
        let native_word_space = definition.field_buffer.begin_word(
            native_boundary.is_native_tight(),
            self.execution.spacing.enabled() || native_boundary == PendingBoundary::Preserved,
            native_boundary == PendingBoundary::Kept,
        ) > 0;
        let separator = usize::from(native_word_space);
        // The text executor supplied native writes before IR projection.
        // A stable owner marker follows wrappers and compact presentation;
        // it never contributes a native cell.
        let anchor_ir_start = format!(
            "{}{}-{}",
            super::INTERNAL_FIELD_WORD,
            self.execution.execution_epoch,
            native_word_start
        );
        // Every source and generated formatter-word entry supplies writes.
        // Empty operands supply an explicit empty write list, preserving the
        // word event without recovering graph facts from semantic output.
        // Register the anchor only AFTER execution, from the receipt: a
        // BACKBEFORE retreat (term.c:901-908) can pop the separator blank
        // recorded above, and a pre-execution content start would put
        // already-accepted cells into a later rejected interval.
        let receipt = definition
            .field_buffer
            .apply_writes(&native_writes.expect("formatter word native writes"));
        definition.field_word_anchors.push((
            native_word_start,
            anchor_ir_start,
            receipt.first_content_cell,
        ));
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
        if printable {
            definition.hang_row.field_last_unbreakable_width = trimmed
                .rsplit([' ', '\n'])
                .find(|part| !part.is_empty())
                .map_or(0, mant_ir::geometry::text_width);
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
        let native_row_breaks = if anchor_count > 0
            && (definition.field_buffer.has_pending_break_markers()
                || (definition.no_break_cleared
                    && !self.execution.no_fill_word_active
                    && definition.cleared_field_capacity_columns > 0))
        {
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
            let capacity = if definition.no_break_cleared && !self.execution.no_fill_word_active {
                usize::from(definition.cleared_field_capacity_columns)
            } else {
                usize::MAX / 2
            };
            let first_vtarget = capacity.saturating_sub(prefix_used);
            // Continuation rows restart at the field offset with the
            // request-cleared trailspace (roff_term.c:77): no minbl, no
            // prefix, so the plain capacity (term.c:124-125, 229-230).
            let rest_vtarget = capacity;
            {
                let word_first_cell = definition.field_word_anchors[anchor_count - 1].2;
                let word_end = definition.field_buffer.cells().len();
                let mut ir_row_breaks = super::line_break_count(incoming);
                let mut closes_before = 0;
                let mut first_pass = !definition.field_buffer.has_committed_pass();
                while let Some(pass) = definition.field_buffer.fill_pass(if first_pass {
                    first_vtarget
                } else {
                    rest_vtarget
                }) {
                    // The final word remains provisional until another
                    // genuine suffix exists. Only established pass ends
                    // advance the live cursor; history is never cloned.
                    if !definition
                        .field_buffer
                        .has_non_ignorable_after(pass.accepted_end, false)
                    {
                        break;
                    }
                    let pass_start = definition.field_buffer.resume_offset();
                    definition.field_buffer.commit_pass(pass);
                    let boundary = definition.field_buffer.resume_offset();
                    // A stop armed by a `\p` marker inside this pass is
                    // already represented in the word's IR: the text
                    // executor resolves the marker's line break at the
                    // same surviving blank term_fill() stopped at (the
                    // consumed cells are markers and break blanks, and
                    // `suppress_break_whitespace` kept them out of the
                    // projection). Re-inserting a boundary there would
                    // split the word one grapheme in. Only a stop with no
                    // marker in its pass range — a width overrun against
                    // the field capacity — still needs a mapped split,
                    // and only a trailing marker with no in-word break
                    // still needs a row close before the word.
                    let has_marker = definition.field_buffer.cells()[pass_start..boundary]
                        .iter()
                        .any(|cell| {
                            matches!(cell, super::super::field_buffer::FieldCell::BreakMarker)
                        });
                    let represented_in_ir = has_marker && ir_row_breaks > 0;
                    if represented_in_ir {
                        ir_row_breaks -= 1;
                    }
                    if boundary <= word_first_cell {
                        closes_before += usize::from(!represented_in_ir);
                    } else if boundary > word_first_cell
                        && boundary < word_end
                        && !represented_in_ir
                    {
                        let projected_boundary = definition
                            .field_buffer
                            .projection_length(word_first_cell, boundary);

                        inside_splits.push(projected_boundary);
                    }
                    if boundary >= word_end {
                        break;
                    }
                    first_pass = false;
                }
                closes_before
            }
        } else {
            0
        };
        let split_word = (!inside_splits.is_empty())
            .then(|| split_word_at_row_boundaries(incoming, &mut inside_splits));
        (native_row_breaks, split_word)
    }
}
