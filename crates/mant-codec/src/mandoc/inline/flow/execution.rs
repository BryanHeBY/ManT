use super::definition::PendingFieldGapOrigin;
use super::field_buffer::{
    DeferredSourceBlank, FieldBuffer, FieldCell, FieldWrite, RunInWordSlots,
};
use super::native_slots::{EntryCursor, FieldPrefix, PrefixExtension, ReplayDomain};
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

fn retain_projected_advances(
    prefix: &mut FieldPrefix,
    input: &FieldBuffer,
    anchors: &[super::NativeWordAnchor],
) {
    // A real reading seam can already own the advance even when its raw
    // slot is indistinguishable from NBRSP. Keep only existing gap receipts.
    prefix.projected_advances.extend(
        anchors
            .iter()
            .rev()
            .take(prefix.cells.len())
            .take_while(|anchor| anchor.content >= prefix.start)
            .filter_map(super::NativeWordAnchor::projected_gap_cell)
            .filter_map(|gap| {
                let relative = gap.checked_sub(prefix.start)?;
                (prefix.cells.get(relative) == Some(&(FieldCell::NonBreakingBlank, 0))
                    && input.cells()[gap] == FieldCell::FixedDrawingAdvance(1))
                .then_some(relative)
            }),
    );
    prefix.projected_advances.sort_unstable();
    prefix.projected_advances.dedup();
}

fn generated_run_in_writes(traversed: usize, native_count: usize, count: usize) -> Vec<FieldWrite> {
    let mut writes = Vec::new();
    if traversed != 0 {
        writes.push(FieldWrite::AdvanceOverBufferedBlanks(traversed));
    }
    writes.extend((0..native_count).map(|index| FieldWrite::OwnedBlank {
        projected: index >= native_count.saturating_sub(count),
    }));
    writes
}

fn retained_run_in_writes(
    slots: &RunInWordSlots,
    native_count: usize,
    count: usize,
    source: DeferredSourceBlank,
) -> Vec<FieldWrite> {
    // Preserve the generated word as native U+00A0 slots. Unlike a
    // separator SP, those slots are graph to term_fill() and can be
    // revisited by a later operand at this same physical field.
    let mut writes = vec![FieldWrite::ReplaceLivePrefix {
        start: slots.start,
        positions: vec![None; slots.overwritten()],
        trailing_blank_slots: Vec::new(),
        retained_advances: std::collections::BTreeMap::new(),
        adopted_advances: Vec::new(),
        first_incoming: slots.start,
        first_incoming_ink: None,
        entry_blanks: 0,
        entry_gap: None,
        unpublished: None,
    }];
    if slots.has_backline() {
        writes.extend([
            FieldWrite::Placed {
                cell: FieldCell::Backline,
                scalars: 0,
            },
            FieldWrite::CancelBackbefore,
        ]);
    }
    writes.extend((0..native_count).map(|index| {
        let cell = FieldCell::Graph {
            text: '\u{a0}',
            width: 1,
        };
        if index >= native_count.saturating_sub(count) {
            FieldWrite::PlacedBlank { source, cell }
        } else {
            FieldWrite::Placed { cell, scalars: 0 }
        }
    }));
    writes.push(FieldWrite::ProveRawSlots {
        count: native_count + usize::from(slots.has_backline()),
        cursor_retreat: 0,
        backbefore: false,
        duplicates: Vec::new(),
        editable_marker: false,
    });
    if slots.has_backline() {
        writes.push(FieldWrite::PlacedZeroWidthRetreat {
            target_from_end: native_count + 2,
            mark_from_end: native_count,
        });
    }
    writes
}

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
            // post or deferred projection belongs to that outer HEAD, not this one.
            state.no_break = None;
            state.no_break_cleared = false;
            state.head_flags_cleared = false;
            state.cleared_field_capacity_columns = 0;
            state.deferred_head_projection = None;
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

    pub(in crate::mandoc) fn clear_word_end_break(&mut self) {
        self.execution.word_end_break = WordEndBreak::Clear;
    }

    pub(in crate::mandoc) fn note_zero_advance_join(&mut self) {
        self.execution.zero_advance_joined = true;
    }

    pub(in crate::mandoc) fn begin_executed_node(&mut self, node: &libmandoc_rs::Node) {
        self.begin_source_row(node);
        // NODE_LINE settles the previous field under its old settings;
        // roff_term_pre_ta() then changes stops for the subsequent field.
        self.execution.execute_tab_configuration(node);
        if self.execution.observe_no_fill_source_lines == SourceLineObservation::Grouped {
            return;
        }
        // mdoc_term.c keys KEEP lifetime from each executed NODE_LINE event,
        // not from the numeric source coordinate. User-macro expansion can
        // execute several input rows that all retain the call site's line.
        if self.execution.keep.keeping() && node.flags.line_start {
            self.execution.keep.phase = KeepPhase::PreKeep;
        }
    }

    /// The line/fill preamble precedes the native NOPRT/comment return.
    pub(in crate::mandoc) fn begin_source_row(&mut self, node: &libmandoc_rs::Node) {
        if self.execution.observe_no_fill_source_lines == SourceLineObservation::Grouped {
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
            && self.table_cell.is_none()
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

    pub(in crate::mandoc) fn with_implicit_word_keep(&mut self, append: impl FnOnce(&mut Self)) {
        // mdoc_term.c::termp_xx_pre/post only install and retire PREKEEP
        // when the caller has no keep. Promoting PREKEEP leaves it armed.
        let owns_keep = self.execution.keep.phase == KeepPhase::Inactive;
        if owns_keep {
            self.enter_keep_words();
        }
        append(self);
        if owns_keep {
            self.exit_keep_words();
        }
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
            state.field_buffer.resume_run_in_owner();
            self.execution.definition = Some(state);
            if let Some(author) = &mut self.execution.author_execution {
                author.break_effect = field.author_effect;
                author.field_output_start = 0;
            }
        }
    }

    /// Execute the generated no-break word between an mdoc inset/diagnostic
    /// head and body (`mdoc_term.c:757-772, 939-945`, `chars.c:50`,
    /// `term.c:620-636, 888-930`). Its `term_word()` transition still runs
    /// when the only fixed cell was consumed by a completed HEAD glyph.
    pub(in crate::mandoc) fn append_run_in_cells(
        &mut self,
        count: usize,
        native_count: usize,
        generated_word: bool,
    ) {
        if !generated_word {
            return;
        }
        if self.try_replay_run_in_cells(count, native_count) {
            return;
        }

        let input = self.execution.word_input().0;
        // This receipt represents an undecorated native U+00A0 slot.
        let retained_slots = (self.execution.font.current == super::Font::Regular)
            .then(|| input.retained_run_in_word(native_count))
            .flatten()
            .filter(|slots| {
                !slots.appends_backline() || self.execution.zero_advance.can_project_prior_slot()
            });
        let (traversed, armed_overlap) = if retained_slots.is_some() {
            (0, false)
        } else {
            input.generated_blank_overlap(native_count)
        };
        let native_count = native_count - traversed;
        let projected_overlap = count.saturating_sub(native_count);
        let count = count.min(native_count);
        self.execution.native_word_boundary = Some(PendingBoundary::Tight);
        self.note_produced_formatter_cell(native_count > 0);
        self.tighten_next_boundary();
        self.begin_word_projection(true);
        self.begin_native_word_owner();
        let source = DeferredSourceBlank {
            owner: self.execution.native_owner_serial,
            write: 0,
            font: self.execution.font.display_current(),
            replayable_tail: false,
            generated_run_in: true,
        };
        let mut projected = Vec::new();
        let writes = if let Some(slots) = retained_slots {
            let pending_owner = self.execution.zero_advance.pending_word_owner();
            self.execution
                .zero_advance
                .release_field_owned_ink(&mut projected);
            if slots.appends_backline()
                && let Some(owner) = pending_owner
                && let Some(receiver) = self
                    .execution
                    .definition
                    .as_ref()
                    .and_then(|definition| definition.deferred_head_projection.as_ref())
            {
                receiver.restore_pending_ink(owner, &mut projected);
            }
            if count > 0 {
                projected.push(Inline::anchor(source.marker()));
            }
            retained_run_in_writes(&slots, native_count, count, source)
        } else {
            if traversed > 0 && self.defer_run_in_overlap(traversed, projected_overlap, source) {
                projected.push(Inline::anchor(source.marker()));
            }
            self.execution.zero_advance.append_generated_cells(
                count,
                armed_overlap,
                &mut projected,
                source.font,
            );
            generated_run_in_writes(traversed, native_count, count)
        };
        self.finish_run_in_word(projected, writes, count);
    }

    /// Execute `-inset`/`-diag` generated `\ ` / `\ \ ` through
    /// `NativeSlots::with_field_prefix` + `PlacedWord::prepare` when the live
    /// HEAD prefix is retreated or positioned with `BACKAFTER` armed
    /// (`mdoc_term.c:757-772, 939-945`, `chars.c:50`, `term.c:620-636, 888-930`).
    fn try_replay_run_in_cells(&mut self, count: usize, native_count: usize) -> bool {
        // In an inset, a single generated U+00A0 can execute after HEAD's
        // font scope pops back to an outer bold or italic selection.
        // UTF-8 encode1 writes two extra raw cells for that selection
        // (mdoc_term.c:760–772; term.c:903–929). Prove the complete marked
        // BACKBEFORE tape before extending the existing regular replay.
        let native_font = self.execution.font.current;
        let decoration = match native_font {
            super::Font::Regular => 0,
            super::Font::Strong | super::Font::Emphasis if native_count == 1 => 2,
            _ => return false,
        };
        let Some(prefix) = self.proved_run_in_prefix(native_count, decoration) else {
            return false;
        };
        let domain = if self
            .execution
            .author_execution
            .is_some_and(|author| author.break_effect == AuthorBreakEffect::Line)
        {
            ReplayDomain::DefinitionLine
        } else {
            ReplayDomain::Scoped
        };
        let display_font = self.execution.font.display_current();
        let owner_serial = self.execution.native_owner_serial.wrapping_add(1);
        let pending_owner = self.execution.zero_advance.pending_word_owner();
        let mut replay_zero_advance = self.execution.zero_advance.clone();
        replay_zero_advance
            .set_native_word_owner(owner_serial, self.head_operand_capture.is_some());
        let mut projected = Vec::new();
        let mut pending_head_ink = Vec::new();
        let Some(writes) = crate::mandoc::inline::text_execution::replay_run_in_word(
            &prefix,
            (count.min(native_count), native_count),
            owner_serial,
            (display_font, native_font),
            domain,
            &mut replay_zero_advance,
            (&mut projected, &mut pending_head_ink),
        ) else {
            return false;
        };
        if !pending_head_ink.is_empty() {
            if let Some(owner) = pending_owner
                && let Some(receiver) = self
                    .execution
                    .definition
                    .as_ref()
                    .and_then(|definition| definition.deferred_head_projection.as_ref())
            {
                receiver.restore_pending_ink(owner, &mut pending_head_ink);
            }
            if !pending_head_ink.is_empty() {
                projected.splice(0..0, pending_head_ink);
            }
        }
        self.execution.native_word_boundary = Some(PendingBoundary::Tight);
        self.note_produced_formatter_cell(native_count > 0);
        self.tighten_next_boundary();
        self.begin_word_projection(true);
        self.begin_native_word_owner();
        debug_assert_eq!(self.execution.native_owner_serial, owner_serial);
        self.execution.zero_advance = replay_zero_advance;
        self.finish_run_in_word(projected, writes, count.min(native_count));
        true
    }

    fn proved_run_in_prefix(
        &self,
        native_count: usize,
        decoration: usize,
    ) -> Option<super::native_slots::FieldPrefix> {
        if native_count == 0 {
            return None;
        }
        let (input, anchors) = self.execution.word_input();
        let marked_backbefore = input.has_marked_backbefore_tail()
            && (!self.execution.zero_advance.has_buffered_glyph()
                || self.execution.zero_advance.can_project_prior_slot());
        let replaces_source_space = native_count == 1 && input.has_proved_inset_separator_entry();
        let replays_inset_zero_width =
            native_count == 1 && input.has_proved_inset_zero_width_separator_entry();
        if decoration != 0 {
            // BACKBEFORE first replaces the trailing SP or appends BS
            // after another cell. Then encode1 emits the active font's raw
            // copies (term.c:903–929). Keep the full-prefix check below.
            if !(marked_backbefore || replaces_source_space || replays_inset_zero_width) {
                return None;
            }
        } else if !(input.has_bounded_decorated_run_in_projection()
            || replaces_source_space
            || replays_inset_zero_width
            || native_count == 2 && input.has_proved_diag_separator_entry()
            || input.has_retained_word_cursor()
            || marked_backbefore
            || input.has_positioned_slots()
                && (input.has_backafter_armed()
                    || input.has_entry_retreat()
                        && self.execution.zero_advance.can_project_prior_slot()))
        {
            return None;
        }
        let mut prefix = input.field_prefix(0, false, true)?;
        if prefix
            .raw_slot_count()
            .saturating_add(native_count + decoration + 1)
            > super::field_buffer::MAX_EDITABLE_APPEND_CELLS
            || decoration != 0 && (prefix.start != 0 || !prefix.backbefore)
            || replays_inset_zero_width && prefix.start != 0
        {
            return None;
        }
        retain_projected_advances(&mut prefix, input, anchors);
        if prefix.pending_cell.is_none()
            && prefix.backbefore
            && self.execution.zero_advance.has_buffered_glyph()
        {
            // `detach_projection_owner()` cleared `backbefore_graph` on
            // HEAD drain while `TERMP_BACKBEFORE` and the buffered glyph
            // stay live (`term.c:903-929`). Identify the pending slot
            // before `cursor` so `settle_prior_slot()` proves survival.
            let cursor = prefix.cells.len().saturating_sub(prefix.cursor_retreat);
            prefix.pending_cell = prefix.cells[..cursor].iter().rposition(|(cell, scalars)| {
                matches!(cell, FieldCell::Graph { .. } | FieldCell::Hyphen)
                    || *scalars > 0 && matches!(cell, FieldCell::ZeroWidthGraph)
            });
        }
        Some(prefix)
    }

    fn finish_run_in_word(
        &mut self,
        projected: Vec<Inline>,
        writes: Vec<FieldWrite>,
        count: usize,
    ) {
        self.execution.native_word_writes = Some(writes);
        self.append_word(projected);
        if count > 0 {
            self.execution.trailing_output = TrailingOutput::FixedBlank;
        }
        // CVS sets TERMP_NOSPACE again before executing BODY children (`mdoc_term.c:772`).
        self.tighten_next_boundary();
        self.execution.final_word_join = Some(false);
        self.execution.final_source_continuation = Some(false);
    }

    fn defer_run_in_overlap(
        &mut self,
        traversed: usize,
        projected: usize,
        source: DeferredSourceBlank,
    ) -> bool {
        let buffer = if self.execution.uses_definition_word_buffer() {
            &mut self
                .execution
                .definition
                .as_mut()
                .expect("run-in field")
                .field_buffer
        } else {
            &mut self.execution.plain_input.buffer
        };
        buffer.defer_run_in_overlap(traversed, projected, source)
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

    pub(in crate::mandoc::inline) fn native_slot_entry(
        &self,
        capture_prefix: bool,
        extend: PrefixExtension,
        leading_retreat: bool,
    ) -> super::native_slots::SlotEntry {
        let (input, anchors) = self.word_input();
        let boundary = self
            .native_word_boundary
            .unwrap_or(super::PendingBoundary::Tight);
        let tight = boundary.is_native_tight();
        let kept_blanks = boundary == super::PendingBoundary::Kept;
        let automatic_blanks =
            usize::from(!tight) * (1 + usize::from(!kept_blanks && self.native_word_sentence));
        let device_tail = input.device_tail();
        let live_edit = input.has_retained_word_cursor()
            || input.ends_with_unresolved_backline()
            || device_tail
                .as_ref()
                .is_some_and(super::native_slots::DeviceTail::has_retreated);
        let domain = if self.table_cell.is_some() {
            ReplayDomain::Scoped
        } else if self.no_fill_word_active {
            if self.uses_definition_word_buffer() {
                ReplayDomain::LiteralDefinitionLine
            } else {
                ReplayDomain::Scoped
            }
        } else if self.definition.is_none() {
            ReplayDomain::Filled
        } else if self
            .author_execution
            .is_some_and(|author| author.break_effect == AuthorBreakEffect::Line)
        {
            ReplayDomain::DefinitionLine
        } else {
            ReplayDomain::Scoped
        };
        let filled_line_domain =
            matches!(domain, ReplayDomain::Filled | ReplayDomain::DefinitionLine);
        let leading_marker_extend = matches!(extend, PrefixExtension::LeadingMarker);
        let extend = match extend {
            PrefixExtension::None => false,
            PrefixExtension::Word => {
                !input.has_pending_break_markers()
                    || input.word_scan_deferred()
                    || input.pending_pass_is_definitively_rejected()
            }
            PrefixExtension::FilledWord => {
                filled_line_domain
                    && (!input.has_pending_break_markers() || input.word_scan_deferred())
            }
            // A new leading marker is itself a cell write, not a flush of
            // the preceding marker. field_prefix still excludes any pass
            // already consumed by the incremental scanner.
            PrefixExtension::LeadingMarker => true,
        };
        super::native_slots::SlotEntry {
            cursor: if input.is_empty() {
                EntryCursor::Empty
            } else if input.writes_at_buffer_end() {
                EntryCursor::AtEnd
            } else {
                EntryCursor::NeedsReplay
            },
            domain,
            automatic_blanks,
            kept_blanks,
            trailing_tab_reference: input.has_live_tab_reference(),
            definition_row_occupied: self
                .definition
                .as_ref()
                .is_some_and(|state| state.hang_row.native_row_occupied()),
            retreat_blanks: if !kept_blanks && input.has_entry_retreat() {
                automatic_blanks
            } else {
                0
            },
            device_tail,
            prefix: (capture_prefix
                || live_edit
                || leading_retreat && !input.has_entry_retreat()
                || filled_line_domain && input.has_replayable_trailing_advance()
                || extend)
                .then(|| {
                    let mut prefix = input.field_prefix(
                        automatic_blanks,
                        kept_blanks,
                        leading_retreat
                            || leading_marker_extend
                            || input.has_retained_word_cursor()
                            || filled_line_domain && input.awaits_flush(),
                    )?;
                    retain_projected_advances(&mut prefix, input, anchors);
                    Some(prefix)
                })
                .flatten(),
        }
    }

    pub(super) fn acknowledge_word_input_pass(&mut self, end: usize) {
        let buffer = if self.uses_definition_word_buffer() {
            &mut self
                .definition
                .as_mut()
                .expect("authored word field")
                .field_buffer
        } else {
            &mut self.plain_input.buffer
        };
        buffer.acknowledge_projected_pass(end);
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
            }
            self.plain_input.buffer.configure_tabs(&self.tab_stops);
        }
    }

    pub(in crate::mandoc) fn reset_default_tabs(&mut self) {
        self.tab_stops = super::Arc::new(super::TabStops::default());
        if let Some(definition) = &mut self.definition {
            definition.field_buffer.configure_tabs(&self.tab_stops);
        }
        self.plain_input.buffer.configure_tabs(&self.tab_stops);
    }

    pub(in crate::mandoc) fn set_literal_tabs(&mut self) {
        self.tab_stops = super::Arc::new(super::TabStops::literal());
        if let Some(definition) = &mut self.definition {
            definition.field_buffer.configure_tabs(&self.tab_stops);
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
