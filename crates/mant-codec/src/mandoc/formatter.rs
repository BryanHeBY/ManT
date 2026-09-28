//! Explicit persistent state, separate from source services and local joins.
use libmandoc_rs::AuthorMode;

use super::inline::{
    AuthorBreakEffect, InlineBuilder, InlineExecutionState, NoFillInlineState, PreservedInlineState,
};

pub(super) struct FinishedInlineLine {
    pub(super) output: Vec<mant_ir::Inline>,
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
        let definition_field_exited = builder.definition_field_exited();
        let definition_body_gap_consumed = builder.definition_body_gap_consumed();
        let (output, mut execution) = builder.finish_formatter_line();
        std::mem::swap(&mut execution, &mut self.execution);
        self.spare_execution = Some(execution);
        FinishedInlineLine {
            output,
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

    pub(super) fn clear_zero_advance(&mut self) {
        self.execution.take_zero_advance_armed();
    }

    /// Execute one mdoc `.An` mode or name in source order.
    ///
    /// Returns whether the name starts a new formatter line.  The first
    /// ordinary name in AUTHORS enables splitting unless `-nosplit` is active.
    pub(super) fn execute_author(
        &mut self,
        mode: Option<AuthorMode>,
        authors_section: bool,
    ) -> bool {
        let mut flow = self.author_flow();
        let breaks = flow.execute(mode, authors_section);
        self.execution.set_author_flow(flow);
        breaks
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
