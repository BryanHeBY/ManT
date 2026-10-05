//! Execute native column entry and posts against the active output destination.
use super::{BlockState, InlineBuilder};

impl BlockState {
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

    pub(super) fn column_uses_literal_output(&self) -> bool {
        self.formatter.execution.has_column_output_scope()
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
        if self.column_uses_literal_output()
            || (!self.formatter.execution.has_column_output_scope() && self.formatter.no_fill)
        {
            self.literal
                .with_inline_builder(&mut self.formatter, InlineBuilder::execute_native_newline);
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, InlineBuilder::execute_native_newline);
        }
        self.formatter.no_fill_inline.retire_consumed_cell();
    }

    pub(in crate::mandoc::blocks) fn finish_column_display_body(
        &mut self,
        kind: Option<libmandoc_rs::DisplayKind>,
    ) {
        let post = |builder: &mut InlineBuilder| builder.finish_display_body(kind);
        if self.column_uses_literal_output() {
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
        if self.column_uses_literal_output() {
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
        if self.column_uses_literal_output() {
            self.literal
                .with_inline_builder(&mut self.formatter, execute);
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, execute);
        }
        self.formatter.no_fill_inline.retire_consumed_cell();
    }
}
