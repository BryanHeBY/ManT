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
            // Plain flow shares the field's ordered cell consumer: the
            // paragraph is a degenerate flush unit (term.c runs one
            // term_fill() over `tcol->buf` regardless of authorship), so
            // its words take the same marker/pass/rejection arithmetic.
            return self.record_plain_unit_word(incoming, native_boundary, native_writes);
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

    /// Plain-flow word recording: the paragraph's flush unit takes the same
    /// `term_word()` cell sequence (separator, writes, anchor) as a field.
    /// It has no hang-row geometry; only the marker pass loop can act.
    fn record_plain_unit_word(
        &mut self,
        incoming: &[Inline],
        native_boundary: PendingBoundary,
        native_writes: Option<Vec<super::super::field_buffer::FieldWrite>>,
    ) -> (usize, Option<Vec<Inline>>) {
        let native_word_start = self.execution.flush_unit.cells().len();
        self.execution.flush_unit.begin_word(
            native_boundary.is_native_tight(),
            self.execution.spacing.enabled() || native_boundary == PendingBoundary::Preserved,
            native_boundary == PendingBoundary::Kept,
        );
        if self.execution.no_fill_word_active {
            // No-fill fragments retire rows outside the builder and keep
            // the text executor's own marker decisions
            // (`field_authoritative` is false there): buffer the cells for
            // upstream parity, but neither anchors nor the shared pass
            // loop may act on them.
            self.execution
                .flush_unit
                .apply_writes(&native_writes.unwrap_or_default());
            return (0, None);
        }
        let anchor_ir_start = format!(
            "{}{}-{}",
            super::INTERNAL_FIELD_WORD,
            self.execution.execution_epoch,
            native_word_start
        );
        let receipt = self
            .execution
            .flush_unit
            .apply_writes(&native_writes.unwrap_or_default());
        self.execution.flush_unit_anchors.push((
            native_word_start,
            anchor_ir_start,
            receipt.first_content_cell,
        ));
        // The plain unit is a BRNEVER-shaped degenerate field (term.c:134,
        // 143-144): responsive reflow owns width, so passes only ever end
        // at authored markers and never at a device-width guess.
        let targets = super::super::field_buffer::FillTargets {
            first: usize::MAX / 2,
            rest: usize::MAX / 2,
            unbounded: true,
        };
        let mut buffer = std::mem::take(&mut self.execution.flush_unit);
        let mut anchors = std::mem::take(&mut self.execution.flush_unit_anchors);
        let result = self.native_unit_passes(incoming, &mut buffer, &mut anchors, targets);
        self.execution.flush_unit = buffer;
        self.execution.flush_unit_anchors = anchors;
        result
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
        let Some(definition) = self.execution.definition.as_mut() else {
            return (0, None);
        };
        let mut buffer = std::mem::take(&mut definition.field_buffer);
        let mut anchors = std::mem::take(&mut definition.field_word_anchors);
        let result = self.native_unit_passes(incoming, &mut buffer, &mut anchors, targets);
        let Some(definition) = self.execution.definition.as_mut() else {
            return result;
        };
        definition.field_buffer = buffer;
        definition.field_word_anchors = anchors;
        result
    }

    /// The shared incremental pass loop (definition fields and plain flush
    /// units alike): commit only authored-marker passes, and report the row
    /// boundary positions relative to the word just recorded.
    #[allow(clippy::unused_self)] // shared by builder methods; state arrives by argument
    fn native_unit_passes(
        &mut self,
        incoming: &[Inline],
        buffer: &mut super::super::field_buffer::FieldBuffer,
        anchors: &mut [(usize, String, usize)],
        targets: super::super::field_buffer::FillTargets,
    ) -> (usize, Option<Vec<Inline>>) {
        let anchor_count = anchors.len();
        let mut inside_splits = Vec::new();
        let mut closes_before = 0;
        if anchor_count > 0 && !buffer.word_scan_deferred() && buffer.has_pending_break_markers() {
            let word_first_cell = anchors[anchor_count - 1].2;
            let word_end = buffer.cells().len();
            let mut ir_row_breaks = super::line_break_count(incoming);
            let mut first_pass = !buffer.has_committed_pass();
            while let Some(pass) = buffer.fill_pass_units(targets.scan(first_pass)) {
                if !buffer.has_non_ignorable_after(pass.end, false) {
                    break;
                }
                let pass_start = buffer.resume_offset();
                let mut boundary = pass.end;
                while matches!(
                    buffer.cells().get(boundary),
                    Some(super::super::field_buffer::FieldCell::BreakableBlank)
                ) {
                    boundary += 1;
                }
                // An unbounded target can never stop at a device-width
                // guess (term.c:134,143-144): every accepted pass there is
                // marker-driven, even when the marker sits exactly at the
                // resumed boundary (term.c:294-295 restarts at it).
                let authored = targets.unbounded
                    || buffer.cells()[pass_start..boundary].iter().any(|cell| {
                        matches!(cell, super::super::field_buffer::FieldCell::BreakMarker)
                    });
                if !authored {
                    // This pass only guessed a device width break. Do not
                    // commit its cursor or IR; the real flush owns it.
                    break;
                }
                buffer.commit_pass(pass, targets.actual(first_pass));
                let represented_in_ir = ir_row_breaks > 0;
                if represented_in_ir {
                    ir_row_breaks -= 1;
                }
                if boundary <= word_first_cell {
                    closes_before += usize::from(!represented_in_ir);
                } else if boundary < word_end && !represented_in_ir {
                    inside_splits.push(buffer.projection_length(word_first_cell, boundary));
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
