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
    /// Publish a validated structural target into its active output at entry.
    /// A column keeps the same vector until its real field post; a detached
    /// display instead selects its inline owner using the source node's mode.
    pub(in crate::mandoc::blocks) fn append_native_target(&mut self, node: &libmandoc_rs::Node) {
        if let Some(target) = crate::mandoc::targets::owned_target(node) {
            let owner = if self.formatter.execution.has_column_output_scope() {
                RowOwner::from_literal(self.uses_literal_output())
            } else {
                self.row_owner(node.flags.no_fill)
            };
            self.with_row_builder(owner, |builder| builder.append(vec![target.into_anchor()]));
        }
    }

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
        subsection: bool,
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
        if InlineBuilder::has_live_section_input(&self.formatter.execution, nodes) {
            let result = run(&mut self.formatter, nodes, owner == RowOwner::Literal);
            if owner == RowOwner::Literal {
                self.literal.finish_section_handoff();
            }
            return Some(result);
        }
        if !subsection || !nodes.is_empty() || !self.formatter.execution.has_detached_printed_row()
        {
            return None;
        }
        let mant_ir::Block::Paragraph {
            children,
            source,
            layout,
            ..
        } = self.output.last_mut()?
        else {
            return None;
        };
        // A filled HP post can publish a paragraph before ending its
        // native device row (man_term.c::post_HP, term.c:244-253).
        // Lend only the padding-only projection of that open row.
        let mut fixed_padding = false;
        let mut only_padding = true;
        mant_ir::visit_inline_plain_text(children, |text| {
            fixed_padding |= text.contains('\u{a0}');
            only_padding &= !text.contains('\n') && text.chars().all(char::is_whitespace);
        });
        if !fixed_padding || !only_padding {
            return None;
        }
        let result = run(&mut self.formatter, children, false);
        if children.is_empty() {
            let (lines, source) = (layout.spacing_before_lines, *source);
            self.output.pop();
            if lines > 0 {
                self.output
                    .push(mant_ir::Block::VerticalSpace { lines, source });
            }
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
