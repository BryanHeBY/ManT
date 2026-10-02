//! Explicit persistent state, separate from source services and local joins.
use libmandoc_rs::AuthorMode;

use super::inline::{
    AuthorBreakEffect, InlineBuilder, InlineExecutionState, NoFillInlineState, PreservedInlineState,
};

pub(super) struct FinishedInlineLine {
    pub(super) output: Vec<mant_ir::Inline>,
    pub(super) definition_term_breaks: Vec<usize>,
    pub(super) definition_field_exited: bool,
    pub(super) definition_body_gap_consumed: bool,
    pub(super) definition_author_restarted: bool,
}

/// A HEAD post's observed physical row, independent of the output container.
#[derive(Clone, Copy, Default)]
struct DefinitionHeadRows {
    occupied: bool,
    completed_empty_rows: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum RowHandoff {
    #[default]
    None,
    AdoptTrailingLiteral,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::mandoc) enum AuthorFlow {
    #[default]
    Automatic,
    Split,
    NoSplit,
}

pub(super) struct FormatterState {
    /// One execution owner across paragraph, heading, definition, and output
    /// segment lifetimes. Inline builders borrow this state by moving it for
    /// one call and returning it before the next source node is dispatched.
    pub(super) execution: InlineExecutionState,
    // Reused inert slot; source-node execution never allocates a new state.
    spare_execution: Option<InlineExecutionState>,
    /// Source-order roff fill channel. List bodies return this state to their
    /// parent; a literal Bd restores its inbound channel at its own post.
    pub(super) no_fill: bool,
    /// The active no-fill formatter row survives nested block output owners.
    /// CVS term.c keeps the row and backtracking flags in one `termp`; a
    /// `BlockLowerer` is only a destination for IR, not a new formatter.
    pub(super) no_fill_inline: NoFillInlineState,
    /// A crossed display can return while its last literal row is still the
    /// formatter's active row. The parent output sink adopts that row before
    /// writing the next source or generated word.
    row_handoff: RowHandoff,
    /// Definition BODY checkpoints observe the one real source walk. Nested
    /// items push their own checkpoint without replaying their parent body.
    definition_bodies: Vec<DefinitionBodyObservation>,
    /// Last HEAD post's physical row, independent of its rendered term.
    definition_head_rows: DefinitionHeadRows,
}

#[derive(Clone, Copy, Debug)]
// Independent observations of one BODY's native execution, not states.
#[allow(clippy::struct_excessive_bools)]
pub(super) struct DefinitionBodyObservation {
    before_visible: bool,
    pending_head_row: bool,
    placement_breaks: bool,
    source_continues_after_run_in: Option<bool>,
    /// The BODY's first visible word printed under `TERMP_NOSPACE` (the
    /// request's `roff_term_pre_br()` left it, `roff_term.c:75-78`): no
    /// separator exists between the head and the body column.
    first_word_concatenated: bool,
    /// The first visible word concatenated because the cleared head field
    /// filled its capacity (term.c:250-253), not because of request
    /// `TERMP_NOSPACE` (`roff_term.c:78`).
    first_word_flushed_at_body: bool,
}

impl DefinitionBodyObservation {
    pub(super) const fn placement_breaks(self) -> bool {
        self.placement_breaks
    }

    pub(super) const fn source_continues_after_run_in(self) -> Option<bool> {
        self.source_continues_after_run_in
    }

    pub(super) const fn first_word_concatenated(self) -> bool {
        self.first_word_concatenated
    }

    #[must_use]
    pub(super) const fn first_word_flushed_at_body(self) -> bool {
        self.first_word_flushed_at_body
    }
}

impl Clone for FormatterState {
    fn clone(&self) -> Self {
        // Clones are speculative table/definition forks, never a second
        // source-order walk. A BODY post executed there must not mark the
        // live walk as closed through ScopePostState's shared record.
        let mut execution = self.execution.clone();
        execution.scope_posts = crate::mandoc::containers::ScopePostState::default();
        execution.escape_coverage = self.execution.escape_coverage.fork();
        Self {
            execution,
            spare_execution: Some(InlineExecutionState::default()),
            no_fill: self.no_fill,
            no_fill_inline: self.no_fill_inline.clone(),
            row_handoff: self.row_handoff,
            definition_bodies: self.definition_bodies.clone(),
            definition_head_rows: self.definition_head_rows,
        }
    }
}

impl Default for FormatterState {
    fn default() -> Self {
        Self {
            execution: InlineExecutionState::default(),
            spare_execution: Some(InlineExecutionState::default()),
            no_fill: false,
            no_fill_inline: NoFillInlineState::new(),
            row_handoff: RowHandoff::None,
            definition_bodies: Vec::new(),
            definition_head_rows: DefinitionHeadRows::default(),
        }
    }
}

impl std::ops::Deref for FormatterState {
    type Target = InlineExecutionState;

    fn deref(&self) -> &Self::Target {
        &self.execution
    }
}

impl std::ops::DerefMut for FormatterState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.execution
    }
}

impl FormatterState {
    pub(super) const fn definition_head_row_occupied(&self) -> bool {
        self.definition_head_rows.occupied
    }

    pub(super) const fn definition_head_completed_empty_rows(&self) -> u16 {
        self.definition_head_rows.completed_empty_rows
    }

    pub(super) fn enter_man_definition_body(&mut self) {
        // man_term.c::pre_IP/pre_TP(BODY), independent of HEAD fitting:
        // an empty visited TEXT calls term_newln, not term_vspace, until
        // the first actual term_word clears NONEWLINE (term.c::term_word).
        self.execution.enter_man_definition_body();
    }

    pub(super) fn begin_definition_body(&mut self, shares_pending_head_row: bool) {
        self.execution.concat_consumed_for_body = false;
        self.definition_bodies.push(DefinitionBodyObservation {
            before_visible: true,
            pending_head_row: shares_pending_head_row,
            placement_breaks: false,
            source_continues_after_run_in: None,
            first_word_concatenated: false,
            first_word_flushed_at_body: false,
        });
    }

    pub(super) fn finish_definition_body(&mut self) -> DefinitionBodyObservation {
        self.definition_bodies
            .pop()
            .expect("definition body checkpoint must close after its source walk")
    }

    pub(super) fn definition_before_visible(&self) -> bool {
        self.definition_bodies
            .last()
            .is_some_and(|body| body.before_visible)
    }

    pub(super) fn note_definition_boundary(&mut self) {
        for body in &mut self.definition_bodies {
            if body.before_visible {
                body.placement_breaks = true;
            }
        }
    }

    pub(super) fn note_definition_run_in_executed(&mut self) {
        let continuation = self.execution.source_row_continues();
        if let Some(body) = self.definition_bodies.last_mut() {
            body.source_continues_after_run_in = Some(continuation);
        }
    }

    /// Record the source-continuation register as it stands at a BODY word's
    /// source-line entry, before that word's own `term_word()` can clear it
    /// (term.c:588). The last record before the first visible row is the
    /// `TERMP_NONEWLINE` state that row's `NODE_LINE` gate reads
    /// (mdoc_term.c:314-317).
    pub(super) fn note_definition_source_line(&mut self) {
        if let Some(body) = self.definition_bodies.last_mut()
            && body.before_visible
        {
            body.source_continues_after_run_in = Some(self.execution.source_row_continues());
        }
    }

    pub(super) fn note_definition_visible(&mut self) {
        // A nested list or definition is also visible content in every
        // enclosing BODY. Do not let an outer pending head row consume a
        // later control-only row after the nested content has appeared.
        let concatenated = self.execution.concat_consumed_for_body;
        let flushed = self.execution.flush_consumed_for_body;
        let last = self.definition_bodies.len().saturating_sub(1);
        for (index, body) in self.definition_bodies.iter_mut().enumerate() {
            body.before_visible = false;
            body.pending_head_row = false;
            if concatenated && index == last {
                body.first_word_concatenated = true;
                body.first_word_flushed_at_body |= flushed;
            }
        }
        self.execution.concat_consumed_for_body = false;
        self.execution.flush_consumed_for_body = false;
    }

    pub(super) fn consume_definition_head_row(&mut self) -> bool {
        let Some(body) = self.definition_bodies.last_mut() else {
            return false;
        };
        let pending = body.before_visible && body.pending_head_row;
        if pending {
            body.pending_head_row = false;
        }
        pending
    }

    pub(super) fn settle_definition_head_rows(&mut self) {
        // CVS term_newln() closes the current tag row even when that request
        // had no BODY cell to project into IR. A later invisible word starts
        // its own physical row and must not be consumed as the tag row.
        if self
            .definition_bodies
            .iter()
            .any(|body| body.pending_head_row)
            && !self.execution.has_printable_pending_zero_advance_glyph()
        {
            self.execution.discard_zero_advance_at_row_end();
        }
        for body in &mut self.definition_bodies {
            body.pending_head_row = false;
        }
    }

    pub(super) fn definition_head_row_pending(&self) -> bool {
        self.definition_bodies
            .last()
            .is_some_and(|body| body.before_visible && body.pending_head_row)
    }

    pub(super) fn with_output_builder<R>(
        &mut self,
        nodes: &mut Vec<mant_ir::Inline>,
        operation: impl FnOnce(&mut InlineBuilder) -> R,
    ) -> R {
        let mut active = self
            .spare_execution
            .take()
            .expect("only one builder can borrow formatter execution");
        std::mem::swap(&mut active, &mut self.execution);
        let mut builder = InlineBuilder::from_parts(std::mem::take(nodes), active);
        builder.inherit_external_head_row(self.definition_head_row_pending());
        let result = operation(&mut builder);
        let (output, mut returned) = builder.into_parts();
        *nodes = output;
        std::mem::swap(&mut returned, &mut self.execution);
        self.spare_execution = Some(returned);
        result
    }

    pub(super) fn mark_trailing_literal_row(&mut self) {
        self.row_handoff = RowHandoff::AdoptTrailingLiteral;
    }

    pub(super) fn take_trailing_literal_row(&mut self) -> bool {
        matches!(
            std::mem::take(&mut self.row_handoff),
            RowHandoff::AdoptTrailingLiteral
        )
    }

    pub(super) fn clear_trailing_literal_row(&mut self) {
        self.row_handoff = RowHandoff::None;
    }
    /// Move document-global registers into one active inline session.
    ///
    /// Keeping this paired with [`Self::finish_inline_line`] and
    /// [`Self::finish_inline_scope`] returns every register to the same owner.
    /// Ordinary paragraphs borrow through [`Self::with_output_builder`];
    /// table fragments execute in a separately proved local scope.
    pub(super) fn begin_inline_session(
        &mut self,
        spacing: bool,
        authors_section: bool,
        author_break_effect: AuthorBreakEffect,
    ) -> InlineBuilder {
        let flow = self.author_flow();
        self.execution.set_spacing_enabled(spacing);
        let mut active = self
            .spare_execution
            .take()
            .expect("only one builder can borrow formatter execution");
        std::mem::swap(&mut active, &mut self.execution);
        let mut builder = InlineBuilder::from_parts(Vec::new(), active);
        builder.inherit_author_execution_with_effect(flow, authors_section, author_break_effect);
        builder
    }

    /// Commit an inline session at a native formatter-line boundary.
    pub(super) fn finish_inline_line(&mut self, builder: InlineBuilder) -> FinishedInlineLine {
        self.finish_inline_line_with_rows(builder, false)
    }

    pub(super) fn finish_inline_line_with_rows(
        &mut self,
        mut builder: InlineBuilder,
        preserve_rows: bool,
    ) -> FinishedInlineLine {
        // mdoc_term.c::termp_it_post() consumes the old field once. Capture
        // before rejection can move its output start; the outcome below and
        // final drain must retain the same accepted-prefix/device receipt.
        let boundary = builder.prepare_formatter_line();
        let definition_field_exited = builder.definition_field_exited(boundary.native());
        let definition_author_restarted = builder.definition_author_restarted();
        // An armed request NOSPACE (`roff_term.c:78`) already owns the
        // word join; the filled-field rule would misclassify its row.
        if !definition_field_exited
            && !builder.concat_word_armed()
            && builder.cleared_field_filled_capacity(boundary.native())
        {
            // term.c:250-253 with 205-207: the HANG head ended at or past
            // the field's own right margin with NOBREAK cleared, so the
            // trailspace roff_term_pre_br() zeroed never separated the
            // body word (the reference prints `afterwardstail text`).
            builder.note_flushed_at_body_column();
        }
        let definition_body_gap_consumed = builder.definition_body_gap_consumed(boundary.native());
        let completed_empty_rows = builder.completed_empty_rows();
        let definition_term_breaks = builder.take_definition_term_breaks();
        let (output, mut execution, occupied) =
            builder.finish_captured_formatter_line(preserve_rows, boundary);
        self.definition_head_rows = DefinitionHeadRows {
            occupied,
            completed_empty_rows,
        };
        std::mem::swap(&mut execution, &mut self.execution);
        self.spare_execution = Some(execution);
        FinishedInlineLine {
            output,
            definition_term_breaks,
            definition_field_exited,
            definition_body_gap_consumed,
            definition_author_restarted,
        }
    }

    /// Commit persistent registers while leaving the local inline execution
    /// state available to a run-in continuation in the surrounding stream.
    pub(super) fn finish_inline_scope(
        &mut self,
        builder: InlineBuilder,
    ) -> (Vec<mant_ir::Inline>, PreservedInlineState) {
        self.definition_head_rows = DefinitionHeadRows {
            occupied: builder.definition_head_row_occupied(),
            completed_empty_rows: builder.completed_empty_rows(),
        };
        let (output, preserved, mut execution) = builder.finish_preserving_execution();
        std::mem::swap(&mut execution, &mut self.execution);
        self.spare_execution = Some(execution);
        (output, preserved)
    }

    /// Enter the body of a top-level mdoc AUTHORS section.
    ///
    /// CVS clears both renderer-global author switches at this exact point;
    /// subsections inherit the resulting mode and unrelated sections do not
    /// reset an explicitly selected mode.
    pub(super) fn enter_authors_section(&mut self) {
        self.execution.set_author_flow(AuthorFlow::Automatic);
    }

    /// Execute one formatter word at a structural boundary.
    ///
    /// The visible spelling belongs to the renderer-neutral IR, but CVS
    /// still routes list markers and similar generated text through
    /// `term_word()`, which clears pending vertical-space debt.
    pub(super) fn execute_word(&mut self) {
        self.vertical_space_debt = 0;
        self.execution.take_zero_advance_armed();
    }

    /// Execute a generated formatter word that is also visible in IR.
    /// Unlike a control-only tbl word, a list marker ends every enclosing
    /// definition BODY's pending head-row prefix (`mdoc_term.c::termp_it_pre`).
    pub(super) fn execute_visible_generated_word(&mut self) {
        self.execute_word();
        self.note_definition_visible();
    }

    pub(super) fn clear_zero_advance(&mut self) {
        self.execution.take_zero_advance_armed();
    }

    pub(super) fn author_flow(&self) -> AuthorFlow {
        self.execution.author_flow().unwrap_or_default()
    }

    pub(super) fn spacing_enabled(&self) -> bool {
        self.execution.spacing_enabled()
    }

    pub(super) fn set_spacing_enabled(&mut self, enabled: bool) {
        self.execution.set_spacing_enabled(enabled);
    }

    pub(super) fn take_zero_advance_armed(&mut self) -> bool {
        self.execution.take_zero_advance_armed()
    }

    pub(super) fn inherit_zero_advance_armed(&mut self, armed: bool) {
        self.execution.inherit_zero_advance_armed(armed);
    }

    pub(super) fn enter_keep_words(&mut self) {
        self.keep.enter();
    }

    pub(super) fn exit_keep_words(&mut self) {
        self.keep.exit();
    }
}

impl AuthorFlow {
    pub(in crate::mandoc) fn execute(
        &mut self,
        mode: Option<AuthorMode>,
        authors_section: bool,
    ) -> bool {
        match mode {
            Some(AuthorMode::Split) => {
                *self = Self::Split;
                false
            }
            Some(AuthorMode::NoSplit) => {
                *self = Self::NoSplit;
                false
            }
            None => {
                let breaks = *self == Self::Split;
                if authors_section && *self != Self::NoSplit {
                    *self = Self::Split;
                }
                breaks
            }
        }
    }
}
