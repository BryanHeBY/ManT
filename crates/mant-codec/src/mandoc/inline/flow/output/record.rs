//! Record source words before projecting their accepted native passes.

use super::super::{Inline, InlineBuilder, PendingBoundary, WordProjectionPolicy};
use super::source_blanks::take_retreated_source_blank;

mod input;
mod passes;
use input::{ExecutedWordEntry, RecordedWordInput, observe_definition_word};

#[derive(Default)]
pub(super) struct WordPassProjection {
    pub(super) closes_before: Vec<std::ops::Range<usize>>,
    pub(super) leading_padding: RestartedPadding,
    pub(super) native_separator: usize,
    pub(super) preserve_native_separator: bool,
    pub(super) consumed_initial_separator: bool,
    pub(super) prints_padding: bool,
    pub(super) authored_separator: Option<Inline>,
    pub(super) split_word: Option<Vec<Inline>>,
}

#[derive(Clone, Copy)]
pub(super) enum RestartedPadding {
    CurrentWord(usize),
    WithEarlierOwners { total: usize, current: usize },
}

impl Default for RestartedPadding {
    fn default() -> Self {
        Self::CurrentWord(0)
    }
}

impl RestartedPadding {
    pub(super) fn remaining(self, earlier_cells: usize) -> usize {
        match self {
            Self::CurrentWord(cells) => cells,
            Self::WithEarlierOwners { total, .. } => total.saturating_sub(earlier_cells),
        }
    }
    pub(super) const fn current_word_cells(self) -> usize {
        match self {
            Self::CurrentWord(cells) => cells,
            Self::WithEarlierOwners { current, .. } => current,
        }
    }
}

impl WordPassProjection {
    fn record_separator(
        &mut self,
        buffer: &super::super::field_buffer::FieldBuffer,
        word_start: usize,
        written: usize,
        surviving: usize,
    ) {
        self.native_separator = buffer.projectable_padding(word_start, word_start + surviving);
        self.preserve_native_separator = written == 2 && surviving == 1;
        self.consumed_initial_separator = surviving > 0 && self.native_separator == 0;
    }

    fn align_separator_owner(&self, anchor: &mut super::super::NativeWordAnchor, content: usize) {
        if self.consumed_initial_separator {
            // The accepted range is traversed with the same scalar
            // coordinates as its projected payload. The skipped prefix
            // must not reserve a slot in the current word.
            anchor.content = anchor.content.max(content);
        }
    }
}

impl InlineBuilder {
    pub(super) fn record_hang_word(
        &mut self,
        incoming: &mut [Inline],
        boundary: PendingBoundary,
        projection: WordProjectionPolicy,
    ) -> WordPassProjection {
        if self.execution.native_word_owner.is_none() {
            self.begin_native_word_owner();
        }
        let input = RecordedWordInput::take(&mut self.execution, boundary, projection);
        if !self.execution.uses_definition_word_buffer() {
            return self.record_plain_unit_word(incoming, input);
        }
        let RecordedWordInput {
            native_writes,
            native_boundary,
            sentence,
            projected_keep,
            projection,
        } = input;
        let materialized_field_padding = self.projected_native_field_padding();
        let Some(definition) = &mut self.execution.definition else {
            return WordPassProjection::default();
        };
        // term_word() buffers its separator and glyph in the native field.
        // Generated IR padding is excluded from that field.
        let native_word_start = definition.field_buffer.cells().len();
        let separator = definition.field_buffer.begin_word_with_spacing(
            native_boundary.is_native_tight(),
            native_boundary == PendingBoundary::Kept,
            1 + usize::from(sentence),
        );
        if let Some(boundary) = definition.run_in_boundaries.last_mut() {
            boundary.observe_following_word(separator);
        }
        // The text executor supplied native writes before IR projection.
        // A stable owner marker follows wrappers and compact presentation;
        // it never contributes a native cell.
        let anchor_ir_start = self
            .execution
            .native_word_owner
            .take()
            .expect("native word owner");
        // Every source and generated formatter-word entry supplies writes.
        // Empty operands supply an explicit empty write list, preserving the
        // word event without recovering graph facts from semantic output.
        // Register the anchor only AFTER execution, from the receipt: a
        // BACKBEFORE retreat (term.c:901-908) can pop the separator blank
        // recorded above, and a pre-execution content start would put
        // already-accepted cells into a later rejected interval.
        let entry = ExecutedWordEntry::execute(
            &mut definition.field_buffer,
            native_writes.expect("formatter word native writes"),
            native_word_start,
            separator,
            &definition.field_word_anchors,
        );
        entry.project_rewind(
            &mut self.nodes,
            incoming,
            &anchor_ir_start,
            &definition.field_word_anchors,
        );
        let receipt = &entry.receipt;
        let native_separator = entry.surviving_separator;
        let authored_separator =
            take_retreated_source_blank(incoming, &anchor_ir_start, receipt.leading_retreat);
        definition
            .field_word_anchors
            .push(super::super::NativeWordAnchor {
                start: native_word_start,
                owner: anchor_ir_start,
                content: if native_separator > 0
                    && self.execution.pending_field_spaces == 0
                    && (!boundary.is_tight()
                        || (self.execution.settled_boundary_glyph && !receipt.prints_padding)
                        || self.execution.zero_advance_joined
                        || receipt.leading_retreat.is_some()
                        || projected_keep
                        || entry.rewound())
                {
                    native_word_start
                } else {
                    receipt.first_content_cell
                },
                projected_device_padding: if receipt.prints_padding {
                    self.execution.pending_field_spaces
                } else {
                    0
                }
                .max(materialized_field_padding),
                projected_field_prefix: false,
                projection,
            });
        observe_definition_word(definition, incoming, separator);
        let mut projection =
            self.record_authored_native_passes(incoming, receipt.first_content_cell);
        let (buffer, _) = self.execution.word_input();
        projection.record_separator(buffer, native_word_start, separator, native_separator);
        entry.record_projection(&mut projection);
        if let Some(anchor) = self
            .execution
            .definition
            .as_mut()
            .and_then(|definition| definition.field_word_anchors.last_mut())
        {
            projection.align_separator_owner(anchor, receipt.first_content_cell);
        }
        projection.preserve_native_separator |= receipt.leading_retreat.is_some() || projected_keep;
        projection.prints_padding = receipt.prints_padding;
        projection.authored_separator = authored_separator;
        projection
    }

    /// Plain-flow word recording: the paragraph's flush unit takes the same
    /// `term_word()` cell sequence (separator, writes, anchor) as a field.
    /// It has no hang-row geometry; only the marker pass loop can act.
    fn record_plain_unit_word(
        &mut self,
        incoming: &mut [Inline],
        input: RecordedWordInput,
    ) -> WordPassProjection {
        let RecordedWordInput {
            native_writes,
            native_boundary,
            sentence,
            projected_keep,
            projection,
        } = input;
        // term_flushln() retired the old native unit before this word.
        // The IR destination may since have changed (D1/Dl, heading, link
        // label). Establish its new output interval here, before writing
        // the word; a Vec offset from the former owner is not transferable.
        self.execution.plain_input.begin_output_at(self.nodes.len());
        let native_word_start = self.execution.plain_input.buffer.cells().len();
        let separator = self.execution.plain_input.buffer.begin_word_with_spacing(
            native_boundary.is_native_tight(),
            native_boundary == PendingBoundary::Kept,
            1 + usize::from(sentence),
        );
        let anchor_ir_start = self
            .execution
            .native_word_owner
            .take()
            .expect("native word owner");
        let entry = ExecutedWordEntry::execute(
            &mut self.execution.plain_input.buffer,
            native_writes.unwrap_or_default(),
            native_word_start,
            separator,
            &self.execution.plain_input.anchors,
        );
        entry.project_rewind(
            &mut self.nodes,
            incoming,
            &anchor_ir_start,
            &self.execution.plain_input.anchors,
        );
        let receipt = &entry.receipt;
        let native_separator = entry.surviving_separator;
        let authored_separator =
            take_retreated_source_blank(incoming, &anchor_ir_start, receipt.leading_retreat);
        self.execution
            .plain_input
            .anchors
            .push(super::super::NativeWordAnchor {
                start: native_word_start,
                owner: anchor_ir_start,
                content: if native_separator > 0
                    && !native_boundary.is_tight()
                    && (!self.execution.boundary.is_tight()
                        || self.execution.zero_advance_joined
                        || receipt.leading_retreat.is_some()
                        || projected_keep
                        || entry.rewound())
                {
                    native_word_start
                } else {
                    receipt.first_content_cell
                },
                projected_device_padding: 0,
                projected_field_prefix: false,
                projection,
            });
        // The plain unit is a BRNEVER-shaped degenerate field (term.c:134,
        // 143-144): responsive reflow owns width, so passes only ever end
        // at authored markers and never at a device-width guess.
        let targets = super::super::field_buffer::FillTargets {
            first: usize::MAX / 2,
            rest: usize::MAX / 2,
            unbounded: true,
        };
        let cached_glyph = self
            .execution
            .zero_advance
            .buffered_glyph_belongs_to_current_word();
        let unit = &mut self.execution.plain_input;
        let mut result = Self::native_unit_passes(
            incoming,
            &mut unit.buffer,
            &mut unit.anchors,
            targets,
            receipt.first_content_cell,
            cached_glyph,
        );
        result.record_separator(&unit.buffer, native_word_start, separator, native_separator);
        entry.record_projection(&mut result);
        if let Some(anchor) = unit.anchors.last_mut() {
            result.align_separator_owner(anchor, receipt.first_content_cell);
            if let Some(handoff) = &mut self.section_handoff {
                handoff.note_heading_word(anchor.start.min(anchor.content));
            }
        }
        result.preserve_native_separator |= receipt.leading_retreat.is_some() || projected_keep;
        result.prints_padding = receipt.prints_padding;
        result.authored_separator = authored_separator;
        result
    }
}
