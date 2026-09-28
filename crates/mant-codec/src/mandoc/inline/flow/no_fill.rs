//! No-fill row bookkeeping on the same text executor used by filled output.
use super::{
    FormatterColumn, InlineBuilder, InlineExecutionState, PendingBoundary, TrailingOutput,
    WordEndBreak,
};
use crate::mandoc::formatter::FormatterState;
use mant_ir::Inline;

/// Only physical-row facts live here. Font, keep, word boundaries, \z,
/// \p, and source continuation belong to the formatter's execution state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct NoFillInlineState {
    active: bool,
    continued: bool,
    formatter_cell: NoFillFormatterCell,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoFillFormatterCell {
    Origin,
    Invisible,
    Visible,
}

impl NoFillInlineState {
    pub(in crate::mandoc) fn new() -> Self {
        Self {
            active: false,
            continued: false,
            formatter_cell: NoFillFormatterCell::Origin,
        }
    }

    pub(in crate::mandoc) fn finish_row(
        &mut self,
        execution: &mut InlineExecutionState,
        output: &mut Vec<Inline>,
    ) {
        if !self.active {
            return;
        }
        // CVS term_newln() flushes only an occupied cell. A bare BACKAFTER
        // request survives an empty row and can affect the first word after
        // .fi; a buffered glyph is committed before this row ends.
        if self.formatter_cell != NoFillFormatterCell::Origin {
            let output_start = output.len();
            execution.zero_advance.finish_into(output);
            if self.formatter_cell == NoFillFormatterCell::Invisible
                && output.len() == output_start
                && execution.word_end_break != WordEndBreak::Pending
            {
                output.push(Inline::Text {
                    value: String::new(),
                });
            }
        }
        if execution.word_end_break == WordEndBreak::Pending {
            if mant_ir::has_printable_character(output) {
                output.push(Inline::LineBreak);
            } else {
                output.push(Inline::Text {
                    value: String::new(),
                });
            }
        }
        let bare_armed = self.formatter_cell == NoFillFormatterCell::Origin
            && execution.zero_advance.take_armed();
        execution.reset_no_fill_row(bare_armed);
        // term_newln() closes a physical row without clearing TERMP_NONEWLINE.
        // Only the next term_word() consumes an active source continuation.
        self.formatter_cell = NoFillFormatterCell::Origin;
        self.active = false;
    }

    pub(in crate::mandoc) fn take_settled_row(
        &mut self,
        execution: &mut InlineExecutionState,
    ) -> Vec<Inline> {
        if !self.active {
            return Vec::new();
        }
        let mut output = Vec::new();
        self.finish_row(execution, &mut output);
        output
    }

    /// Flush an occupied cell under `TERMP_NOBREAK` without ending a `\c`
    /// continuation. A deferred \p can make an otherwise empty cell visible.
    pub(in crate::mandoc) fn take_no_break_cell(
        &mut self,
        execution: &mut InlineExecutionState,
    ) -> Vec<Inline> {
        if !self.active {
            return Vec::new();
        }
        let mut output = Vec::new();
        let realizes_word_end_break =
            execution.word_end_break == WordEndBreak::Pending && !self.continued;
        execution.zero_advance.finish_into(&mut output);
        if realizes_word_end_break {
            if output.is_empty() {
                output.push(Inline::Text {
                    value: String::new(),
                });
            }
            output.push(Inline::LineBreak);
        }
        execution.reset_no_fill_row(false);
        self.formatter_cell = NoFillFormatterCell::Origin;
        self.active = false;
        output
    }

    pub(in crate::mandoc) fn has_pending_formatter_cell(
        &self,
        execution: &InlineExecutionState,
    ) -> bool {
        self.active
            && (self.formatter_cell != NoFillFormatterCell::Origin
                || execution.zero_advance.has_buffered_glyph()
                || execution.word_end_break == WordEndBreak::Pending)
    }

    pub(in crate::mandoc) const fn continues_source_line(&self) -> bool {
        self.continued
    }
}

impl InlineExecutionState {
    /// A physical row resets word geometry while preserving document registers.
    /// Output-segment drains and Rust function returns do not call this method.
    fn reset_no_fill_row(&mut self, bare_armed: bool) {
        self.boundary = PendingBoundary::Ordinary;
        self.last_visible_character = None;
        self.has_printable_content = false;
        self.formatter_column = FormatterColumn::Origin;
        self.empty_word = false;
        self.trailing_output = TrailingOutput::None;
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
        self.pending_line_indent = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.leading_line_boundary = super::LeadingLineBoundary::None;
        self.zero_advance.reset_projection(bare_armed);
        self.zero_advance_joined = false;
        self.final_word_join = None;
        // The source continuation register survives term_newln() even though
        // the projected row geometry is reset.
    }
}

/// Source words and generated punctuation execute against the one formatter
/// state. The returned Vec is only an IR destination for this fragment.
pub(in crate::mandoc) fn lower_no_fill_fragment_with_formatter(
    formatter: &mut FormatterState,
    source_continuation_fallback: bool,
    finishes_row: bool,
    append: impl FnOnce(&mut InlineBuilder),
) -> (Vec<Inline>, bool, bool) {
    let continued = formatter.no_fill_inline.continued;
    let mut output = Vec::new();
    let (continues_line, formatter_cell_occupied, asserted_vertical_row) = formatter
        .with_output_builder(&mut output, |builder| {
            if continued {
                builder.continue_source_line(true);
            }
            append(builder);
            (
                builder.final_source_continuation_or(source_continuation_fallback),
                builder.has_formatter_cell(),
                builder.asserted_vertical_row(),
            )
        });
    // A generated word can realize a pending \p inside this fragment. The
    // fragment return is not term_newln(): the projected break belongs to the
    // active literal sink even when it is the last node produced here.
    let row = &mut formatter.no_fill_inline;
    row.active = true;
    row.continued = continues_line;
    if mant_ir::has_printable_character(&output) {
        row.formatter_cell = NoFillFormatterCell::Visible;
    } else if formatter_cell_occupied && row.formatter_cell == NoFillFormatterCell::Origin {
        row.formatter_cell = NoFillFormatterCell::Invisible;
    }
    if finishes_row && !continues_line {
        row.finish_row(&mut formatter.execution, &mut output);
    }
    (output, continues_line, asserted_vertical_row)
}
