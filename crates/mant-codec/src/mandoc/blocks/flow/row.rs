//! Borrow the current row's output without changing native execution policy.

use super::{BlockState, InlineBuilder, InlineOutputRole};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RowOwner {
    Paragraph,
    Literal,
}

impl RowOwner {
    pub(super) const fn from_literal(literal: bool) -> Self {
        if literal {
            Self::Literal
        } else {
            Self::Paragraph
        }
    }
}

impl BlockState {
    pub(in crate::mandoc::blocks) const fn inline_output_role(&self) -> InlineOutputRole {
        self.inline_output_role
    }

    pub(in crate::mandoc::blocks) fn set_inline_output_role(
        &mut self,
        role: InlineOutputRole,
    ) -> InlineOutputRole {
        std::mem::replace(&mut self.inline_output_role, role)
    }

    pub(super) fn uses_literal_output(&self) -> bool {
        self.single_line_display_owner
            || self.formatter.execution.has_column_output_scope()
                && (self.column_display_owner || self.literal.has_output())
    }

    pub(in crate::mandoc::blocks) fn inline_output_is_literal(&self, no_fill: bool) -> bool {
        self.uses_literal_output()
            || no_fill && self.inline_output_role == InlineOutputRole::SourceMode
    }

    pub(super) fn row_owner(&self, no_fill: bool) -> RowOwner {
        RowOwner::from_literal(self.inline_output_is_literal(no_fill))
    }

    /// HEAD's native flush can finish the previous owner's still-live input.
    /// Lend its exact projection vector, including native word coordinates.
    pub(in crate::mandoc::blocks) fn with_pending_section_output<R>(
        &mut self,
        run: impl FnOnce(
            &mut crate::mandoc::formatter::FormatterState,
            &mut Vec<mant_ir::Inline>,
            bool,
        ) -> R,
    ) -> Option<R> {
        let owner = self.row_owner(self.formatter.no_fill);
        let nodes = match owner {
            RowOwner::Paragraph => self.paragraph.output_nodes_mut(),
            RowOwner::Literal => self.literal.output_nodes_mut(),
        };
        if !InlineBuilder::has_live_section_input(&self.formatter.execution, nodes) {
            return None;
        }
        let result = run(&mut self.formatter, nodes, owner == RowOwner::Literal);
        if owner == RowOwner::Literal {
            self.literal.finish_section_handoff();
        }
        Some(result)
    }

    /// Borrow the chosen vector and update its local occupancy on return.
    /// Callers choose the owner and whether to execute a native pre or post.
    pub(super) fn with_row_builder<R>(
        &mut self,
        owner: RowOwner,
        operation: impl FnOnce(&mut InlineBuilder) -> R,
    ) -> R {
        match owner {
            RowOwner::Paragraph => self
                .paragraph
                .with_inline_builder(&mut self.formatter, operation),
            RowOwner::Literal => self
                .literal
                .with_inline_builder(&mut self.formatter, operation),
        }
    }

    /// Run a source boundary that ends the old no-fill projection. The
    /// callback's result (such as a device-row close) is independent of
    /// that bookkeeping and does not decide whether to flush native input.
    pub(super) fn with_row_boundary<R>(
        &mut self,
        owner: RowOwner,
        operation: impl FnOnce(&mut InlineBuilder) -> R,
    ) -> R {
        let result = self.with_row_builder(owner, operation);
        self.formatter.retire_no_fill_row_projection();
        result
    }

    pub(in crate::mandoc::blocks) fn note_inline_source(
        &mut self,
        source: Option<mant_ir::SourceSpan>,
    ) {
        self.adopt_inline_output_owner();
        match self.row_owner(self.formatter.no_fill) {
            RowOwner::Paragraph => self.paragraph.note_source(source),
            RowOwner::Literal => self.literal.note_source(source),
        }
    }

    /// Generated words and inline macros also publish their source effects.
    /// A native post can borrow a row without adopting a new destination or
    /// observing a new source word.
    pub(in crate::mandoc::blocks) fn with_active_output_builder<R>(
        &mut self,
        operation: impl FnOnce(&mut InlineBuilder) -> R,
    ) -> R {
        self.adopt_inline_output_owner();
        let visible_before = self.formatter.execution.visible_content_checkpoint();
        let result = self.with_row_builder(self.row_owner(self.formatter.no_fill), operation);
        self.observe_inline_source_effects(visible_before);
        result
    }
}
