use super::super::flow::{KeepState, PendingBoundary, WordBoundaryState};
use super::{FontState, Inline, InlineBuilder, ZeroAdvanceState, builder_with_zero_advance};
use crate::mandoc::containers::ScopePostState;

/// Lower one executed no-fill input row.
///
/// A deferred `\p` at the end of the formatter word is settled by the
/// physical input-row boundary, not emitted as a second inline break.  Any
/// completed `\z` glyph still belongs to the row and must be committed before
/// the outer literal flow appends that boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct NoFillInlineState {
    zero_advance: ZeroAdvanceState,
    pending_word_end_break: bool,
    continued: bool,
    formatter_cell: NoFillFormatterCell,
    boundary: PendingBoundary,
    word_boundary: WordBoundaryState,
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
            zero_advance: ZeroAdvanceState::new(),
            pending_word_end_break: false,
            continued: false,
            formatter_cell: NoFillFormatterCell::Origin,
            boundary: PendingBoundary::Ordinary,
            word_boundary: WordBoundaryState::new(),
        }
    }

    pub(in crate::mandoc) fn finish_row(&mut self, output: &mut Vec<Inline>) {
        // A physical no-fill row flushes an occupied formatter cell, but CVS
        // leaves a bare BACKAFTER request alive when no glyph was ever
        // buffered.  It can therefore affect the first word after `.fi`.
        if self.formatter_cell != NoFillFormatterCell::Origin {
            let output_start = output.len();
            self.zero_advance.finish_into(output);
            if self.formatter_cell == NoFillFormatterCell::Invisible
                && output.len() == output_start
                && !self.pending_word_end_break
            {
                // `\&` advances the native cell without producing a glyph.
                // A real line flush still owns that empty row, so retain the
                // same explicit sentinel used by an authored empty word.
                output.push(Inline::Text {
                    value: String::new(),
                });
            }
        }
        if self.pending_word_end_break {
            if mant_ir::has_printable_character(output) {
                output.push(Inline::LineBreak);
            } else {
                // A control-only `\p` still occupies one native no-fill row.
                // Use the same explicit empty-row sentinel as an empty TEXT
                // word so LiteralFlow cannot trim it as a formatter-only tail.
                output.push(Inline::Text {
                    value: String::new(),
                });
            }
        }
        self.pending_word_end_break = false;
        self.continued = false;
        self.formatter_cell = NoFillFormatterCell::Origin;
        self.boundary = PendingBoundary::Ordinary;
        self.word_boundary = WordBoundaryState::new();
    }

    pub(in crate::mandoc) fn take_settled_row(&mut self) -> Vec<Inline> {
        let mut output = Vec::new();
        self.finish_row(&mut output);
        output
    }

    /// Flush the current terminal cell under `TERMP_NOBREAK`.
    /// A buffered `\p` makes the cell active but does not emit a visual row
    /// boundary during this flush; a completed `\zX` glyph is materialized.
    pub(in crate::mandoc) fn take_no_break_cell(&mut self) -> Vec<Inline> {
        let mut output = Vec::new();
        let realizes_word_end_break = self.pending_word_end_break && !self.continued;
        self.zero_advance.finish_into(&mut output);
        if realizes_word_end_break {
            if output.is_empty() {
                output.push(Inline::Text {
                    value: String::new(),
                });
            }
            output.push(Inline::LineBreak);
        }
        self.pending_word_end_break = false;
        // roff_term_pre_mc() clears NOSPACE for the next formatter word but
        // preserves independently active TERMP_NONEWLINE from authored \c.
        self.formatter_cell = NoFillFormatterCell::Origin;
        self.boundary = PendingBoundary::Ordinary;
        self.word_boundary = WordBoundaryState::new();
        output
    }

    pub(in crate::mandoc) const fn has_pending_formatter_cell(&self) -> bool {
        !matches!(self.formatter_cell, NoFillFormatterCell::Origin)
            || self.zero_advance.has_buffered_glyph()
            || self.pending_word_end_break
    }

    pub(in crate::mandoc) fn inherit_zero_advance_armed(&mut self, armed: bool) {
        self.zero_advance.inherit_armed(armed);
    }

    pub(in crate::mandoc) fn take_bare_zero_advance_armed(&mut self) -> bool {
        if self.formatter_cell != NoFillFormatterCell::Origin
            || self.zero_advance.has_buffered_glyph()
        {
            false
        } else {
            self.zero_advance.take_armed()
        }
    }

    pub(in crate::mandoc) const fn continues_source_line(&self) -> bool {
        self.continued
    }
}

/// Execute generated container content in the same no-fill formatter state
/// as ordinary source nodes. Generated punctuation does not itself finish an
/// authored input row; source operands do.
pub(in crate::mandoc) fn lower_no_fill_fragment_with_font_state(
    spacing: bool,
    registers: NoFillRegisters<'_>,
    scope_posts: &ScopePostState,
    source_continuation_fallback: bool,
    finishes_row: bool,
    append: impl FnOnce(&mut InlineBuilder),
) -> (Vec<Inline>, bool) {
    let NoFillRegisters {
        font: state,
        row: inline_state,
        keep,
    } = registers;
    let mut builder = builder_with_zero_advance(spacing, *state, &mut inline_state.zero_advance);
    builder.scope_posts = scope_posts.clone();
    builder.inherit_word_boundary_state(inline_state.word_boundary);
    builder.inherit_keep_state(*keep);
    builder.inherit_boundary_state(inline_state.boundary);
    if inline_state.continued {
        builder.continue_source_line(true);
        builder.inherit_boundary_state(inline_state.boundary);
    }
    if inline_state.pending_word_end_break {
        builder.request_word_end_break();
    }
    append(&mut builder);
    *keep = builder.keep_state();
    inline_state.word_boundary = builder.word_boundary_state();
    inline_state.boundary = builder.boundary_state();
    *state = builder.font;
    let (mut output, execution) = builder.finish_preserving_execution();
    let continues_line = execution
        .source_continuation
        .unwrap_or(source_continuation_fallback);
    inline_state.zero_advance = execution.zero_advance;
    inline_state.pending_word_end_break = execution.word_end_break;
    inline_state.continued = continues_line;
    if mant_ir::has_printable_character(&output) {
        inline_state.formatter_cell = NoFillFormatterCell::Visible;
    } else if execution.formatter_cell_occupied
        && inline_state.formatter_cell == NoFillFormatterCell::Origin
    {
        inline_state.formatter_cell = NoFillFormatterCell::Invisible;
    }
    if finishes_row && !continues_line {
        inline_state.finish_row(&mut output);
    }
    (output, continues_line)
}

pub(in crate::mandoc) struct NoFillRegisters<'a> {
    pub(in crate::mandoc) font: &'a mut FontState,
    pub(in crate::mandoc) row: &'a mut NoFillInlineState,
    pub(in crate::mandoc) keep: &'a mut KeepState,
}
