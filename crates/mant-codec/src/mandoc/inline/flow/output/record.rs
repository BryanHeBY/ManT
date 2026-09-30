use super::super::{Inline, InlineBuilder, PendingBoundary};
use super::split::split_word_at_row_boundaries;

impl InlineBuilder {
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
        self.record_authored_native_passes(incoming)
    }

    fn record_authored_native_passes(
        &mut self,
        incoming: &[Inline],
    ) -> (usize, Option<Vec<Inline>>) {
        // Width-dependent passes stay provisional until term_flushln(): a
        // later .ta may reinterpret every buffered tab. Only authored break
        // markers can settle a word's existing hard-break execution here.
        let Some(targets) = self.native_field_targets(false, None) else {
            return (0, None);
        };
        let definition = self.execution.definition.as_mut().expect("native field");
        let anchor_count = definition.field_word_anchors.len();
        let mut inside_splits = Vec::new();
        let mut closes_before = 0;
        if anchor_count > 0
            && !definition.field_buffer.word_scan_deferred()
            && definition.field_buffer.has_pending_break_markers()
        {
            let word_first_cell = definition.field_word_anchors[anchor_count - 1].2;
            let word_end = definition.field_buffer.cells().len();
            let mut ir_row_breaks = super::line_break_count(incoming);
            let mut first_pass = !definition.field_buffer.has_committed_pass();
            while let Some(pass) = definition
                .field_buffer
                .fill_pass_units(targets.scan(first_pass))
            {
                if !definition
                    .field_buffer
                    .has_non_ignorable_after(pass.end, false)
                {
                    break;
                }
                let pass_start = definition.field_buffer.resume_offset();
                let mut boundary = pass.end;
                while matches!(
                    definition.field_buffer.cells().get(boundary),
                    Some(super::super::field_buffer::FieldCell::BreakableBlank)
                ) {
                    boundary += 1;
                }
                let authored = definition.field_buffer.cells()[pass_start..boundary]
                    .iter()
                    .any(|cell| matches!(cell, super::super::field_buffer::FieldCell::BreakMarker));
                if !authored {
                    // This pass only guessed a device width break. Do not
                    // commit its cursor or IR; the real flush owns it.
                    break;
                }
                definition
                    .field_buffer
                    .commit_pass(pass, targets.actual(first_pass));
                let represented_in_ir = ir_row_breaks > 0;
                if represented_in_ir {
                    ir_row_breaks -= 1;
                }
                if boundary <= word_first_cell {
                    closes_before += usize::from(!represented_in_ir);
                } else if boundary < word_end && !represented_in_ir {
                    inside_splits.push(
                        definition
                            .field_buffer
                            .projection_length(word_first_cell, boundary),
                    );
                }
                if boundary >= word_end {
                    break;
                }
                first_pass = false;
            }
        }
        let split_word = (!inside_splits.is_empty())
            .then(|| split_word_at_row_boundaries(incoming, &mut inside_splits));
        (closes_before, split_word)
    }
}
