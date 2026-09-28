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
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DefinitionBodyObservation {
    before_visible: bool,
    pending_head_row: bool,
    placement_breaks: bool,
}

impl DefinitionBodyObservation {
    pub(super) const fn placement_breaks(self) -> bool {
        self.placement_breaks
    }
}

impl Clone for FormatterState {
    fn clone(&self) -> Self {
        // Clones are speculative table/definition forks, never a second
        // source-order walk. A BODY post executed there must not mark the
        // live walk as closed through ScopePostState's shared record.
        let mut execution = self.execution.clone();
        execution.scope_posts = crate::mandoc::containers::ScopePostState::default();
        Self {
            execution,
            spare_execution: Some(InlineExecutionState::default()),
            no_fill: self.no_fill,
            no_fill_inline: self.no_fill_inline.clone(),
            row_handoff: self.row_handoff,
            definition_bodies: self.definition_bodies.clone(),
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
    pub(super) fn begin_definition_body(&mut self, shares_pending_head_row: bool) {
        self.definition_bodies.push(DefinitionBodyObservation {
            before_visible: true,
            pending_head_row: shares_pending_head_row,
            placement_breaks: false,
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

    pub(super) fn note_definition_visible(&mut self) {
        // A nested list or definition is also visible content in every
        // enclosing BODY. Do not let an outer pending head row consume a
        // later control-only row after the nested content has appeared.
        for body in &mut self.definition_bodies {
            body.before_visible = false;
            body.pending_head_row = false;
        }
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
        if builder.discarded_exited_definition_buffer() {
            // mdoc_term.c::termp_it_post() calls term_newln() for the HEAD.
            // A buffered but unprinted TAG word can complete an empty row
            // after an earlier .sp closed the tag row.
            builder.hard_break();
        }
        // A detached HEAD ends at mdoc_term.c::termp_it_post(), even without
        // an explicit .br. Do not export IR from a HANG field for which
        // term_fill() never produced a printable device slice.
        builder.discard_unprinted_definition_field_output();
        let definition_field_exited = builder.definition_field_exited();
        let definition_body_gap_consumed = builder.definition_body_gap_consumed();
        let definition_term_breaks = builder.take_definition_term_breaks();
        let (output, mut execution) = builder.finish_formatter_line(preserve_rows);
        std::mem::swap(&mut execution, &mut self.execution);
        self.spare_execution = Some(execution);
        FinishedInlineLine {
            output,
            definition_term_breaks,
            definition_field_exited,
            definition_body_gap_consumed,
        }
    }

    /// Commit persistent registers while leaving the local inline execution
    /// state available to a run-in continuation in the surrounding stream.
    pub(super) fn finish_inline_scope(
        &mut self,
        builder: InlineBuilder,
    ) -> (Vec<mant_ir::Inline>, PreservedInlineState) {
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
