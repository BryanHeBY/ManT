use super::definition::PendingFieldGapOrigin;
use super::{
    AuthorBreakEffect, AuthorExecution, FormatterColumn, Inline, InlineBuilder,
    InlineExecutionState, KeepPhase, NativeWordSpacing, PendingBoundary, PreservedInlineState,
    SourceFragmentState, SourceLineObservation, SpacingMode, TrailingOutput, WordEndBreak,
    last_visible_character, trim_trailing_breakable_spaces, updated_spacing,
};
use libmandoc_rs::{
    MacroSet,
    MacroToken::{Man, Roff},
    ManMacro, RoffMacro,
};

impl InlineBuilder {
    /// A pre-handler's direct `term_word` operands do not run the native node
    /// exit. `pre_alternate` explicitly forwards its child EOS separately.
    pub(in crate::mandoc) fn finish_executed_node(&mut self, node: &libmandoc_rs::Node) {
        if self.visits_native_node() {
            self.execution.finish_executed_node(node);
        }
    }

    /// `roff_term_pre_ce()` directly visits words and roff requests. Source
    /// coordinates remain provenance; its child `NODE_LINE` flags are not
    /// additional `print_man_node/print_mdoc_node` entry events.
    pub(in crate::mandoc) fn with_grouped_line_dispatch(&mut self, append: impl FnOnce(&mut Self)) {
        let previous = std::mem::replace(
            &mut self.execution.observe_no_fill_source_lines,
            SourceLineObservation::Grouped,
        );
        append(self);
        self.execution.observe_no_fill_source_lines = previous;
    }
    pub(in crate::mandoc) fn tighten_next_boundary(&mut self) {
        self.execution.native_word_spacing = NativeWordSpacing::NoSpace;
        self.join_projected_boundary();
    }

    /// A decoded overstrike joins its projection after the incoming native
    /// word boundary already executed. It cannot select NOSPACE a second time.
    pub(in crate::mandoc) fn join_projected_boundary(&mut self) {
        self.execution.boundary = PendingBoundary::Tight;
    }

    pub(in crate::mandoc) fn inherit_author_execution(
        &mut self,
        flow: crate::mandoc::formatter::AuthorFlow,
        authors_section: bool,
    ) {
        // Changing an IR output owner refreshes section/author context, not
        // the surrounding field's live NOBREAK/trailspace device flags.
        let effect = if self.execution.has_persistent_native_field_scope() {
            self.execution
                .author_execution
                .map_or(AuthorBreakEffect::Line, |author| author.break_effect)
        } else {
            AuthorBreakEffect::Line
        };
        self.inherit_author_execution_with_effect(flow, authors_section, effect);
    }

    pub(in crate::mandoc) fn inherit_author_execution_with_effect(
        &mut self,
        flow: crate::mandoc::formatter::AuthorFlow,
        authors_section: bool,
        break_effect: AuthorBreakEffect,
    ) {
        let inherited_start = self
            .execution
            .author_execution
            .is_none()
            .then(|| {
                let live = self
                    .execution
                    .definition
                    .as_ref()
                    .is_some_and(|state| !state.field_word_anchors.is_empty())
                    || !self.execution.plain_input.anchors.is_empty();
                live.then_some(self.execution.plain_input.output_start)
            })
            .flatten();
        if matches!(break_effect, AuthorBreakEffect::Field { .. }) {
            self.definition_state_mut();
        }
        self.execution.author_execution = Some(AuthorExecution {
            flow,
            authors_section,
            break_effect,
            field_output_start: inherited_start.unwrap_or(self.nodes.len()),
        });
    }

    /// It HEAD pre changes field flags and NOSPACE after any direct BLOCK
    /// prefix has executed. Those pending cells keep their original owners
    /// and buffer start: installing HEAD geometry does not flush or drain.
    pub(in crate::mandoc) fn enter_definition_head_effect(&mut self, effect: AuthorBreakEffect) {
        if matches!(effect, AuthorBreakEffect::Field { .. }) {
            self.definition_state_mut();
        }
        if let Some(state) = &mut self.execution.definition {
            // A nested original It HEAD installs its own pre flags on the
            // same native cells/device row. An enclosing HEAD's completed
            // post is not evidence that this new HEAD's flags were cleared.
            state.no_break = None;
            state.no_break_cleared = false;
            state.head_flags_cleared = false;
            state.cleared_field_capacity_columns = 0;
        }
        if let Some(author) = &mut self.execution.author_execution {
            author.break_effect = effect;
        }
        self.tighten_next_boundary();
    }

    pub(in crate::mandoc) fn execute_author(&mut self, mode: Option<libmandoc_rs::AuthorMode>) {
        if mode.is_some()
            && !self.has_formatter_cell()
            && self.execution.pending_field_spaces > 0
            && self.execution.pending_field_gap_origin != PendingFieldGapOrigin::CommittedFlush
        {
            // A mode-only An emits no word. A preceding term_newln()
            // (NODE_LINE or a macro post) committed native trailspace for
            // the next word; an unrealized author separator is abandoned.
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
        let Some(break_effect) = self
            .execution
            .author_execution
            .as_mut()
            .and_then(|execution| {
                execution
                    .flow
                    .execute(mode, execution.authors_section)
                    .then_some(execution.break_effect)
            })
        else {
            return;
        };
        match break_effect {
            // termp_an_pre() calls term_newln(), including the current
            // field's accepted device positions before node geometry returns.
            AuthorBreakEffect::Line => self.execute_native_newline(),
            AuthorBreakEffect::Field { flags, .. } => {
                if flags.contains(super::native_field::FieldFlag::Hang)
                    && let Some(definition) = &mut self.execution.definition
                    && definition.vertical_started_row
                {
                    // term_vspace() ended the device row. The body margin
                    // is offset geometry, not printed viscol: termp_an_pre()
                    // only calls conditional term_newln() (mdoc_term.c:1085;
                    // term.c:475-480). Fabricating occupancy here would flush
                    // and clear a bare BACKAFTER that wrote no native cell.
                    definition.vertical_started_row = false;
                }
                // termp_an_pre() invokes the same term_newln() as a source
                // event. Its captured receipt already decides the tail;
                // projected text width cannot end this row a second time.
                self.execute_native_newline_with_tail();
                if !flags.wraps()
                    && let Some(definition) = &mut self.execution.definition
                {
                    // termp_an_pre() called term_newln(): its NOSPACE flag
                    // suppresses term_word()'s automatic blank before the
                    // next author glyph. minbl from the flushed field stays.
                    definition.hang_row.suppress_next_auto_space = true;
                }
            }
        }
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
        }
    }

    pub(in crate::mandoc) fn request_word_end_break(&mut self, separated_from_graph: bool) {
        self.execution.word_end_break_separated = separated_from_graph;
        if let Some(definition) = &mut self.execution.definition {
            definition.hang_row.field_pending_word_end_break = true;
        }
        // CVS ESCAPE_BREAK buffers a newline cell even when the decoded word
        // has no glyph. This cell belongs to the current source fragment,
        // unlike an occupied formatter row inherited from a detached HEAD.
        self.note_produced_formatter_cell(true);
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
        if self.execution.observe_no_fill_source_lines == SourceLineObservation::Grouped {
            self.execution.execute_tab_configuration(node);
            return;
        }
        let line_entered = self.execution.take_no_fill_source_line_entered(node.id);
        self.execution.enter_tab_source_node(node);
        if self.execution.observe_no_fill_source_lines == SourceLineObservation::NoFill
            && !line_entered
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
        // mdoc_term.c:314-321 applies the incoming BRNEVER only after
        // NODE_LINE consumes the preceding buffer under its old flags.
        // BRNEVER widens term_fill's target, not term_flushln's final
        // NOBREAK tail comparison (term.c:143-144,250-253).
        self.execution.no_fill_word_active = node.flags.no_fill;
        // NODE_LINE settles the previous field under its old settings;
        // roff_term_pre_ta() then changes stops for the subsequent field.
        self.execution.execute_tab_configuration(node);
        // mdoc_term.c keys KEEP lifetime from each executed NODE_LINE event,
        // not from the numeric source coordinate. User-macro expansion can
        // execute several input rows that all retain the call site's line.
        if self.execution.keep.keeping() && node.flags.line_start {
            self.execution.keep.phase = KeepPhase::PreKeep;
        }
    }

    /// Apply the TEXT-specific branches of `print_man_node` and `print_mdoc_node`
    /// after node entry and any ancestor pre-handler's generated words.
    /// Return true when the visitor consumes the empty node itself.
    pub(in crate::mandoc) fn execute_text_source_boundary(
        &mut self,
        node: &libmandoc_rs::Node,
        native: &str,
    ) -> bool {
        if !self.visits_native_node() || self.execution.macro_set == MacroSet::None {
            return false;
        }
        if native.is_empty() && (self.execution.macro_set == MacroSet::Man || node.flags.line_start)
        {
            self.execute_visited_empty_text(node.flags.no_fill);
            return true;
        }
        // tbl_term.c fills cell operands directly. NODE_LINE from a synthetic
        // recovery parse does not give a tbl cell another source-line event.
        if node.flags.line_start
            && native.starts_with(' ')
            && self.table_buffer_written.is_none()
            && !self.final_source_continuation_or(false)
        {
            self.execute_native_newline();
        }
        false
    }

    pub(in crate::mandoc) fn observe_no_fill_source_lines(&mut self, enabled: bool) {
        use SourceLineObservation::{Disabled, DisabledEntered, NoFill, NoFillEntered};
        self.execution.observe_no_fill_source_lines =
            match (enabled, self.execution.observe_no_fill_source_lines) {
                (true, DisabledEntered(id) | NoFillEntered(id)) => NoFillEntered(id),
                (false, DisabledEntered(id) | NoFillEntered(id)) => DisabledEntered(id),
                (true, _) => NoFill,
                (false, _) => Disabled,
            };
    }

    pub(in crate::mandoc) fn final_word_join_or(&self, fallback: bool) -> bool {
        self.execution.final_word_join.unwrap_or(fallback)
    }

    /// Start one source fragment with an empty word-join result slot. Source
    /// continuation is also a live native formatter register: the fragment's
    /// empty TEXT visitor must still see `TERMP_NONEWLINE` from its predecessor.
    /// A real word replaces it; a transparent fragment leaves it live.
    pub(in crate::mandoc) fn begin_source_fragment(&mut self) -> SourceFragmentState {
        SourceFragmentState {
            final_word_join: self.execution.final_word_join.take(),
            final_source_continuation: self.execution.final_source_continuation,
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
        // Eo/En post may clear NOSPACE without writing a word, including
        // under NONOSPACE (mdoc_term.c:1683,1734-1746). This is a real native
        // register transition, unlike retiring the current IR owner.
        self.execution.native_word_spacing = NativeWordSpacing::Separate;
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
        if let Some(field) = state.definition_field {
            // The item's NOBREAK field is still open (term.c keeps it from
            // termp_it_pre() until the BODY post term_newln()); continue it
            // with its original configuration.
            let mut state = field.state;
            state.run_in_continuation = true;
            self.execution.definition = Some(state);
            if let Some(author) = &mut self.execution.author_execution {
                author.break_effect = field.author_effect;
                author.field_output_start = 0;
            }
        }
    }

    /// Execute the generated no-break word between an mdoc inset/diagnostic
    /// head and body.  Its `term_word()` transition still runs when the only
    /// fixed cell was consumed by a completed HEAD glyph.
    pub(in crate::mandoc) fn append_run_in_cells(
        &mut self,
        count: usize,
        native_count: usize,
        generated_word: bool,
    ) {
        if !generated_word {
            return;
        }
        self.execution.native_word_writes = Some(
            (0..native_count)
                .map(|index| super::field_buffer::FieldWrite::OwnedBlank {
                    projected: index >= native_count.saturating_sub(count),
                })
                .collect(),
        );
        self.execution.native_word_boundary = Some(PendingBoundary::Tight);
        self.note_produced_formatter_cell(native_count > 0);
        self.tighten_next_boundary();
        self.begin_word_projection(true);
        self.begin_native_word_owner();
        if count > 0 {
            let mut projected = Vec::new();
            self.execution.zero_advance.append_generated_cells(
                count,
                &mut projected,
                self.execution.font.display_current(),
            );
            self.append_word(projected);
            self.execution.trailing_output = TrailingOutput::FixedBlank;
        } else {
            self.append_word(Vec::new());
        }
        // CVS sets TERMP_NOSPACE again before executing BODY children.
        self.tighten_next_boundary();
        self.execution.final_word_join = Some(false);
        self.execution.final_source_continuation = Some(false);
    }

    pub(in crate::mandoc) fn take_zero_advance_armed(&mut self) -> bool {
        self.execution.zero_advance.take_armed()
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
        let previous = self.execution.native_word_spacing;
        self.execution.native_word_spacing = NativeWordSpacing::NoSpace;
        self.execution.boundary = PendingBoundary::PrefixJoin;
        append(self);
        if self.execution.boundary == PendingBoundary::PrefixJoin {
            self.execution.boundary = PendingBoundary::Ordinary;
            self.execution.native_word_spacing = previous;
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
        let has_input_cells = !self.execution.word_input().0.is_empty();
        // CVS mdoc_term.c::termp_sm_pre() clears NOSPACE whenever spacing
        // ends up enabled and the live input buffer is occupied (p->col),
        // even for a repeated `.Sm on`. An empty term_word() releases a
        // word boundary without writing a cell; NBRZW and a pending \z
        // glyph do write cells. Read the native ledger, not projected IR
        // or the output device's column. Neither NONEWLINE nor KEEP changes.
        if updated && has_input_cells {
            self.execution.native_word_spacing = NativeWordSpacing::Separate;
            if !matches!(
                self.execution.boundary,
                PendingBoundary::CommittedField | PendingBoundary::Kept
            ) {
                self.execution.boundary = PendingBoundary::Ordinary;
            }
        }
        self.execution.spacing = SpacingMode::from(updated);
    }
}

impl super::InlineExecutionState {
    /// Select the global `TERMP_NOSPACE` flag at a real native entry. This
    /// does not drain an IR owner or consume the current input buffer.
    pub(in crate::mandoc) fn select_native_no_space(&mut self) {
        self.native_word_spacing = NativeWordSpacing::NoSpace;
    }

    /// The recorder and all word-time register readers must use the same
    /// live input. A standalone recovered Nm can retain field geometry
    /// after restoring an authorless session; that geometry does not route
    /// later words away from `PlainInput` (`term_word` writes one live tcol).
    pub(super) fn uses_definition_word_buffer(&self) -> bool {
        self.definition.is_some() && self.author_execution.is_some()
    }

    pub(super) fn word_input(
        &self,
    ) -> (
        &super::field_buffer::FieldBuffer,
        &[super::NativeWordAnchor],
    ) {
        if self.uses_definition_word_buffer() {
            let field = self.definition.as_ref().expect("authored word field");
            (&field.field_buffer, &field.field_word_anchors)
        } else {
            (&self.plain_input.buffer, &self.plain_input.anchors)
        }
    }

    pub(in crate::mandoc) fn enter_tab_source_node(&mut self, node: &libmandoc_rs::Node) {
        // A source node routed from block to inline execution is still one
        // entry event. Use stable AST identity, never a source coordinate.
        if node.id != 0 && self.last_tab_source_node == Some(node.id) {
            return;
        }
        self.last_tab_source_node = (node.id != 0).then_some(node.id);
        if !node.flags.no_fill && node.flags.line_start {
            // Both normal node drivers insert ASCII_TABREF on filled
            // NODE_LINE, before any handler including state-only requests
            // (mdoc_term.c:321; man_term.c:929; term.c:873-878).
            if let Some(definition) = &mut self.definition {
                definition.field_buffer.note_tab_reference();
            } else {
                self.plain_input.buffer.note_tab_reference();
            }
        }
    }

    pub(in crate::mandoc) fn execute_tab_configuration(&mut self, node: &libmandoc_rs::Node) {
        if node.macro_token.as_ref() == Some(&Roff(RoffMacro::Ll)) && !node.flags.no_print {
            self.set_table_line_length(
                node.children
                    .first()
                    .and_then(libmandoc_rs::Node::decoder_text),
            );
        }
        if node.macro_token.as_ref() == Some(&Man(ManMacro::Dt)) && !node.flags.no_print {
            self.reset_default_tabs();
        } else if node.macro_token.as_ref() == Some(&Roff(RoffMacro::Ta)) && !node.flags.no_print {
            self.tab_stops = super::Arc::new(super::TabStops::from_arguments(
                node.children
                    .iter()
                    .filter_map(libmandoc_rs::Node::decoder_text),
            ));
            if let Some(definition) = &mut self.definition {
                definition.field_buffer.configure_tabs(&self.tab_stops);
                definition.projected_passes = 0;
            }
            self.plain_input.buffer.configure_tabs(&self.tab_stops);
        }
    }

    pub(in crate::mandoc) fn reset_default_tabs(&mut self) {
        self.tab_stops = super::Arc::new(super::TabStops::default());
        if let Some(definition) = &mut self.definition {
            definition.field_buffer.configure_tabs(&self.tab_stops);
            definition.projected_passes = 0;
        }
        self.plain_input.buffer.configure_tabs(&self.tab_stops);
    }

    pub(in crate::mandoc) fn set_literal_tabs(&mut self) {
        self.tab_stops = super::Arc::new(super::TabStops::literal());
        if let Some(definition) = &mut self.definition {
            definition.field_buffer.configure_tabs(&self.tab_stops);
            definition.projected_passes = 0;
        }
        self.plain_input.buffer.configure_tabs(&self.tab_stops);
    }
}

impl InlineExecutionState {
    /// One outer source-line settle already executed the native gate for
    /// this exact node. Preserve the observation mode; source coordinates
    /// can repeat during macro expansion and never identify this receipt.
    pub(in crate::mandoc) fn note_no_fill_source_line_entered(&mut self, node: u32) {
        use SourceLineObservation::{Disabled, DisabledEntered, Grouped, NoFill, NoFillEntered};
        self.observe_no_fill_source_lines = match self.observe_no_fill_source_lines {
            Grouped => Grouped,
            Disabled | DisabledEntered(_) => DisabledEntered(node),
            NoFill | NoFillEntered(_) => NoFillEntered(node),
        };
    }

    /// Consume at the first actual node dispatcher. A different child is a
    /// fresh source event and retains its original observation policy.
    pub(in crate::mandoc) fn take_no_fill_source_line_entered(&mut self, node: u32) -> bool {
        use SourceLineObservation::{Disabled, DisabledEntered, NoFill, NoFillEntered};
        match self.observe_no_fill_source_lines {
            DisabledEntered(id) => {
                self.observe_no_fill_source_lines = Disabled;
                id == node
            }
            NoFillEntered(id) => {
                self.observe_no_fill_source_lines = NoFill;
                id == node
            }
            _ => false,
        }
    }

    /// Structural handlers need not enter the root through an inline sink.
    /// Their unused receipt expires when that source node returns.
    pub(in crate::mandoc) fn finish_no_fill_source_line_entry(&mut self, node: u32) {
        if matches!(
            self.observe_no_fill_source_lines,
            SourceLineObservation::DisabledEntered(id) | SourceLineObservation::NoFillEntered(id)
                if id == node
        ) {
            let _ = self.take_no_fill_source_line_entered(node);
        }
    }

    pub(in crate::mandoc) fn has_native_definition_field(&self) -> bool {
        self.definition.is_some() && self.author_execution.is_some()
    }
    /// Native EOS is supplied by the parser, never inferred from rendered
    /// spelling. Both native node walkers arm it after font/post execution.
    pub(in crate::mandoc) fn finish_executed_node(&mut self, node: &libmandoc_rs::Node) {
        if node.flags.sentence_end {
            self.arm_sentence_end();
        }
    }

    /// Explicit native generated-word posts (the final bibliography period)
    /// set `TERMP_SENTENCE` independently of any authored `NODE_EOS` flag.
    pub(in crate::mandoc) fn arm_sentence_end(&mut self) {
        self.sentence_pending = true;
    }

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
