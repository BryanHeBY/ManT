use super::{
    AuthorBreakEffect, AuthorExecution, FormatterColumn, Inline, InlineBuilder,
    InlineExecutionState, KeepPhase, PendingBoundary, PreservedInlineState, SourceFragmentState,
    SourceLineObservation, SpacingMode, TrailingOutput, WordEndBreak, last_visible_character,
    trim_trailing_breakable_spaces, updated_spacing,
};

impl InlineBuilder {
    pub(in crate::mandoc) fn tighten_next_boundary(&mut self) {
        self.execution.boundary = PendingBoundary::Tight;
    }

    pub(in crate::mandoc) fn inherit_author_execution(
        &mut self,
        flow: crate::mandoc::formatter::AuthorFlow,
        authors_section: bool,
    ) {
        self.inherit_author_execution_with_effect(flow, authors_section, AuthorBreakEffect::Line);
    }

    pub(in crate::mandoc) fn inherit_author_execution_with_effect(
        &mut self,
        flow: crate::mandoc::formatter::AuthorFlow,
        authors_section: bool,
        break_effect: AuthorBreakEffect,
    ) {
        if matches!(break_effect, AuthorBreakEffect::Field { .. }) {
            self.definition_state_mut();
        }
        self.execution.author_execution = Some(AuthorExecution {
            flow,
            authors_section,
            break_effect,
            field_output_start: self.nodes.len(),
        });
    }

    pub(in crate::mandoc) fn execute_author(&mut self, mode: Option<libmandoc_rs::AuthorMode>) {
        if mode.is_some() && !self.has_formatter_cell() && self.execution.pending_field_spaces > 0 {
            // A mode-only `.An -split/-nosplit` executes no formatter word,
            // but the later author pre-handler calls `term_newln()` before
            // its word.  With no buffered cell that call retains NOSPACE and
            // discards an unrealized HANG field separator.
            self.execution.pending_field_spaces = 0;
            self.execution.boundary = PendingBoundary::Tight;
        }
        if !self.has_formatter_cell()
            && (self.pending_definition_indent().is_some()
                || self.execution.pending_line_indent > 0)
        {
            if let Some(state) = &mut self.execution.definition {
                state.pending_indent = None;
                state.outcome.clear_body_gap_consumed();
            }
            self.execution.pending_line_indent = 0;
            // The native BRIND offset belongs to the immediately following
            // child scope, and `.ti` is likewise scoped to the formatter row
            // active while its request executes.  A control-only `.An`
            // transition can leave either scope without printing a word, so
            // a later author must not inherit abandoned geometry or its
            // consumed body gap.
        }
        let Some((break_effect, field_output_start)) = self
            .execution
            .author_execution
            .as_mut()
            .and_then(|execution| {
                execution
                    .flow
                    .execute(mode, execution.authors_section)
                    .then_some((execution.break_effect, execution.field_output_start))
            })
        else {
            return;
        };
        match break_effect {
            AuthorBreakEffect::Line => self.hard_break(),
            AuthorBreakEffect::Field {
                gap_cells,
                body_width_columns,
                wraps,
            } => {
                self.flush_definition_field(
                    field_output_start,
                    gap_cells,
                    body_width_columns,
                    wraps,
                    false,
                );
            }
        }
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
        }
    }

    pub(in crate::mandoc) fn request_word_end_break(&mut self) {
        // A NOBREAK field separator is ordered before the next formatter
        // word, but it is still trailing field geometry until that word
        // emits a cell.  CVS `term_field()` therefore drops it when a
        // control-only `\p` ends the field.  Keep it for empty words and for
        // a pending `\zX` glyph: both can still make the field observable.
        if self.execution.empty_word
            && !self.execution.zero_advance.has_pending_glyph()
            && let TrailingOutput::FieldBlank(count) = self.execution.trailing_output
        {
            trim_trailing_breakable_spaces(&mut self.nodes, count);
            self.execution.last_visible_character = last_visible_character(&self.nodes);
            self.execution.trailing_output = if self
                .execution
                .last_visible_character
                .is_some_and(char::is_whitespace)
            {
                TrailingOutput::BoundaryBlank
            } else if self.execution.last_visible_character.is_some() {
                TrailingOutput::NonBlank
            } else {
                TrailingOutput::None
            };
        }
        self.execution.word_end_break = WordEndBreak::Pending;
    }

    pub(in crate::mandoc) fn take_word_end_break(&mut self) -> bool {
        std::mem::replace(&mut self.execution.word_end_break, WordEndBreak::Clear)
            == WordEndBreak::Pending
    }

    pub(in crate::mandoc) fn note_zero_advance_join(&mut self) {
        self.execution.zero_advance_joined = true;
    }

    pub(in crate::mandoc) fn begin_executed_node(&mut self, node: &libmandoc_rs::Node) {
        if self.execution.observe_no_fill_source_lines == SourceLineObservation::NoFill
            && node.flags.no_fill
            && node.flags.line_start
            && !self.final_source_continuation_or(false)
        {
            // mdoc_term.c::print_mdoc_node() performs NODE_NOFILL/NODE_LINE
            // before macro pre or generated punctuation, even in It HEAD.
            // Distinct expanded rows can have the same source coordinate.
            // A repeated event on an empty formatter cell is a no-op.
            self.no_fill_source_line();
        }
        if node.line != 0 {
            self.execution.last_executed_source_line = Some(node.line);
        }
        // mdoc_term.c keys KEEP lifetime from each executed NODE_LINE event,
        // not from the numeric source coordinate. User-macro expansion can
        // execute several input rows that all retain the call site's line.
        if self.execution.keep.keeping() && node.flags.line_start {
            self.execution.keep.phase = KeepPhase::PreKeep;
        }
    }

    pub(in crate::mandoc) fn observe_no_fill_source_lines(&mut self, enabled: bool) {
        self.execution.observe_no_fill_source_lines = if enabled {
            SourceLineObservation::NoFill
        } else {
            SourceLineObservation::Disabled
        };
    }

    pub(in crate::mandoc) fn final_word_join_or(&self, fallback: bool) -> bool {
        self.execution.final_word_join.unwrap_or(fallback)
    }

    /// Start one source fragment with an empty result slot while retaining the
    /// prior formatter decision for transparent fragments such as `.Tg`.
    /// A real word, break, or release writes its own final state; an anchor or
    /// font-only request leaves the slot empty and therefore cannot consume a
    /// still-live physical continuation.
    pub(in crate::mandoc) fn begin_source_fragment(&mut self) -> SourceFragmentState {
        SourceFragmentState {
            final_word_join: self.execution.final_word_join.take(),
            final_source_continuation: self.execution.final_source_continuation.take(),
            execution_epoch: self.execution.execution_epoch,
        }
    }

    pub(in crate::mandoc) fn finish_source_fragment(&mut self, state: SourceFragmentState) {
        if self.execution.execution_epoch == state.execution_epoch
            && self.execution.final_word_join.is_none()
        {
            self.execution.final_word_join = state.final_word_join;
        }
        if self.execution.execution_epoch == state.execution_epoch
            && self.execution.final_source_continuation.is_none()
        {
            self.execution.final_source_continuation = state.final_source_continuation;
        }
    }

    pub(in crate::mandoc) fn continue_source_line(&mut self, continued: bool) {
        self.execution.final_word_join = Some(continued);
        self.execution.final_source_continuation = Some(continued);
    }

    pub(in crate::mandoc) fn final_word_join_state(&self) -> Option<bool> {
        self.execution.final_word_join
    }

    pub(in crate::mandoc) fn final_source_continuation_or(&self, fallback: bool) -> bool {
        self.execution.final_source_continuation.unwrap_or(fallback)
    }

    /// Adopt the already-executed tail result from a private formatter scope.
    /// The scope may have consumed a child `\\c` with generated punctuation;
    /// looking at the outer AST flag after that would reverse the execution
    /// order.
    pub(in crate::mandoc) fn inherit_final_word_join(&mut self, result: Option<bool>) {
        self.execution.final_word_join = result;
    }

    pub(in crate::mandoc) fn release_next_boundary(&mut self) {
        self.execution.boundary = PendingBoundary::Ordinary;
        self.execution.final_word_join = Some(false);
    }

    pub(in crate::mandoc) fn enter_keep_words(&mut self) {
        self.execution.keep.phase = KeepPhase::PreKeep;
    }

    pub(in crate::mandoc) fn exit_keep_words(&mut self) {
        // CVS stores PREKEEP/KEEP as formatter-global flags rather than a
        // nesting depth. Consequently an inner Ek clears an outer Bk too.
        self.execution.keep.phase = KeepPhase::Inactive;
    }

    /// Extract a buffered `\p` that has no word or glyph to own it. A real
    /// formatter flush emits this as one empty row. Keeping it as a pending
    /// inline event would either discard it or move it past a structural
    /// container.
    pub(in crate::mandoc) fn take_unrepresented_word_end_break(&mut self) -> bool {
        let only_transparent_nodes = self
            .nodes
            .iter()
            .all(|node| matches!(node, Inline::Anchor { .. }));
        if only_transparent_nodes
            && !self.execution.zero_advance.has_buffered_glyph()
            && self.execution.word_end_break == WordEndBreak::Pending
        {
            self.execution.word_end_break = WordEndBreak::Clear;
            true
        } else {
            false
        }
    }

    /// Apply one signed `.sp`/`.Pp` request to CVS `skipvsp` state and return
    /// the rows that remain visible in the renderer-neutral IR.
    pub(in crate::mandoc) fn resolve_vertical_space(&mut self, rows: i32) -> u16 {
        self.execution.resolve_vertical_space(rows)
    }

    pub(in crate::mandoc) fn inherit_vertical_space_debt(&mut self, debt: u16) {
        self.execution.vertical_space_debt = debt;
    }

    pub(in crate::mandoc) const fn vertical_space_debt(&self) -> u16 {
        self.execution.vertical_space_debt
    }

    pub(in crate::mandoc) fn inherit_zero_advance_armed(&mut self, armed: bool) {
        self.execution.zero_advance.inherit_armed(armed);
    }

    /// Continue a formatter stream across an IR-only ownership split.
    pub(in crate::mandoc) fn inherit_preserved_execution(&mut self, state: PreservedInlineState) {
        self.execution.last_executed_source_line = state.last_executed_source_line;
        self.execution.zero_advance = state.zero_advance;
        self.execution.word_end_break = if state.word_end_break {
            WordEndBreak::Pending
        } else {
            WordEndBreak::Clear
        };
        self.execution.final_source_continuation = state.source_continuation;
        if state.formatter_cell_occupied {
            self.execution.formatter_column = FormatterColumn::Advanced;
        }
        self.execution.pending_line_indent = state.pending_line_indent;
        if state.pending_definition_indent.is_some() {
            self.set_pending_definition_indent(state.pending_definition_indent);
        }
    }

    /// Execute the generated no-break cells between an mdoc inset/diagnostic
    /// head and body.  The caller represents surviving cells as body-leading
    /// inline space, so no independent layout gap is added later.
    pub(in crate::mandoc) fn append_run_in_cells(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.tighten_next_boundary();
        self.begin_word_projection(true);
        let mut projected = Vec::new();
        self.execution.zero_advance.append_generated_cells(
            count,
            &mut projected,
            self.execution.font.display_current(),
        );
        self.append_word(projected);
        self.execution.trailing_output = TrailingOutput::FixedBlank;
        // CVS sets TERMP_NOSPACE again before executing BODY children.
        self.tighten_next_boundary();
        self.execution.final_word_join = Some(false);
        self.execution.final_source_continuation = Some(false);
    }

    pub(in crate::mandoc) fn take_zero_advance_armed(&mut self) -> bool {
        self.execution.zero_advance.take_armed()
    }

    /// Source wrapping is a word boundary even while macro auto-spacing is
    /// disabled. Explicit joins still take precedence over ordinary wrapping.
    pub(in crate::mandoc) fn preserve_source_word_boundary(&mut self) {
        if !self.execution.boundary.is_tight() {
            self.execution.boundary = PendingBoundary::Preserved;
        }
    }

    /// Preserve the formatter word boundary released inside a physically
    /// continued source line.  The incoming word decides whether its own
    /// leading blank adds a second space; this state must survive a new
    /// source-fragment reset.
    pub(in crate::mandoc) fn preserve_continued_boundary(&mut self) {
        if !self.execution.boundary.is_tight() {
            self.execution.boundary = PendingBoundary::Continued;
        }
    }

    /// Join a generated prefix only to its own operand scope. Word events
    /// consume it even without glyphs; anchors do not. Expire unused joins.
    /// An explicit control replaces `PrefixJoin` with `Tight` and must survive.
    pub(in crate::mandoc) fn with_prefix_join(&mut self, append: impl FnOnce(&mut Self)) {
        self.execution.boundary = PendingBoundary::PrefixJoin;
        append(self);
        if self.execution.boundary == PendingBoundary::PrefixJoin {
            self.execution.boundary = PendingBoundary::Ordinary;
        }
    }

    pub(in crate::mandoc) const fn has_tight_boundary(&self) -> bool {
        self.execution.boundary.is_tight()
    }

    pub(in crate::mandoc) const fn spacing_enabled(&self) -> bool {
        self.execution.spacing.enabled()
    }

    pub(in crate::mandoc) fn set_spacing(&mut self, setting: &str) {
        let updated = updated_spacing(self.execution.spacing.enabled(), setting);
        if updated == self.execution.spacing.enabled() {
            return;
        }
        // `.Sm off` changes spacing *after* the request. If printable
        // content precedes the transition, retain its ordinary boundary to
        // the first following fragment, then concatenate subsequent macro
        // arguments until spacing is enabled again.
        self.execution.boundary = match (
            updated,
            !self.execution.has_printable_content,
            self.execution.boundary,
        ) {
            (_, _, boundary) if boundary.is_tight() => boundary,
            (false, false, _) => PendingBoundary::Preserved,
            _ => PendingBoundary::Ordinary,
        };
        self.execution.spacing = SpacingMode::from(updated);
    }
}

impl InlineExecutionState {
    pub(in crate::mandoc) fn resolve_vertical_space(&mut self, rows: i32) -> u16 {
        if rows < 0 {
            let debt = u16::try_from(rows.unsigned_abs()).unwrap_or(u16::MAX);
            self.vertical_space_debt = self.vertical_space_debt.saturating_add(debt);
            return 0;
        }
        let rows = u16::try_from(rows).unwrap_or(u16::MAX);
        let consumed = rows.min(self.vertical_space_debt);
        self.vertical_space_debt -= consumed;
        rows - consumed
    }
}
