//! Record source words before projecting their accepted native passes.

use super::super::{
    Inline, InlineBuilder, PendingBoundary, ProjectedNativeGap, WordProjectionPolicy,
};
use super::source_blanks::take_retreated_source_blank;

mod input;
mod passes;
use input::{ExecutedWordEntry, RecordedWordInput, observe_definition_word};

#[derive(Default)]
pub(super) struct WordPassProjection {
    pub(super) closes_before: Vec<std::ops::Range<usize>>,
    pub(super) leading_padding: RestartedPadding,
    pub(super) native_separator: usize,
    pub(super) first_payload_cell: Option<usize>,
    pub(super) replayed_entry_gap: Option<usize>,
    pub(super) automatic_separator_slot: Option<usize>,
    pub(super) separator: SeparatorDisposition,
    pub(super) prints_padding: bool,
    pub(super) authored_separator: Option<Inline>,
    pub(super) split_word: Option<Vec<Inline>>,
    pub(super) retained_motion_width: usize,
    pub(super) retained_motion_hidden_blanks: usize,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SeparatorDisposition {
    #[default]
    Automatic,
    Keep,
    Consumed,
    DeviceAdvance(usize),
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
    pub(super) const fn projected_gap(&self, field_padding: bool) -> Option<usize> {
        match self.separator {
            SeparatorDisposition::DeviceAdvance(gap) if !field_padding => Some(gap),
            _ => None,
        }
    }

    fn record_separator(
        &mut self,
        buffer: &super::super::field_buffer::FieldBuffer,
        word_start: usize,
        written: usize,
        surviving: usize,
    ) {
        self.native_separator = buffer.projectable_padding(word_start, word_start + surviving);
        self.separator = if surviving > 0 && self.native_separator == 0 {
            SeparatorDisposition::Consumed
        } else if written == 2 && surviving == 1 {
            SeparatorDisposition::Keep
        } else {
            SeparatorDisposition::Automatic
        };
    }

    /// Advance `anchor.content` past a consumed entry separator (`term.c:573-581`)
    /// so `term_fill()`/`term_flushln()` cuts (`term.c:205-220`) align with the
    /// word's live payload rather than skipped prefix blanks.
    fn align_separator_owners(
        &self,
        anchors: &mut [super::super::NativeWordAnchor],
        initial_index: usize,
        content: usize,
    ) {
        let owner = anchors.last().map(|anchor| anchor.owner.clone());
        if let Some(owner) = owner
            && initial_index + 1 < anchors.len()
            && anchors[initial_index].owner == owner
        {
            let limit = anchors[initial_index + 1].content;
            self.align_separator_owner(&mut anchors[initial_index], content.min(limit));
        }
        if let Some(anchor) = anchors.last_mut() {
            self.align_separator_owner(anchor, content);
        }
    }

    fn align_separator_owner(&self, anchor: &mut super::super::NativeWordAnchor, content: usize) {
        if self.separator == SeparatorDisposition::Consumed {
            // The accepted range is traversed with the same scalar
            // coordinates as its projected payload. The skipped prefix
            // must not reserve a slot in the current word.
            anchor.content = anchor.content.max(content);
        }
    }

    fn record_separator_effects(
        &mut self,
        (buffer, anchors): (
            &super::super::field_buffer::FieldBuffer,
            &[super::super::NativeWordAnchor],
        ),
        word_start: usize,
        entry: &ExecutedWordEntry,
        keep: bool,
        plain_output: bool,
    ) {
        // The earlier \h cell is already in the live word buffer. Reuse
        // the virtual space projection when the new word would print it.
        let replayed = entry.has_replayed_slots();
        let gap_end = if replayed {
            entry.receipt.first_content_cell
        } else {
            word_start
        };
        if let Some(ink) = entry.incoming_ink()
            && let Some(gap) = buffer.retained_motion_gap(ink, word_start, plain_output)
            && gap.width >= self.native_separator
        {
            if plain_output
                && replayed
                && gap.previous_graph >= entry.receipt.first_content_cell
                && gap.projection_is_complete
            {
                // The replay payload already includes an earlier graph
                // and this complete separator. Printing the gap before
                // that payload would put a second copy on its left.
                self.separator = SeparatorDisposition::Consumed;
            } else {
                self.separator = SeparatorDisposition::DeviceAdvance(gap.marker);
                self.retained_motion_width = gap.width;
                self.retained_motion_hidden_blanks = gap.hidden_projected_blanks;
            }
        } else if self.native_separator == 0
            && entry.receipt.prints_padding
            && entry.receipt.first_content_cell >= word_start
            && let Some((gap, previous)) = buffer.device_gap_before(gap_end, replayed)
            && !entry.retains_blank(gap)
            && !(plain_output && buffer.has_deferred_blank_at(gap))
            // A scalar-bearing SP within the earlier ink's word is
            // already represented. An intervening invisible word's is not.
            && (buffer.projection_length(gap, gap + 1) == 0
                || anchors.partition_point(|anchor| anchor.content <= previous)
                    != anchors.partition_point(|anchor| anchor.content <= gap))
        {
            self.separator = SeparatorDisposition::DeviceAdvance(gap);
        } else if self.separator != SeparatorDisposition::Consumed
            && (keep || entry.receipt.leading_retreat.is_some())
        {
            self.separator = SeparatorDisposition::Keep;
        }
    }
}

/// Choose the native start cell (`content`) and `minbl`/padding carry for a
/// `TERMP_HANG`/`TERMP_NOBREAK` list word (`mdoc_term.c:790-835`,
/// `term.c:115-116, 236, 250-253, 573-581`).
fn hang_word_anchor(
    (pending_field_spaces, settled_boundary_glyph, zero_advance_joined, no_fill_word_active): (
        usize,
        bool,
        bool,
        bool,
    ),
    (boundary, native_word_start, separator): (PendingBoundary, usize, usize),
    (anchor_ir_start, materialized_field_padding, projected_keep, projection): (
        String,
        usize,
        bool,
        WordProjectionPolicy,
    ),
    entry: &ExecutedWordEntry,
) -> super::super::NativeWordAnchor {
    let receipt = &entry.receipt;
    let native_separator = entry.surviving_separator;
    let suppress_committed_no_fill =
        no_fill_word_active && boundary == PendingBoundary::CommittedField;
    let content = if native_separator > 0
        && pending_field_spaces == 0
        && !suppress_committed_no_fill
        && (!boundary.is_tight()
            || (settled_boundary_glyph && !receipt.prints_padding)
            || zero_advance_joined
            || receipt.leading_retreat.is_some()
            || projected_keep
            || entry.rewound()
            || entry.has_replayed_slots()
            || (separator == 2 && native_separator == 1))
    {
        native_word_start
    } else {
        receipt.first_content_cell
    };
    super::super::NativeWordAnchor {
        start: native_word_start,
        owner: anchor_ir_start,
        content,
        projected_device_padding: if receipt.prints_padding {
            pending_field_spaces
        } else {
            0
        }
        .max(materialized_field_padding),
        projected_field_prefix: false,
        projected_gap: ProjectedNativeGap::default(),
        projection,
    }
}

fn plain_unit_word_anchor(
    (boundary, native_boundary): (PendingBoundary, PendingBoundary),
    (zero_advance_joined, projected_keep): (bool, bool),
    (native_word_start, anchor_ir_start, projection): (usize, String, WordProjectionPolicy),
    entry: &ExecutedWordEntry,
) -> super::super::NativeWordAnchor {
    let receipt = &entry.receipt;
    let content = if entry.surviving_separator > 0
        && !native_boundary.is_tight()
        && (!boundary.is_tight()
            || zero_advance_joined
            || receipt.leading_retreat.is_some()
            || projected_keep
            || entry.rewound())
    {
        native_word_start
    } else {
        receipt.first_content_cell
    };
    super::super::NativeWordAnchor {
        start: native_word_start,
        owner: anchor_ir_start,
        content,
        projected_device_padding: 0,
        projected_field_prefix: false,
        projected_gap: ProjectedNativeGap::default(),
        projection,
    }
}

impl InlineBuilder {
    pub(super) fn record_hang_word(
        &mut self,
        incoming: &mut Vec<Inline>,
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
        let mut entry = ExecutedWordEntry::execute(
            &mut definition.field_buffer,
            native_writes.expect("formatter word native writes"),
            native_word_start,
            separator,
            &mut definition.field_word_anchors,
        );
        entry.project_rewind(
            &mut self.nodes,
            incoming,
            &anchor_ir_start,
            &definition.field_word_anchors,
            definition.deferred_head_projection.as_ref(),
        );
        let native_word_start = entry.word_start(native_word_start);
        let receipt = &entry.receipt;
        let native_separator = entry.surviving_separator;
        let authored_separator =
            take_retreated_source_blank(incoming, &anchor_ir_start, receipt.leading_retreat);
        let initial_anchor_index = definition.field_word_anchors.len();
        definition.field_word_anchors.push(hang_word_anchor(
            (
                self.execution.pending_field_spaces,
                self.execution.settled_boundary_glyph,
                self.execution.zero_advance_joined,
                self.execution.no_fill_word_active,
            ),
            (boundary, native_word_start, separator),
            (
                anchor_ir_start,
                materialized_field_padding,
                projected_keep,
                projection,
            ),
            &entry,
        ));
        entry.append_owner_intervals(&mut definition.field_word_anchors);
        observe_definition_word(definition, incoming, separator);
        let mut projection = self.record_authored_native_passes(
            incoming,
            receipt.first_content_cell,
            entry.relocated_cells(),
        );
        let input = self.execution.word_input();
        projection.record_separator(input.0, native_word_start, separator, native_separator);
        entry.record_projection(&mut projection);
        projection.record_separator_effects(
            input,
            native_word_start,
            &entry,
            projected_keep,
            false,
        );
        self.align_last_definition_separator(
            &projection,
            initial_anchor_index,
            receipt.first_content_cell,
        );
        projection.prints_padding = receipt.prints_padding;
        projection.authored_separator = authored_separator;
        projection
    }

    fn align_last_definition_separator(
        &mut self,
        projection: &WordPassProjection,
        initial_index: usize,
        content: usize,
    ) {
        if let Some(anchors) = self
            .execution
            .definition
            .as_mut()
            .map(|definition| &mut definition.field_word_anchors)
        {
            projection.align_separator_owners(anchors, initial_index, content);
        }
    }

    /// Plain-flow word recording: the paragraph's flush unit takes the same
    /// `term_word()` cell sequence (separator, writes, anchor) as a field.
    /// It has no hang-row geometry; only the marker pass loop can act.
    fn record_plain_unit_word(
        &mut self,
        incoming: &mut Vec<Inline>,
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
        let mut entry = ExecutedWordEntry::execute(
            &mut self.execution.plain_input.buffer,
            native_writes.unwrap_or_default(),
            native_word_start,
            separator,
            &mut self.execution.plain_input.anchors,
        );
        entry.project_rewind(
            &mut self.nodes,
            incoming,
            &anchor_ir_start,
            &self.execution.plain_input.anchors,
            None,
        );
        entry.transfer_positioning_slots(&mut self.execution.plain_input.buffer);
        let native_word_start = entry.word_start(native_word_start);
        let receipt = &entry.receipt;
        let native_separator = entry.surviving_separator;
        let authored_separator =
            take_retreated_source_blank(incoming, &anchor_ir_start, receipt.leading_retreat);
        let initial_anchor_index = self.execution.plain_input.anchors.len();
        self.execution
            .plain_input
            .anchors
            .push(plain_unit_word_anchor(
                (self.execution.boundary, native_boundary),
                (self.execution.zero_advance_joined, projected_keep),
                (native_word_start, anchor_ir_start, projection),
                &entry,
            ));
        entry.append_owner_intervals(&mut self.execution.plain_input.anchors);
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
            entry.relocated_cells(),
        );
        result.record_separator(&unit.buffer, native_word_start, separator, native_separator);
        entry.record_projection(&mut result);
        result.record_separator_effects(
            (&unit.buffer, &unit.anchors),
            native_word_start,
            &entry,
            projected_keep,
            true,
        );
        result.align_separator_owners(
            &mut unit.anchors,
            initial_anchor_index,
            receipt.first_content_cell,
        );
        if let Some(anchor) = unit.anchors.last()
            && let Some(handoff) = &mut self.section_handoff
        {
            handoff.note_heading_word(anchor.start.min(anchor.content));
        }
        result.prints_padding = receipt.prints_padding;
        result.authored_separator = authored_separator;
        result
    }
}
