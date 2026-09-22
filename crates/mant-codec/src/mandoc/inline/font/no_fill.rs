use super::{
    FontState, Inline, Node, ZeroAdvanceState, append_inline_nodes, builder_with_zero_advance,
};
use crate::mandoc::inline::draft::has_printable_character;

/// Lower one executed no-fill input row.
///
/// A deferred `\p` at the end of the formatter word is settled by the
/// physical input-row boundary, not emitted as a second inline break.  Any
/// completed `\z` glyph still belongs to the row and must be committed before
/// the outer literal flow appends that boundary.
pub(in crate::mandoc) struct NoFillInlineState {
    zero_advance: ZeroAdvanceState,
    pending_word_end_break: bool,
    continued: bool,
    formatter_cell: NoFillFormatterCell,
}

#[derive(Clone, Copy, Eq, PartialEq)]
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
            if has_printable_character(output) {
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
        self.continued = false;
        self.formatter_cell = NoFillFormatterCell::Origin;
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
}

pub(in crate::mandoc) fn lower_no_fill_line_with_font_state(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
    inline_state: &mut NoFillInlineState,
    source_continuation_fallback: bool,
) -> (Vec<Inline>, bool) {
    let mut builder = builder_with_zero_advance(spacing, *state, &mut inline_state.zero_advance);
    if inline_state.continued {
        builder.continue_source_line(true);
        builder.tighten_next_boundary();
    }
    if inline_state.pending_word_end_break {
        builder.request_word_end_break();
    }
    append_inline_nodes(&mut builder, nodes, default_name);
    *state = builder.font;
    let (mut output, execution) = builder.finish_preserving_execution();
    let continues_line = execution
        .source_continuation
        .unwrap_or(source_continuation_fallback);
    inline_state.zero_advance = execution.zero_advance;
    inline_state.pending_word_end_break = execution.word_end_break;
    inline_state.continued = continues_line;
    if has_printable_character(&output) {
        inline_state.formatter_cell = NoFillFormatterCell::Visible;
    } else if execution.formatter_cell_occupied
        && inline_state.formatter_cell == NoFillFormatterCell::Origin
    {
        inline_state.formatter_cell = NoFillFormatterCell::Invisible;
    }
    if !continues_line {
        inline_state.finish_row(&mut output);
    }
    (output, continues_line)
}
