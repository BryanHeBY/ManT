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
        // Resolve the complete active native unit against the actual literal
        // owner before its cells retire. A fragment return is not a flush:
        // earlier TEXT and delayed glyph owners remain reachable here.
        // CVS term_newln() flushes only an occupied cell. A bare BACKAFTER
        // request survives an empty row and can affect the first word after
        // .fi; a buffered glyph is committed before this row ends.
        let mut accepted_invisible_row = false;
        if self.formatter_cell != NoFillFormatterCell::Origin {
            let output_start = output.len();
            execution.zero_advance.finish_into(output);
            if self.formatter_cell == NoFillFormatterCell::Invisible
                && output.len() == output_start
                && execution.word_end_break != WordEndBreak::Pending
            {
                accepted_invisible_row = true;
            }
        }
        let represented_marker_row = execution.flush_unit.has_projected_rows();
        let rejected_row = InlineBuilder::retire_plain_flush_unit_at(
            execution,
            output,
            super::output::CompletedRowOrigin::LiteralText,
        );
        if rejected_row {
            execution.word_end_break = WordEndBreak::Clear;
        }
        if rejected_row || (accepted_invisible_row && !InlineBuilder::has_literal_tail_row(output))
        {
            // nbr=0 ends the rejected pass's own physical row, independently
            // of any earlier accepted marker pass (term.c:143-146,250-253).
            // An accepted invisible row likewise belongs to the whole
            // consumed buffer, not its last empty word's accepted scalar
            // interval. Append its witness after receipt filtering.
            output.push(Inline::Text {
                value: String::new(),
            });
        }
        if execution.word_end_break == WordEndBreak::Pending
            && !(represented_marker_row && super::output::ends_with_executed_line_break(output))
        {
            if mant_ir::has_printable_character(output) {
                output.push(Inline::line_break());
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

    /// The actual native no-break consumer already settled the live output.
    /// Its buffer retirement does not end the physically continued row.
    pub(in crate::mandoc) fn retire_consumed_cell(&mut self) {
        self.formatter_cell = NoFillFormatterCell::Origin;
        self.active = false;
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
        self.boundary = PendingBoundary::Tight;
        self.last_visible_character = None;
        self.has_printable_content = false;
        self.formatter_column = FormatterColumn::Origin;
        self.empty_word = false;
        self.trailing_output = TrailingOutput::None;
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
        self.pending_line_indent = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.wipe_remainder = false;
        self.row_zero_graph = false;
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
    output: &mut Vec<Inline>,
    source_continuation_fallback: bool,
    finishes_row: bool,
    append: impl FnOnce(&mut InlineBuilder),
) -> (bool, bool) {
    let continued = formatter.no_fill_inline.continued;
    // A no-fill BODY word's NODE_LINE gate reads TERMP_NONEWLINE as it
    // stands at this fragment's entry (mdoc_term.c:314-317), before the
    // word's own term_word() clears it (term.c:588).
    formatter.note_definition_source_line();
    let (
        continues_line,
        formatter_cell_occupied,
        current_row_visible,
        produced_formatter_cell,
        asserted_vertical_row,
    ) = formatter.with_output_builder(output, |builder| {
        if continued {
            builder.continue_source_line(true);
        }
        // mdoc_term.c:314-318: the NODE_NOFILL subtree prints under
        // TERMP_BRNEVER.
        let was_no_fill_word = builder.execution.no_fill_word_active;
        builder.execution.no_fill_word_active = true;
        append(builder);
        builder.execution.no_fill_word_active = was_no_fill_word;
        (
            builder.final_source_continuation_or(source_continuation_fallback),
            builder.has_formatter_cell(),
            builder.execution.has_printable_content,
            builder.produced_formatter_cell(),
            builder.asserted_vertical_row(),
        )
    });
    // LiteralFlow owns the asserted row for this fragment. Its execution
    // fact must not be replayed if the formatter later returns to filled IR.
    formatter.execution.completed_vertical_rows = 0;
    // A generated word can realize a pending \p inside this fragment. The
    // fragment return is not term_newln(): the projected break belongs to the
    // active literal sink even when it is the last node produced here.
    let row = &mut formatter.no_fill_inline;
    row.active = true;
    row.continued = continues_line;
    // The output owner can retain earlier completed rows. They cannot make
    // a new bare BACKAFTER word an occupied native row: term_newln() only
    // flushes the current native buffer (term.c:475-481).
    if formatter_cell_occupied && current_row_visible {
        row.formatter_cell = NoFillFormatterCell::Visible;
    } else if formatter_cell_occupied
        && produced_formatter_cell
        && row.formatter_cell == NoFillFormatterCell::Origin
    {
        row.formatter_cell = NoFillFormatterCell::Invisible;
    }
    if finishes_row && !continues_line {
        row.finish_row(&mut formatter.execution, output);
    }
    (continues_line, asserted_vertical_row)
}
