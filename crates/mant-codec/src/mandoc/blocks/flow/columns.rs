//! Execute native column entry and posts against the active output destination.
use super::{BlockState, InlineBuilder, InlineOutputRole};

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

    pub(in crate::mandoc::blocks) fn inline_output_is_literal(&self, no_fill: bool) -> bool {
        self.uses_literal_output()
            || no_fill && self.inline_output_role == InlineOutputRole::SourceMode
    }

    pub(in crate::mandoc::blocks) fn note_inline_source(
        &mut self,
        source: Option<mant_ir::SourceSpan>,
    ) {
        self.adopt_inline_output_owner();
        if self.inline_output_is_literal(self.formatter.no_fill) {
            self.literal.note_source(source);
        } else {
            self.paragraph.note_source(source);
        }
    }

    /// Native operations borrow the active destination and its one input
    /// session. The walker dispatches source entry independently of owners.
    pub(in crate::mandoc::blocks) fn with_active_output_builder<R>(
        &mut self,
        operation: impl FnOnce(&mut InlineBuilder) -> R,
    ) -> R {
        self.adopt_inline_output_owner();
        let visible_before = self.formatter.execution.visible_content_checkpoint();
        let result = if self.inline_output_is_literal(self.formatter.no_fill) {
            self.literal
                .with_inline_builder(&mut self.formatter, operation)
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, operation)
        };
        self.observe_inline_source_effects(visible_before);
        result
    }

    pub(in crate::mandoc::blocks) fn enter_name_body_node(
        &mut self,
        node: &libmandoc_rs::Node,
        width: usize,
    ) -> crate::mandoc::inline::names::NameBodyScope {
        self.formatter.no_fill = node.flags.no_fill;
        self.with_active_output_builder(|builder| {
            builder.observe_no_fill_source_lines(true);
            crate::mandoc::inline::names::enter_body(builder, node, width)
        })
    }

    pub(in crate::mandoc::blocks) fn finish_name_body_node(
        &mut self,
        node: &libmandoc_rs::Node,
        saved: crate::mandoc::inline::names::NameBodyScope,
    ) {
        let finish = |builder: &mut InlineBuilder| {
            crate::mandoc::inline::names::finish_body(builder, node, saved);
        };
        if self.inline_output_is_literal(self.formatter.no_fill) {
            self.literal
                .with_inline_builder(&mut self.formatter, finish);
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, finish);
        }
        self.formatter.no_fill_inline.retire_consumed_cell();
        if !self.inline_output_is_literal(self.formatter.no_fill) {
            // The actual Nm BODY post has already flushed the native field.
            // Its completed owner can now retain the BODY's resolved wrap
            // origin before print_mdoc_node restores the source scope.
            self.flush_paragraph();
        }
    }

    /// A display changes the active IR destination, not its native field.
    /// Keep the exact vector/anchor coordinate system until its real post.
    pub(in crate::mandoc::blocks) fn enter_column_display_output(&mut self) {
        self.column_display_owner = true;
        if self.literal.is_empty() && !self.paragraph.is_empty() {
            let (nodes, source) = self.paragraph.take_active_output();
            let occupied = self.formatter.execution.has_formatter_cell()
                || self.formatter.execution.has_open_native_device_row();
            self.literal.adopt_active_output(nodes, source, occupied);
        }
    }

    pub(super) fn uses_literal_output(&self) -> bool {
        self.single_line_display_owner
            || self.formatter.execution.has_column_output_scope()
                && (self.column_display_owner || self.literal.has_output())
    }

    /// A no-fill word clears CVS skipvsp; its zero-width registers are
    /// already updated by the shared text executor.
    pub(in crate::mandoc::blocks) fn begin_column_body(
        &mut self,
        width: u16,
        origin: usize,
        last: bool,
    ) {
        let parent_units = self.indent_columns.physical_basic_units();
        self.paragraph
            .with_inline_builder(&mut self.formatter, |builder| {
                builder.begin_column_body(width, origin, last);
                builder.set_column_reading_parent(parent_units);
            });
    }

    pub(in crate::mandoc::blocks) fn finish_column_nested_row(&mut self) {
        if self.formatter.execution.has_column_output_scope() {
            self.finish_native_structural_row();
        }
    }

    /// A real macro pre/post calls `term_newln` independently of its IR owner.
    /// Borrow the active destination so a detached HEAD row is consumed even
    /// when the BODY has no local glyph (`mdoc_term.c::termp_bl_pre`).
    pub(in crate::mandoc::blocks) fn finish_native_structural_row(&mut self) {
        let no_fill = self.formatter.no_fill;
        if self.uses_literal_output()
            || (!self.formatter.execution.has_column_output_scope()
                && self.inline_output_is_literal(no_fill))
        {
            // A structural term_newln closes the currently occupied source
            // row, not a separate layout gap. In man_term.c::pre_RS that
            // row stays in the surrounding no-fill owner's content.
            self.finish_native_line_in_source_mode(no_fill);
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, InlineBuilder::execute_native_newline);
            self.formatter.no_fill_inline.retire_consumed_cell();
        }
    }

    pub(in crate::mandoc::blocks) fn finish_column_display_body(
        &mut self,
        kind: Option<libmandoc_rs::DisplayKind>,
    ) {
        let post = |builder: &mut InlineBuilder| builder.finish_display_body(kind);
        if self.inline_output_is_literal(self.formatter.no_fill) {
            self.literal.with_inline_builder(&mut self.formatter, post);
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, post);
        }
        self.formatter.no_fill_inline.retire_consumed_cell();
    }

    pub(in crate::mandoc::blocks) fn enter_column_node(&mut self, node: &libmandoc_rs::Node) {
        let enter = |builder: &mut InlineBuilder| {
            builder.begin_executed_node(node);
        };
        if self.uses_literal_output() {
            self.literal.with_inline_builder(&mut self.formatter, enter);
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, enter);
        }
    }

    pub(in crate::mandoc::blocks) fn enter_column_body_node(&mut self, node: &libmandoc_rs::Node) {
        self.formatter.no_fill = node.flags.no_fill;
        self.paragraph
            .with_inline_builder(&mut self.formatter, |builder| {
                builder.observe_no_fill_source_lines(true);
                builder.begin_executed_node(node);
            });
    }

    pub(in crate::mandoc::blocks) fn column_vertical_space(&mut self, rows: i32) {
        let execute = |builder: &mut InlineBuilder| {
            builder.native_vertical_space(u16::try_from(rows.max(0)).unwrap_or(u16::MAX));
        };
        if self.uses_literal_output() {
            self.literal
                .with_inline_builder(&mut self.formatter, execute);
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, execute);
        }
        self.formatter.no_fill_inline.retire_consumed_cell();
    }
}
