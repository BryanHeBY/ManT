//! Execute filled and literal source input against their existing flow owners.
use super::{BlockState, InlineBuilder};
use mant_ir::{Block, Inline};

impl BlockState {
    pub(in crate::mandoc::blocks) fn literal_mode_boundary(&mut self) {
        // Native fi/nf execute term_newln even when the requested mode is
        // already active. Keep adjacent literal blocks coalesced while ending
        // their pending row (including a continued word), not adding a gap.
        if self.hanging_origin.is_some() {
            self.flush_preformatted();
            self.flush_paragraph_for_line_request();
            self.consume_hanging_first_line();
        } else {
            self.literal.end_line();
            self.flush_paragraph_for_line_request();
        }
        // .nf/.fi have an authored source position and close the current
        // terminal row. The next no-fill word cannot still consume a pending
        // definition HEAD row (man_term.c::print_man_node, NODE_NOFILL).
        self.formatter.settle_definition_head_rows();
    }

    /// A mdoc container's `NODE_LINE` is executed before its HEAD children.
    /// Those children can lack `NODE_LINE` themselves (notably Eo operands).
    pub(in crate::mandoc::blocks) fn begin_no_fill_source_line(&mut self) {
        if self.literal.starts_new_row(true) {
            if self.hanging_origin.is_some() {
                // man_term.c switches an HP BODY to its hanging origin after
                // the first no-fill child row. Keep that first row in its own
                // IR owner before entering the next executed source row.
                self.flush_preformatted();
            } else {
                self.literal.end_line();
            }
        }
    }

    pub(in crate::mandoc::blocks) fn set_spacing(&mut self, setting: &str) {
        self.paragraph
            .with_inline_builder(&mut self.formatter, |builder| {
                builder.set_spacing(setting);
            });
    }

    #[cfg(test)]
    pub(in crate::mandoc::blocks) fn push_inline(
        &mut self,
        nodes: Vec<Inline>,
        source: Option<mant_ir::SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
    ) {
        if nodes.is_empty() {
            if continues_line {
                self.paragraph.tighten_next_boundary(&mut self.formatter);
            }
            return;
        }
        self.push_inline_with(source, starts_indented_line, continues_line, |paragraph| {
            paragraph.append(nodes);
        });
    }

    /// Lower siblings into the same flow so zero-width controls and two-sided
    /// punctuation can act on both the preceding and following visible runs.
    pub(in crate::mandoc::blocks) fn push_inline_with(
        &mut self,
        source: Option<mant_ir::SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        self.push_source_inline_with(source, starts_indented_line, continues_line, false, append);
    }

    pub(in crate::mandoc::blocks) fn push_source_inline_with(
        &mut self,
        source: Option<mant_ir::SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
        ordinary_text: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        let visible_before = self.formatter.execution.visible_content_checkpoint();
        if self.column_uses_literal_output() {
            self.literal.append_filled_fragment(
                &mut self.formatter,
                source,
                starts_indented_line,
                continues_line,
                ordinary_text,
                append,
            );
        } else {
            self.paragraph.append(
                &mut self.formatter,
                source,
                starts_indented_line,
                continues_line,
                ordinary_text,
                append,
            );
        }
        let visible = self.formatter.definition_before_visible()
            && self
                .formatter
                .execution
                .has_visible_content_since(visible_before);
        if self.formatter.definition_before_visible()
            && let Some(boundary_checkpoint) = self.formatter.execution.take_leading_line_boundary()
        {
            // mdoc_term.c::termp_lk_pre() executes the description word
            // before its generated colon/target settle a pending break.
            // One handler can report both observations; the actual glyph
            // checkpoint preserves their order across the output wrapper.
            if visible && boundary_checkpoint != visible_before.0 {
                self.formatter.note_definition_visible();
            }
            self.formatter.note_definition_boundary();
            if self.formatter.definition_head_row_pending()
                && self.paragraph.consume_invisible_head_row()
            {
                self.formatter.consume_definition_head_row();
            }
            // Inline macro pre-handlers, such as An in AUTHORS, execute
            // term_newln() in the same source stream as block requests.
            self.formatter.settle_definition_head_rows();
        }
        if self.formatter.definition_before_visible() && visible {
            self.formatter.note_definition_visible();
        }
    }

    pub(in crate::mandoc::blocks) fn paragraph_is_empty(&self) -> bool {
        self.paragraph.is_empty()
    }

    pub(in crate::mandoc::blocks) fn hard_break(&mut self) {
        self.formatter.note_definition_boundary();
        self.paragraph.hard_break(&mut self.formatter);
        self.settle_definition_line_boundary();
    }

    /// The native request can close a row whose glyphs belong to HEAD.
    /// Its BODY owner consumes that same row once, before another word
    /// writes cells; request dispatch must not bypass this ownership seam.
    pub(super) fn settle_definition_line_boundary(&mut self) {
        if self.formatter.definition_head_row_pending()
            && self.paragraph.consume_invisible_head_row()
        {
            self.formatter.consume_definition_head_row();
        }
        self.formatter.settle_definition_head_rows();
    }

    /// Execute a native `term_newln()` request against the literal row owner.
    /// The request ends an active row even when its source was continued by
    /// \c; an IR paragraph break alone cannot release `LiteralFlow`'s join.
    pub(in crate::mandoc::blocks) fn end_literal_execution_line(&mut self) {
        self.literal.end_line();
    }

    pub(in crate::mandoc::blocks) fn tighten_next_boundary(&mut self) {
        self.paragraph.tighten_next_boundary(&mut self.formatter);
    }

    pub(in crate::mandoc::blocks) fn execute_no_fill_fragment(
        &mut self,
        source: Option<mant_ir::SourceSpan>,
        fallback: bool,
        finishes_row: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        if self.literal.execute_fragment(
            &mut self.formatter,
            source,
            fallback,
            finishes_row,
            append,
        ) {
            self.formatter.note_definition_visible();
        }
    }

    pub(in crate::mandoc::blocks) fn settle_no_fill_row(&mut self) {
        self.literal.settle_native_row(&mut self.formatter);
    }

    pub(in crate::mandoc::blocks) fn push_preformatted(
        &mut self,
        nodes: Vec<Inline>,
        source: Option<mant_ir::SourceSpan>,
        continues_line: bool,
        starts_line: bool,
        occupies_row: bool,
    ) {
        // The no-fill word has already executed against the same formatter.
        // Flushing an empty paragraph here would reset its live row state.
        if !self.formatter.no_fill || !self.paragraph.is_empty() {
            self.flush_paragraph();
        }
        if mant_ir::has_printable_character(&nodes) {
            self.formatter.note_definition_visible();
        }
        if self.hanging_origin.is_some() && self.literal.starts_new_row(starts_line) {
            // Materialize HP's temporary first row before adopting its permanent
            // body origin. A continued source line does not reach this boundary.
            self.flush_preformatted();
        }
        self.literal
            .append(nodes, source, continues_line, starts_line, occupies_row);
    }

    pub(in crate::mandoc::blocks) fn adopt_trailing_preformatted(&mut self) -> bool {
        if !self.literal.is_empty() {
            return false;
        }
        if !matches!(self.output.last(), Some(Block::Preformatted { .. })) {
            return false;
        }
        let Some(Block::Preformatted {
            children,
            layout,
            source,
            ..
        }) = self.output.pop()
        else {
            unreachable!("the last block was checked above")
        };
        self.literal.adopt(children, source, layout);
        true
    }

    /// A request's `pre_br` consumes the live row without changing IR owner.
    /// `ti` uses exactly this entry with or without numeric operands; source
    /// `NODE_LINE` and the request are separate `term_newln` events.
    pub(in crate::mandoc::blocks) fn pre_break_request(&mut self) {
        self.formatter.note_definition_boundary();
        if self.formatter.no_fill || self.column_uses_literal_output() {
            self.literal
                .with_inline_builder(&mut self.formatter, |builder| {
                    builder.control_line_break();
                });
        } else {
            self.paragraph
                .with_inline_builder(&mut self.formatter, |builder| {
                    builder.control_line_break();
                });
        }
        self.formatter.no_fill_inline.retire_consumed_cell();
        self.settle_definition_line_boundary();
    }

    pub(in crate::mandoc::blocks) fn no_break_formatter_flush(&mut self) {
        if self.formatter.no_fill || self.column_uses_literal_output() {
            self.literal.no_break_flush(&mut self.formatter);
        } else {
            self.paragraph.no_break_flush(&mut self.formatter);
        }
    }

    pub(in crate::mandoc::blocks) fn has_formatter_cell(&self) -> bool {
        self.formatter.execution.has_formatter_cell() || self.literal.has_formatter_column()
    }

    pub(in crate::mandoc::blocks) fn resolve_vertical_space(&mut self, rows: i32) -> u16 {
        self.formatter.note_definition_boundary();
        self.paragraph
            .resolve_vertical_space(&mut self.formatter, rows)
    }
}
