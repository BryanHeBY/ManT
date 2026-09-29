use super::super::InlineBuilder;
use super::head_row::HeadRowState;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) struct DefinitionOutcome(u8);
// The booleans are independent native registers (fed flags and latch
// carries), not alternative states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Default)]
pub(in crate::mandoc::inline::flow) struct DefinitionFieldState {
    pub(in crate::mandoc::inline::flow) pending_indent: Option<usize>,
    /// Latched: some pass rejected and the buffer wipe (term.c:144-146
    /// with 235) discarded its suffix. Unlike `hang_row.field_discarded`
    /// this fact survives later flushes; a run-in BODY reads it to know
    /// its first text shared the wiped buffer.
    pub(in crate::mandoc::inline::flow) suffix_discarded_seen: bool,
    /// True only for a NOBREAK field carried across the HEAD/BODY ownership
    /// split (`PreservedDefinitionField`): its rejection is decided by the
    /// BODY words and must be committed at the item post drain
    /// (mdoc_term.c:939-945), the only `term_newln()` this field ever sees.
    pub(in crate::mandoc::inline::flow) run_in_continuation: bool,
    /// A line-break request (`.sp`/`.br`, and `.nf`/`.fi` through the same
    /// `roff_term_pre_br()` dispatch, roff_term.c:45-58, 71-78) ran inside
    /// this field and cleared `TERMP_NOBREAK`. Roff request nodes return
    /// before the flag save/restore (mdoc_term.c:394-396), so the clear
    /// outlives the request; only the item post resets it
    /// (mdoc_term.c:961-962). While set, every pass of the field's
    /// `term_fill()` targets `vfield` instead of the page margin
    /// (term.c:134-136): head words wrap at the field's own width.
    pub(in crate::mandoc::inline::flow) no_break_cleared: bool,
    /// Row geometry a control request left behind: the indent a break's
    /// row carries and the jump a word emits onto the open HANG row, with
    /// the print-deferral lifetime upstream gives them.
    pub(in crate::mandoc::inline::flow) row: HeadRowState,
    /// The head field's content capacity `rmargin - offset`, latched for
    /// `no_break_cleared` sessions: the request that cleared NOBREAK may
    /// also degrade the author effect to `Line`, but the remaining head
    /// words still fill against this bound (term.c:134-136).
    pub(in crate::mandoc::inline::flow) cleared_field_capacity_columns: u16,
    /// The native input buffer of this field, fed in source order at each
    /// formatter word (term.c's `tcol->buf`); pass decisions at flush time
    /// come from it instead of streaming heuristics.
    pub(in crate::mandoc::inline::flow) field_buffer: super::super::field_buffer::FieldBuffer,
    /// (cell index, IR node count) at each fed word's start, mapping
    /// buffer positions to output ranges for wipes.
    pub(in crate::mandoc::inline::flow) field_word_anchors: Vec<(usize, usize)>,
    /// The still-pending `\z` glyph has already entered the field buffer;
    /// it must not re-enter while IR resolution lags behind the native
    /// `encode1()` write.
    pub(in crate::mandoc::inline::flow) pending_glyph_fed: bool,
    /// A word's trailing `\p` deferred into the field (no in-operand
    /// blank): its `'\n'` cell still has to enter the native buffer.
    pub(in crate::mandoc::inline::flow) trailing_marker_unfed: bool,
    pub(in crate::mandoc::inline::flow) outcome: DefinitionOutcome,
    pub(in crate::mandoc::inline::flow) no_break: Option<NoBreakField>,
    // A positive term_vspace() ends the HANG device row. The next author
    // pre-handler can then start its field at the BODY margin.
    pub(in crate::mandoc::inline::flow) vertical_started_row: bool,
    pub(in crate::mandoc::inline::flow) hang_row: HangNativeRow,
    pub(in crate::mandoc::inline::flow) pending_gap_origin: PendingFieldGapOrigin,
}
/// A definition field that stays open across the HEAD/BODY ownership split.
///
/// CVS `mdoc_term.c::termp_it_pre()` configures a `-diag` NOBREAK field
/// before the HEAD prints and keeps it active until the item's BODY post
/// runs `term_newln()`. When the parsed HEAD degenerates to plain text
/// (for example `.It Xo`, where the extension block closes outside the
/// head), the marker-driven field rules must therefore continue in the
/// BODY session: carry the live field state and its configuration across
/// the `PreservedInlineState` seam.
pub(in crate::mandoc) struct PreservedDefinitionField {
    pub(in crate::mandoc::inline::flow) state: DefinitionFieldState,
    pub(in crate::mandoc::inline::flow) gap_cells: u8,
    pub(in crate::mandoc::inline::flow) body_width_columns: u16,
    pub(in crate::mandoc::inline::flow) field_width_columns: u16,
    pub(in crate::mandoc::inline::flow) flags: super::super::native_field::FieldFlags,
}
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) enum PendingFieldGapOrigin {
    #[default]
    Other,
    SourceLine,
}
/// The two persistent columns in `term.c::term_flushln()`, plus its unflushed
/// input field. Generated IR padding never enters this ledger.
// These are independent flags of one native field, not alternative states.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Default)]
pub(in crate::mandoc::inline::flow) struct HangNativeRow {
    pub(in crate::mandoc::inline::flow) viscol: usize,
    pub(in crate::mandoc::inline::flow) minbl: usize,
    // BRIND changes the offset while nested HEAD children execute. The
    // enclosing HEAD restores its old offset before its final post flush.
    pub(in crate::mandoc::inline::flow) field_offset: usize,
    pub(in crate::mandoc::inline::flow) field_width: usize,
    pub(in crate::mandoc::inline::flow) trailing_breakable: usize,
    pub(in crate::mandoc::inline::flow) field_printable: bool,
    // A field with a breakable boundary can be redistributed by term_fill().
    // The cumulative width is then not proof of its final device column.
    pub(in crate::mandoc::inline::flow) field_breakable: bool,
    pub(in crate::mandoc::inline::flow) field_discretionary_break: bool,
    pub(in crate::mandoc::inline::flow) field_unproven_break: bool,
    pub(in crate::mandoc::inline::flow) field_pending_word_end_break: bool,
    // term_fill() returns nbr=0 if \p precedes the field's first graph and
    // the next formatter word adds a separator. No part of that field prints.
    pub(in crate::mandoc::inline::flow) field_native_graph: bool,
    // A \p followed by a blank before this word supplied a graph. The
    // pending field must retain only the prefix accepted by term_fill().
    pub(in crate::mandoc::inline::flow) field_break_before_graph_prefix: Option<usize>,
    pub(in crate::mandoc::inline::flow) accepted_prefix_before_rejection: bool,
    pub(in crate::mandoc::inline::flow) last_word_started_with_separator: bool,
    pub(in crate::mandoc::inline::flow) last_word_supplied_graph: bool,
    pub(in crate::mandoc::inline::flow) consumed_pending_word_end_break: bool,
    pub(in crate::mandoc::inline::flow) provisional_trailing_break: Option<usize>,
    pub(in crate::mandoc::inline::flow) field_discarded: bool,
    pub(in crate::mandoc::inline::flow) field_last_unbreakable_width: usize,
    pub(in crate::mandoc::inline::flow) transition: HangRowTransition,
    pub(in crate::mandoc::inline::flow) suppress_next_auto_space: bool,
    pub(in crate::mandoc::inline::flow) margin_flush_seen: bool,
    // Snapshot taken when a source word's decode begins: a \p armed by an
    // EARLIER word starts this word's term_fill() pass at a blank (term.c
    // resumes right after that word's '\n' buffer cell). When that armed
    // pass had accepted no graph yet, the very first blank rejects the pass
    // (nbr=0, term.c:293-295) and this whole word is unprinted buffer.
    pub(in crate::mandoc::inline::flow) field_armed_at_word_start: bool,
    pub(in crate::mandoc::inline::flow) field_native_graph_at_word_start: bool,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) enum HangRowTransition {
    #[default]
    Initial,
    Flushed,
    WordAfterFlush,
}

impl HangNativeRow {
    pub(in crate::mandoc::inline::flow) fn word(
        &mut self,
        separator: usize,
        width: usize,
        trailing_spaces: usize,
        printable: bool,
    ) {
        self.last_word_started_with_separator = separator > 0;
        self.last_word_supplied_graph = printable;
        self.consumed_pending_word_end_break = separator > 0 && self.field_pending_word_end_break;
        if separator > 0 && self.field_pending_word_end_break {
            self.field_unproven_break = true;
            self.field_discarded |= !self.field_native_graph;
            // term_fill() starts again after its accepted prefix; a graph
            // from that prefix cannot make the following field printable.
            self.field_native_graph = false;
        }
        if separator > 0 {
            self.field_pending_word_end_break = false;
        }
        self.field_breakable |=
            self.field_printable && self.trailing_breakable.saturating_add(separator) > 0;
        self.trailing_breakable = self.trailing_breakable.saturating_add(separator);
        if printable {
            self.field_width = self
                .field_width
                .saturating_add(self.trailing_breakable)
                .saturating_add(width);
            self.trailing_breakable = 0;
            self.field_printable = true;
            // A term_fill() pass that already returned nbr=0 cannot make
            // later bytes in that rejected field into an accepted prefix.
            self.field_native_graph = !self.field_discarded;
            // Callers with a pending glyph have one indivisible formatter
            // word. Source words refine this to their final component.
            self.field_last_unbreakable_width = width;
        }
        self.trailing_breakable = self.trailing_breakable.saturating_add(trailing_spaces);
        if self.transition == HangRowTransition::Flushed {
            self.transition = HangRowTransition::WordAfterFlush;
        }
        self.suppress_next_auto_space = false;
    }

    pub(super) fn pending_glyph(&mut self, width: usize) {
        let extends_last_word = self.field_printable && self.trailing_breakable == 0;
        let previous_width = self.field_last_unbreakable_width;
        self.word(0, width, 0, true);
        if extends_last_word {
            self.field_last_unbreakable_width = previous_width.saturating_add(width);
        }
    }

    pub(super) fn flush(&mut self, trailspace: usize) {
        // term_fill() drops trailing ordinary spaces. term_field() advances
        // vbl only when there is a printable cell, including a fixed blank.
        if self.field_printable && !self.field_discarded {
            self.viscol = self
                .viscol
                .saturating_add(self.minbl)
                .max(self.field_offset)
                .saturating_add(self.field_width);
        }
        self.field_width = 0;
        self.trailing_breakable = 0;
        self.field_printable = false;
        self.field_breakable = false;
        self.field_discretionary_break = false;
        self.field_unproven_break = false;
        self.field_pending_word_end_break = false;
        self.field_native_graph = false;
        self.field_break_before_graph_prefix = None;
        self.accepted_prefix_before_rejection = false;
        self.last_word_started_with_separator = false;
        self.last_word_supplied_graph = false;
        self.consumed_pending_word_end_break = false;
        self.provisional_trailing_break = None;
        self.field_discarded = false;
        self.field_last_unbreakable_width = 0;
        self.field_armed_at_word_start = false;
        self.field_native_graph_at_word_start = false;
        self.minbl = trailspace;
        self.transition = HangRowTransition::Flushed;
    }

    pub(in crate::mandoc::inline::flow) fn endline(&mut self) {
        self.viscol = 0;
        self.minbl = 0;
        self.field_width = 0;
        self.trailing_breakable = 0;
        self.field_printable = false;
        self.field_breakable = false;
        self.field_discretionary_break = false;
        self.field_unproven_break = false;
        self.field_pending_word_end_break = false;
        self.field_native_graph = false;
        self.field_break_before_graph_prefix = None;
        self.accepted_prefix_before_rejection = false;
        self.last_word_started_with_separator = false;
        self.last_word_supplied_graph = false;
        self.consumed_pending_word_end_break = false;
        self.provisional_trailing_break = None;
        self.field_discarded = false;
        self.field_last_unbreakable_width = 0;
        self.field_armed_at_word_start = false;
        self.field_native_graph_at_word_start = false;
        self.transition = HangRowTransition::Flushed;
    }

    pub(super) fn final_column(&self) -> usize {
        if self.field_printable && !self.field_discarded {
            self.viscol
                .saturating_add(self.minbl)
                .saturating_add(self.field_width)
        } else {
            // An unprinted field does not erase the previous field's
            // trailspace. term_flushln() keeps minbl for the BODY word even
            // when term_fill() returns nbr=0 for the current field.
            self.viscol
                .saturating_add(self.minbl)
                .max(self.field_offset)
        }
    }
}
impl InlineBuilder {
    pub(in crate::mandoc::inline::flow) fn definition_state_mut(
        &mut self,
    ) -> &mut DefinitionFieldState {
        self.definition
            .get_or_insert_with(DefinitionFieldState::default)
    }

    pub(in crate::mandoc::inline::flow) fn pending_definition_indent(&self) -> Option<usize> {
        self.definition
            .as_ref()
            .and_then(|state| state.pending_indent)
    }

    pub(in crate::mandoc::inline::flow) fn set_pending_definition_indent(
        &mut self,
        indent: Option<usize>,
    ) {
        self.definition_state_mut().pending_indent = indent;
    }
}
impl DefinitionOutcome {
    const FIELD_EXITED: u8 = 1;
    const BODY_GAP_CONSUMED: u8 = 2;
    const AUTHOR_RESTARTED: u8 = 4;

    /// An author-split row end that restarts the field: the next author
    /// word is the restarted field's first word, so the head did not exit
    /// (mdoc_term.c:1084-1085 keeps NOBREAK and BRIND until the item post).
    pub(in crate::mandoc::inline::flow) fn mark_field_restarted(&mut self) {
        self.0 |= Self::AUTHOR_RESTARTED;
    }

    pub(in crate::mandoc::inline::flow) fn is_field_restarted(self) -> bool {
        self.0 & Self::AUTHOR_RESTARTED != 0
    }

    pub(in crate::mandoc::inline::flow) fn mark_field_exited(&mut self) {
        self.0 |= Self::FIELD_EXITED;
    }

    pub(in crate::mandoc::inline::flow) fn mark_body_gap_consumed(&mut self) {
        self.0 |= Self::BODY_GAP_CONSUMED;
    }

    pub(in crate::mandoc::inline::flow) fn clear_body_gap_consumed(&mut self) {
        self.0 &= !Self::BODY_GAP_CONSUMED;
    }

    pub(in crate::mandoc::inline::flow) const fn field_exited(self) -> bool {
        self.0 & Self::FIELD_EXITED != 0
    }

    pub(in crate::mandoc::inline::flow) const fn body_gap_consumed(self) -> bool {
        self.0 & Self::BODY_GAP_CONSUMED != 0
    }
}
/// Native field state left behind by `roff_term_pre_mc()`.
///
/// CVS clears `NOBREAK` and `NOSPACE` after flushing, but deliberately keeps
/// `BRIND`, `HANG`, and the list field geometry for a following request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) struct NoBreakField {
    pub(super) output_end_before_separator: usize,
    pub(super) resumed_output_start: usize,
    pub(super) resumed_execution_epoch: u64,
    pub(super) field_width: usize,
    pub(super) body_width: usize,
    /// `rmargin - offset` of the head field (term.c:124-125 uses it as the
    /// pass target after a request cleared `TERMP_NOBREAK`).
    pub(super) field_capacity_columns: u16,
    pub(super) trailspace_cells: usize,
    pub(super) separator_cells: usize,
    pub(super) style: DefinitionFieldStyle,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DefinitionFieldStyle {
    Tag,
    Hang,
}
impl InlineBuilder {
    /// Record that a word's trailing `\p` stays deferred in the field: the
    /// decoder never emits its IR break, but `term.c::bufferc()` wrote the
    /// `'\\n'` cell (term.c:657-658). The word accounting consumes it.
    pub(in crate::mandoc) fn note_field_trailing_marker(&mut self) {
        if let Some(definition) = &mut self.execution.definition {
            definition.trailing_marker_unfed = true;
        }
    }
}
