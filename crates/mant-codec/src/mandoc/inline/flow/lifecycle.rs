//! Initialize and retire projection owners while retaining native formatter registers.
use super::tab_stops::TabStops;
use super::{
    AuthorBreakEffect, AuthorExecution, DetachedDeviceRow, FontState, FormatterColumn,
    InlineExecutionState, KeepState, LeadingLineBoundary, PendingBoundary, SourceLineObservation,
    SpacingMode, TrailingOutput, WordEndBreak, ZeroAdvanceState, definition, field_buffer,
};
use libmandoc_rs::MacroSet;
use std::sync::Arc;

impl InlineExecutionState {
    pub(in crate::mandoc) fn source_row_continues(&self) -> bool {
        self.final_source_continuation.unwrap_or(false)
    }

    pub(in crate::mandoc) fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            escape_coverage: crate::mandoc::escape_coverage::EscapeCoverage::default(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::from_enabled(spacing_enabled),
            last_visible_character: None,
            has_printable_content: false,
            visible_glyph_epoch: 0,
            formatter_column: FormatterColumn::Origin,
            empty_word: false,
            trailing_output: TrailingOutput::None,
            pending_breakable_spaces: 0,
            pending_field_spaces: 0,
            pending_field_gap_origin: definition::PendingFieldGapOrigin::Other,
            pending_line_indent: 0,
            word_end_break: WordEndBreak::Clear,
            word_end_break_separated: false,
            leading_line_boundary: LeadingLineBoundary::None,
            vertical_space_debt: 0,
            completed_vertical_rows: 0,
            keep: KeepState::new(),
            font: FontState::new(),
            macro_set: MacroSet::None,
            zero_advance: ZeroAdvanceState::new(),
            zero_advance_joined: false,
            final_word_join: None,
            final_source_continuation: None,
            execution_epoch: 0,
            author_execution: None,
            definition: None,
            detached_device_row: None,
            tab_stops: Arc::new(TabStops::default()),
            last_tab_source_node: None,
            last_executed_source_line: None,
            observe_no_fill_source_lines: SourceLineObservation::Disabled,
            no_fill_word_active: false,
            wipe_remainder: false,
            row_zero_graph: false,
            native_word_writes: None,
            native_word_boundary: None,
            native_word_owner: None,
            native_owner_serial: 0,
            flush_unit: field_buffer::FieldBuffer::default(),
            flush_unit_anchors: Vec::new(),
            flush_unit_output_start: 0,
            concat_next_word: false,
            concat_flush_source: false,
            concat_consumed_for_body: false,
            flush_consumed_for_body: false,
            scope_posts: crate::mandoc::containers::ScopePostState::default(),
            column_output_depth: 0,
        }
    }

    pub(super) fn reset_paragraph_segment(&mut self, armed_zero_advance: bool) {
        if self.has_column_output_scope() {
            // term_flushln() already settled the native buffer at the real
            // pre/post. Retiring its IR owner must retain NOSPACE/NONEWLINE,
            // the occupied device row and the next field's native minbl.
            self.retire_output_owner();
            return;
        }
        self.boundary = PendingBoundary::Ordinary;
        self.last_visible_character = None;
        self.has_printable_content = false;
        self.formatter_column = FormatterColumn::Origin;
        self.empty_word = false;
        self.trailing_output = TrailingOutput::None;
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
        self.pending_line_indent = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.wipe_remainder = false;
        self.row_zero_graph = false;
        self.leading_line_boundary = LeadingLineBoundary::None;
        self.zero_advance.reset_projection(armed_zero_advance);
        self.zero_advance_joined = false;
        self.final_word_join = None;
        // TERMP_NONEWLINE is a native execution register, not an IR segment
        // property. A paragraph drain does not consume a preceding \c.
        self.execution_epoch = 0;
        self.retire_definition_output_owner();
        self.last_executed_source_line = None;
        self.completed_vertical_rows = 0;
        if let Some(author) = &mut self.author_execution {
            // The author mode is a formatter register; this index belongs to
            // the drained IR segment and cannot cross its output boundary.
            author.field_output_start = 0;
        }
    }

    /// Retire only indices into a drained IR owner. The native word, row,
    /// and separator registers continue across an mdoc HEAD/BODY split.
    pub(super) fn retire_output_owner(&mut self) {
        // These observations were already projected into the detached HEAD.
        // In particular, an An -split term_newln() cannot become a second
        // leading break before the BODY's first word.
        self.leading_line_boundary = LeadingLineBoundary::None;
        self.completed_vertical_rows = 0;
        self.pending_line_indent = 0;
        self.zero_advance_joined = false;
        self.final_word_join = None;
        self.execution_epoch = 0;
        self.retire_definition_output_owner();
        self.last_executed_source_line = None;
        if let Some(author) = &mut self.author_execution {
            author.field_output_start = 0;
        }
    }

    pub(in crate::mandoc) fn enter_column_output_scope(&mut self) {
        self.column_output_depth = self.column_output_depth.saturating_add(1);
    }

    pub(in crate::mandoc) fn exit_column_output_scope(&mut self) {
        self.column_output_depth = self.column_output_depth.saturating_sub(1);
        self.retire_definition_output_owner();
    }

    pub(in crate::mandoc) fn has_column_output_scope(&self) -> bool {
        self.column_output_depth > 0
    }

    pub(super) fn retire_definition_output_owner(&mut self) {
        if self.has_column_output_scope() {
            // print_mdoc_node() restores node geometry after post, but never
            // clears viscol/minbl or the surrounding column flags because an
            // IR destination returned (mdoc_term.c:409-439, term.c:233-253).
            if let Some(field) = &mut self.definition {
                field.field_word_anchors.clear();
                field.field_buffer.detach_projection_owner();
                field.row.retire_row_origin();
            }
        } else if let Some(field) = self.definition.take() {
            self.detached_device_row =
                field
                    .hang_row
                    .native_row_occupied()
                    .then_some(DetachedDeviceRow {
                        viscol: field.hang_row.viscol,
                        minbl: field.hang_row.minbl,
                        page_origin_printed: field.hang_row.page_origin_printed,
                    });
        }
    }

    pub(in crate::mandoc) fn spacing_enabled(&self) -> bool {
        self.spacing == SpacingMode::Enabled
    }

    pub(in crate::mandoc) fn set_spacing_enabled(&mut self, enabled: bool) {
        self.spacing = SpacingMode::from_enabled(enabled);
    }

    pub(in crate::mandoc) fn has_formatter_cell(&self) -> bool {
        self.formatter_column == FormatterColumn::Advanced
            || self.zero_advance.has_buffered_glyph()
            || self.word_end_break == WordEndBreak::Pending
    }

    pub(in crate::mandoc) fn enter_man_definition_body(&mut self) {
        self.boundary = PendingBoundary::Tight;
        self.final_source_continuation = Some(true);
        self.final_word_join = Some(false);
    }

    pub(in crate::mandoc) fn visible_content_checkpoint(&self) -> (u64, bool) {
        (
            self.visible_glyph_epoch,
            self.zero_advance.has_printable_pending_glyph(),
        )
    }

    pub(in crate::mandoc) fn has_visible_content_since(&self, before: (u64, bool)) -> bool {
        self.visible_glyph_epoch != before.0
            || (!before.1
                && self.zero_advance.has_printable_pending_glyph()
                && !self.current_native_word_is_rejected())
    }

    pub(super) fn current_native_word_is_rejected(&self) -> bool {
        let (buffer, anchors) = self
            .definition
            .as_ref()
            .map_or((&self.flush_unit, &self.flush_unit_anchors), |field| {
                (&field.field_buffer, &field.field_word_anchors)
            });
        buffer.pending_pass_is_definitively_rejected()
            && anchors
                .last()
                .is_some_and(|anchor| anchor.content >= buffer.resume_offset())
    }

    pub(in crate::mandoc) fn has_printable_pending_zero_advance_glyph(&self) -> bool {
        self.zero_advance.has_printable_pending_glyph()
    }

    pub(in crate::mandoc) fn discard_zero_advance_at_row_end(&mut self) {
        self.zero_advance.discard_at_row_end();
    }

    pub(in crate::mandoc) fn take_leading_line_boundary(&mut self) -> Option<u64> {
        match std::mem::replace(&mut self.leading_line_boundary, LeadingLineBoundary::None) {
            LeadingLineBoundary::None => None,
            LeadingLineBoundary::AtVisibleCheckpoint(checkpoint) => Some(checkpoint),
        }
    }

    pub(in crate::mandoc) fn take_zero_advance_armed(&mut self) -> bool {
        self.zero_advance.take_armed()
    }

    pub(in crate::mandoc) fn inherit_zero_advance_armed(&mut self, armed: bool) {
        self.zero_advance.inherit_armed(armed);
    }

    pub(in crate::mandoc) fn author_flow(&self) -> Option<crate::mandoc::formatter::AuthorFlow> {
        self.author_execution.map(|execution| execution.flow)
    }

    pub(in crate::mandoc) fn set_author_flow(
        &mut self,
        flow: crate::mandoc::formatter::AuthorFlow,
    ) {
        if let Some(execution) = &mut self.author_execution {
            execution.flow = flow;
        } else {
            self.author_execution = Some(AuthorExecution {
                flow,
                authors_section: false,
                break_effect: AuthorBreakEffect::Line,
                field_output_start: 0,
            });
        }
    }
}

impl Default for InlineExecutionState {
    fn default() -> Self {
        Self::with_spacing(true)
    }
}
