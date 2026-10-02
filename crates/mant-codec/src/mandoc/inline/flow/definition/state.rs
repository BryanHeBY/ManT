use super::super::InlineBuilder;
use super::head_row::HeadRowState;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) struct DefinitionOutcome(u8);
// The booleans are independent native registers (fed flags and latch
// carries), not alternative states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Default)]
pub(in crate::mandoc::inline::flow) struct DefinitionFieldState {
    /// A roff break moves rmargin to the responsive page margin until a
    /// document node restores its saved geometry (mdoc_term.c:329,437-439).
    pub(in crate::mandoc::inline::flow) margin_override: Option<usize>,
    pub(in crate::mandoc::inline::flow) native_margin_units: Option<usize>,
    pub(in crate::mandoc::inline::flow) field_offset_units: usize,
    /// The current declared column origin. Its normal device padding is
    /// already represented by table placement; temporary node origins are
    /// not. This geometry is set at BODY pre, independently of IR owners.
    pub(in crate::mandoc::inline::flow) column_origin_units: Option<usize>,
    /// The normal BODY origin is already expressed by list/definition IR
    /// placement. Only later temporary geometry can add inline positioning.
    pub(in crate::mandoc::inline::flow) declared_body_origin_units: Option<usize>,
    pub(in crate::mandoc::inline::flow) projected_passes: usize,
    pub(in crate::mandoc::inline::flow) pending_indent: Option<usize>,
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
    /// The It HEAD post already ran (`mdoc_term.c:961-962`): NOBREAK, BRTRSP,
    /// BRIND, HANG, and trailspace are cleared even though the run-in kind
    /// (`LIST_diag`) flushed nothing at that point (939-945 keeps the shared
    /// buffer). Later flush decisions over the surviving cells must use the
    /// cleared flag set, not the HEAD's temporary field flags.
    pub(in crate::mandoc::inline::flow) head_flags_cleared: bool,
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
    /// Native word start, stable private owner marker, and content start.
    /// Markers retain ownership through style/link wrapping and compaction.
    pub(in crate::mandoc::inline::flow) field_word_anchors: Vec<super::super::NativeWordAnchor>,
    pub(in crate::mandoc::inline::flow) outcome: DefinitionOutcome,
    pub(in crate::mandoc::inline::flow) no_break: Option<NoBreakField>,
    // A positive term_vspace() ends the HANG device row. The next author
    // pre-handler can then start its field at the BODY margin.
    pub(in crate::mandoc::inline::flow) vertical_started_row: bool,
    pub(in crate::mandoc::inline::flow) hang_row: HangNativeRow,
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
    pub(in crate::mandoc::inline::flow) author_effect: super::super::AuthorBreakEffect,
}
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) enum PendingFieldGapOrigin {
    #[default]
    Other,
    CommittedFlush,
    /// Plain .mc's projected next-word separator. It represents a future
    /// `term_word` blank, not device minbl; an empty word can consume it.
    AutomaticWord,
}
/// The two persistent columns in `term.c::term_flushln()`, plus its unflushed
/// input field. Generated IR padding never enters this ledger.
// These are independent flags of one native field, not alternative states.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Default)]
pub(in crate::mandoc::inline::flow) struct HangNativeRow {
    pub(in crate::mandoc::inline::flow) viscol: usize,
    /// `term_field()` already advanced to the page's common left origin.
    /// A printed zero-width Unicode graph can establish it even when the
    /// relative viscol stays zero; NBRZW cannot (term.c:397-434).
    pub(in crate::mandoc::inline::flow) page_origin_printed: bool,
    /// Actual origin advances printed on this device row beyond declared
    /// column placement (`term.c::term_field()`). Node geometry restoration
    /// cannot undo them; only a real endline retires the receipt.
    pub(in crate::mandoc::inline::flow) unprojected_origin_units: usize,
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
    pub(in crate::mandoc::inline::flow) accepted_prefix_before_rejection: bool,
    pub(in crate::mandoc::inline::flow) provisional_trailing_break: Option<usize>,
    pub(in crate::mandoc::inline::flow) field_discarded: bool,
    pub(in crate::mandoc::inline::flow) field_last_unbreakable_width: usize,
    pub(in crate::mandoc::inline::flow) transition: HangRowTransition,
    pub(in crate::mandoc::inline::flow) suppress_next_auto_space: bool,
    pub(in crate::mandoc::inline::flow) margin_flush_seen: bool,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) enum HangRowTransition {
    #[default]
    Initial,
    Flushed,
    WordAfterFlush,
}

impl HangNativeRow {
    pub(in crate::mandoc::inline::flow) fn native_row_occupied(&self) -> bool {
        self.page_origin_printed || self.viscol > 0
    }

    /// Numeric `term_flushln()` padding with the page's common offset removed
    /// once. `term_ascii.c` sets defindent=5; `mdoc_term.c::termp_sh_pre()` adds
    /// it to offset. On a fresh row that offset covers minbl. After any
    /// actual graph prints, minbl is independent spacing on the same row.
    pub(super) fn padding_units(&self, offset_units: usize) -> usize {
        const PAGE_ORIGIN_UNITS: usize = 5 * 24;
        let unprinted_origin = if self.native_row_occupied() {
            0
        } else {
            PAGE_ORIGIN_UNITS
        };
        offset_units
            .saturating_add(unprinted_origin)
            .saturating_sub(self.viscol.saturating_mul(24))
            .max(self.minbl.saturating_mul(24))
            .saturating_sub(unprinted_origin)
    }

    pub(in crate::mandoc::inline::flow) fn word(
        &mut self,
        separator: usize,
        width: usize,
        trailing_spaces: usize,
        printable: bool,
    ) {
        if separator > 0 && self.field_pending_word_end_break {
            self.field_unproven_break = true;
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
        self.accepted_prefix_before_rejection = false;
        self.provisional_trailing_break = None;
        self.field_discarded = false;
        self.field_last_unbreakable_width = 0;
        self.minbl = trailspace;
        self.transition = HangRowTransition::Flushed;
    }

    pub(in crate::mandoc::inline::flow) fn endline(&mut self) {
        self.viscol = 0;
        self.page_origin_printed = false;
        self.unprojected_origin_units = 0;
        self.minbl = 0;
        self.field_width = 0;
        self.trailing_breakable = 0;
        self.field_printable = false;
        self.field_breakable = false;
        self.field_discretionary_break = false;
        self.field_unproven_break = false;
        self.field_pending_word_end_break = false;
        self.accepted_prefix_before_rejection = false;
        self.provisional_trailing_break = None;
        self.field_discarded = false;
        self.field_last_unbreakable_width = 0;
        self.transition = HangRowTransition::Flushed;
    }
}
impl InlineBuilder {
    pub(in crate::mandoc::inline::flow) fn projected_native_field_padding(&self) -> usize {
        self.execution
            .definition
            .as_ref()
            .and_then(|state| state.no_break)
            .map_or(0, |field| field.separator_cells.saturating_sub(1))
    }
    pub(in crate::mandoc::inline::flow) fn definition_state_mut(
        &mut self,
    ) -> &mut DefinitionFieldState {
        let tabs = self.execution.tab_stops.clone();
        let detached_row = self.execution.detached_device_row.take();
        let state = self
            .execution
            .definition
            .get_or_insert_with(DefinitionFieldState::default);
        if let Some(row) = detached_row {
            state.hang_row.viscol = row.viscol;
            state.hang_row.minbl = row.minbl;
            state.hang_row.page_origin_printed = row.page_origin_printed;
        }
        if state.field_buffer.configure_tabs(&tabs) {
            state.projected_passes = 0;
        }
        state
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
    /// Actual native pad/break flags retained after `.mc` clears NOBREAK.
    /// A layout style cannot reconstruct these: a Column is wrappable,
    /// but never introduces a TAG's BRIND or BRTRSP.
    pub(super) flags: super::super::native_field::FieldFlags,
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
