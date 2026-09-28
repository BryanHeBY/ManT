//! Explicit persistent state, separate from source services and local joins.
use libmandoc_rs::AuthorMode;

use super::inline::{AuthorBreakEffect, FontState, InlineBuilder, PreservedInlineState};

pub(super) struct FinishedInlineLine {
    pub(super) output: Vec<mant_ir::Inline>,
    pub(super) definition_field_exited: bool,
    pub(super) definition_body_gap_consumed: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::mandoc) enum AuthorFlow {
    #[default]
    Automatic,
    Split,
    NoSplit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FormatterState {
    pub(super) font: FontState,
    pub(super) spacing: bool,
    /// Source-order roff fill channel. List bodies return this state to their
    /// parent; a literal Bd restores its inbound channel at its own post.
    pub(super) no_fill: bool,
    /// CVS `termp.skipvsp` is formatter-global: structural and presentation
    /// scopes do not clear it, while the next real formatter word does.
    pub(super) vertical_space_debt: u16,
    /// Bare CVS `TERMP_BACKAFTER` state.  A structural newline with no
    /// formatter cell does not clear it; the next real or generated word
    /// consumes it.  Tables are the deliberate exception and clear both
    /// backtracking flags before rendering each native cell.
    pub(super) zero_advance_armed: bool,
    /// CVS `TERMP_SPLIT`/`TERMP_NOSPLIT` persist across intervening inline
    /// nodes and nested block lowerers; they are not `.An` adjacency.
    author_flow: AuthorFlow,
}

impl Default for FormatterState {
    fn default() -> Self {
        Self {
            font: FontState::new(),
            spacing: true,
            no_fill: false,
            vertical_space_debt: 0,
            zero_advance_armed: false,
            author_flow: AuthorFlow::default(),
        }
    }
}

impl FormatterState {
    /// Execute one source-order node in an already live paragraph builder.
    /// The builder owns joins and buffered IR; this carrier owns the font
    /// register between nodes. There is no independently writable font copy
    /// while the node is executing.
    pub(super) fn with_inline_node<R>(
        &mut self,
        builder: &mut InlineBuilder,
        execute: impl FnOnce(&mut InlineBuilder) -> R,
    ) -> R {
        // The paragraph keeps a neutral slot between nodes. Swap the live
        // register into it for this node, then return that same register to
        // the formatter. CVS man_html.c::print_man_node processes one node's
        // text/font before walking to the next sibling.
        debug_assert_eq!(builder.font, FontState::new());
        std::mem::swap(&mut builder.font, &mut self.font);
        let result = execute(builder);
        std::mem::swap(&mut builder.font, &mut self.font);
        self.spacing = builder.spacing_enabled();
        self.vertical_space_debt = builder.vertical_space_debt();
        result
    }

    /// Move document-global registers into one active inline session.
    ///
    /// Keeping this paired with [`Self::finish_inline_line`] and
    /// [`Self::finish_inline_scope`] prevents callers from silently omitting
    /// a newly added execution register during a structural handoff. Live
    /// paragraph flows and isolated table fragments intentionally have split
    /// ownership and do not use this author-aware session.
    pub(super) fn begin_inline_session(
        &mut self,
        spacing: bool,
        authors_section: bool,
        author_break_effect: AuthorBreakEffect,
    ) -> InlineBuilder {
        let mut builder = InlineBuilder::with_spacing(spacing);
        builder.font = std::mem::replace(&mut self.font, FontState::new());
        self.spacing = true;
        builder.inherit_vertical_space_debt(std::mem::take(&mut self.vertical_space_debt));
        builder.inherit_zero_advance_armed(std::mem::take(&mut self.zero_advance_armed));
        builder.inherit_author_execution_with_effect(
            std::mem::take(&mut self.author_flow),
            authors_section,
            author_break_effect,
        );
        builder
    }

    /// Commit an inline session at a native formatter-line boundary.
    pub(super) fn finish_inline_line(&mut self, builder: InlineBuilder) -> FinishedInlineLine {
        self.inherit_inline_registers(&builder);
        let definition_field_exited = builder.definition_field_exited();
        let definition_body_gap_consumed = builder.definition_body_gap_consumed();
        let (output, surviving_armed) = builder.finish_formatter_line();
        self.zero_advance_armed = surviving_armed;
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
        self.inherit_inline_registers(&builder);
        self.zero_advance_armed = false;
        builder.finish_preserving_execution()
    }

    fn inherit_inline_registers(&mut self, builder: &InlineBuilder) {
        if let Some(author_flow) = builder.author_flow() {
            self.author_flow = author_flow;
        }
        self.font = builder.font;
        self.spacing = builder.spacing_enabled();
        self.vertical_space_debt = builder.vertical_space_debt();
    }

    /// Enter the body of a top-level mdoc AUTHORS section.
    ///
    /// CVS clears both renderer-global author switches at this exact point;
    /// subsections inherit the resulting mode and unrelated sections do not
    /// reset an explicitly selected mode.
    pub(super) fn enter_authors_section(&mut self) {
        self.author_flow = AuthorFlow::Automatic;
    }

    /// Execute one formatter word at a structural boundary.
    ///
    /// The visible spelling belongs to the renderer-neutral IR, but CVS
    /// still routes list markers and similar generated text through
    /// `term_word()`, which clears pending vertical-space debt.
    pub(super) fn execute_word(&mut self) {
        self.vertical_space_debt = 0;
        self.zero_advance_armed = false;
    }

    pub(super) fn clear_zero_advance(&mut self) {
        self.zero_advance_armed = false;
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
        self.author_flow.execute(mode, authors_section)
    }

    pub(super) const fn author_flow(&self) -> AuthorFlow {
        self.author_flow
    }

    pub(super) fn set_author_flow(&mut self, flow: AuthorFlow) {
        self.author_flow = flow;
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
