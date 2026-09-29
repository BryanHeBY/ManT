//! The man(7)/mdoc(7) definition-list row machine in one place: TAG/HANG
//! field execution state (`term.c::term_flushln()` columns) and the
//! `InlineBuilder` methods that consume definition HEAD/BODY source rows.
//! Siblings only read this state through `InlineExecutionState::definition`;
//! the row invariants themselves stay private to this module.

use super::native_field::{FieldFlag, FieldFlags, row_continues};
use super::{
    AuthorBreakEffect, FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput,
    WordEndBreak, has_printable_character, last_visible_character, trim_trailing_breakable_spaces,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct DefinitionOutcome(u8);

/// Row geometry a control request left behind in a definition head.
///
/// Upstream, the request's `roff_term_pre_br()` moves the device row
/// origin (`offset <- rmargin`, roff_term.c:73-75) and the roff node
/// escapes the document save/restore (mdoc_term.c:393-397), so the
/// geometry lives until the next document node boundary restores the
/// authored values (mdoc_term.c:329-330, 437-439). A HANG row stays open
/// through the request (`roff_term.c:76`), so the next word prints
/// through `vbl = offset - viscol` (term.c:113-114) - a horizontal jump -
/// and upstream defers that word's flush until either a later input-row
/// event prints it (the jump stands) or the head close does, after the
/// restore (the jump collapses). This machine models that lifetime
/// instead of scanning output nodes after the fact.
#[derive(Clone, Debug, Default)]
pub(super) struct HeadRowState {
    /// Indent (columns relative to the field origin) the row a break
    /// starts carries - the tag path's row origin after the request.
    pub(super) indent_columns: u16,
    /// Row offset (`rmargin` relative to the field origin) a request
    /// armed for the open HANG row: the fill at print time is
    /// `offset - viscol` (term.c:113-114), computed when the carrying
    /// word emits, because viscol keeps advancing until then.
    armed_offset_columns: u16,
    /// An emitted jump awaiting its word's print timing.
    pending: Option<PendingRowJump>,
}

#[derive(Clone, Copy, Debug)]
struct PendingRowJump {
    /// Node index of the emitted fill text.
    node: usize,
    /// The word carrying the fill finished appending.
    word_closed: bool,
}

impl HeadRowState {
    /// Arm the request-moved row offset for the next word on the open
    /// row.
    pub(super) fn arm_jump(&mut self, offset_columns: u16) {
        self.armed_offset_columns = offset_columns;
    }

    /// Emit the armed jump as `offset - viscol` fill at `node`, carried
    /// by the word currently appending.
    pub(super) fn emit_armed(&mut self, node: usize, viscol: u16) -> u16 {
        let columns = self.armed_offset_columns.saturating_sub(viscol);
        self.armed_offset_columns = 0;
        if columns > 0 {
            self.pending = Some(PendingRowJump {
                node,
                word_closed: false,
            });
        }
        columns
    }

    /// The word carrying an emitted jump finished appending.
    pub(super) fn close_word(&mut self) {
        if let Some(pending) = &mut self.pending {
            pending.word_closed = true;
        }
    }

    /// A later word arrived inside the head: upstream's `term_newln()`
    /// flush prints the carrying word before the restore, so the jump
    /// stands. Returns the fill to keep.
    pub(super) fn commit_on_later_word(&mut self) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.word_closed)
        {
            self.pending = None;
        }
    }

    /// The head closed with a jump still uncommitted: upstream prints the
    /// buffered word only after the restore zeroed the offset, so the
    /// jump collapses. Returns the node whose fill retracts.
    pub(super) fn retract_on_head_close(&mut self) -> Option<usize> {
        self.pending.take().map(|pending| pending.node)
    }
}

// The booleans are independent native registers (fed flags and latch
// carries), not alternative states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Default)]
pub(super) struct DefinitionFieldState {
    pub(super) pending_indent: Option<usize>,
    /// Latched: some pass rejected and the buffer wipe (term.c:144-146
    /// with 235) discarded its suffix. Unlike `hang_row.field_discarded`
    /// this fact survives later flushes; a run-in BODY reads it to know
    /// its first text shared the wiped buffer.
    pub(super) suffix_discarded_seen: bool,
    /// True only for a NOBREAK field carried across the HEAD/BODY ownership
    /// split (`PreservedDefinitionField`): its rejection is decided by the
    /// BODY words and must be committed at the item post drain
    /// (mdoc_term.c:939-945), the only `term_newln()` this field ever sees.
    pub(super) run_in_continuation: bool,
    /// A line-break request (`.sp`/`.br`, and `.nf`/`.fi` through the same
    /// `roff_term_pre_br()` dispatch, roff_term.c:45-58, 71-78) ran inside
    /// this field and cleared `TERMP_NOBREAK`. Roff request nodes return
    /// before the flag save/restore (mdoc_term.c:394-396), so the clear
    /// outlives the request; only the item post resets it
    /// (mdoc_term.c:961-962). While set, every pass of the field's
    /// `term_fill()` targets `vfield` instead of the page margin
    /// (term.c:134-136): head words wrap at the field's own width.
    pub(super) no_break_cleared: bool,
    /// Row geometry a control request left behind: the indent a break's
    /// row carries and the jump a word emits onto the open HANG row, with
    /// the print-deferral lifetime upstream gives them.
    pub(super) row: HeadRowState,
    /// The head field's content capacity `rmargin - offset`, latched for
    /// `no_break_cleared` sessions: the request that cleared NOBREAK may
    /// also degrade the author effect to `Line`, but the remaining head
    /// words still fill against this bound (term.c:134-136).
    pub(super) cleared_field_capacity_columns: u16,
    /// The native input buffer of this field, fed in source order at each
    /// formatter word (term.c's `tcol->buf`); pass decisions at flush time
    /// come from it instead of streaming heuristics.
    pub(super) field_buffer: super::field_buffer::FieldBuffer,
    /// (cell index, IR node count) at each fed word's start, mapping
    /// buffer positions to output ranges for wipes.
    pub(super) field_word_anchors: Vec<(usize, usize)>,
    /// The still-pending `\z` glyph has already entered the field buffer;
    /// it must not re-enter while IR resolution lags behind the native
    /// `encode1()` write.
    pub(super) pending_glyph_fed: bool,
    /// A word's trailing `\p` deferred into the field (no in-operand
    /// blank): its `'\n'` cell still has to enter the native buffer.
    pub(super) trailing_marker_unfed: bool,
    pub(super) outcome: DefinitionOutcome,
    pub(super) no_break: Option<NoBreakField>,
    // A positive term_vspace() ends the HANG device row. The next author
    // pre-handler can then start its field at the BODY margin.
    pub(super) vertical_started_row: bool,
    pub(super) hang_row: HangNativeRow,
    pub(super) pending_gap_origin: PendingFieldGapOrigin,
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
    pub(super) state: DefinitionFieldState,
    pub(super) gap_cells: u8,
    pub(super) body_width_columns: u16,
    pub(super) field_width_columns: u16,
    pub(super) flags: super::native_field::FieldFlags,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum PendingFieldGapOrigin {
    #[default]
    Other,
    SourceLine,
}

/// The two persistent columns in `term.c::term_flushln()`, plus its unflushed
/// input field. Generated IR padding never enters this ledger.
// These are independent flags of one native field, not alternative states.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Default)]
pub(super) struct HangNativeRow {
    pub(super) viscol: usize,
    pub(super) minbl: usize,
    // BRIND changes the offset while nested HEAD children execute. The
    // enclosing HEAD restores its old offset before its final post flush.
    pub(super) field_offset: usize,
    pub(super) field_width: usize,
    pub(super) trailing_breakable: usize,
    pub(super) field_printable: bool,
    // A field with a breakable boundary can be redistributed by term_fill().
    // The cumulative width is then not proof of its final device column.
    pub(super) field_breakable: bool,
    pub(super) field_discretionary_break: bool,
    pub(super) field_unproven_break: bool,
    pub(super) field_pending_word_end_break: bool,
    // term_fill() returns nbr=0 if \p precedes the field's first graph and
    // the next formatter word adds a separator. No part of that field prints.
    pub(super) field_native_graph: bool,
    // A \p followed by a blank before this word supplied a graph. The
    // pending field must retain only the prefix accepted by term_fill().
    pub(super) field_break_before_graph_prefix: Option<usize>,
    pub(super) accepted_prefix_before_rejection: bool,
    pub(super) last_word_started_with_separator: bool,
    pub(super) last_word_supplied_graph: bool,
    pub(super) consumed_pending_word_end_break: bool,
    pub(super) provisional_trailing_break: Option<usize>,
    pub(super) field_discarded: bool,
    pub(super) field_last_unbreakable_width: usize,
    pub(super) transition: HangRowTransition,
    pub(super) suppress_next_auto_space: bool,
    pub(super) margin_flush_seen: bool,
    // Snapshot taken when a source word's decode begins: a \p armed by an
    // EARLIER word starts this word's term_fill() pass at a blank (term.c
    // resumes right after that word's '\n' buffer cell). When that armed
    // pass had accepted no graph yet, the very first blank rejects the pass
    // (nbr=0, term.c:293-295) and this whole word is unprinted buffer.
    pub(super) field_armed_at_word_start: bool,
    pub(super) field_native_graph_at_word_start: bool,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum HangRowTransition {
    #[default]
    Initial,
    Flushed,
    WordAfterFlush,
}

impl HangNativeRow {
    pub(super) fn word(
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

    fn pending_glyph(&mut self, width: usize) {
        let extends_last_word = self.field_printable && self.trailing_breakable == 0;
        let previous_width = self.field_last_unbreakable_width;
        self.word(0, width, 0, true);
        if extends_last_word {
            self.field_last_unbreakable_width = previous_width.saturating_add(width);
        }
    }

    fn flush(&mut self, trailspace: usize) {
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

    pub(super) fn endline(&mut self) {
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

    fn final_column(&self) -> usize {
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
    pub(super) fn definition_state_mut(&mut self) -> &mut DefinitionFieldState {
        self.definition
            .get_or_insert_with(DefinitionFieldState::default)
    }

    pub(super) fn pending_definition_indent(&self) -> Option<usize> {
        self.definition
            .as_ref()
            .and_then(|state| state.pending_indent)
    }

    pub(super) fn set_pending_definition_indent(&mut self, indent: Option<usize>) {
        self.definition_state_mut().pending_indent = indent;
    }
}

impl DefinitionOutcome {
    const FIELD_EXITED: u8 = 1;
    const BODY_GAP_CONSUMED: u8 = 2;

    pub(super) fn mark_field_exited(&mut self) {
        self.0 |= Self::FIELD_EXITED;
    }

    pub(super) fn mark_body_gap_consumed(&mut self) {
        self.0 |= Self::BODY_GAP_CONSUMED;
    }

    pub(super) fn clear_body_gap_consumed(&mut self) {
        self.0 &= !Self::BODY_GAP_CONSUMED;
    }

    pub(super) const fn field_exited(self) -> bool {
        self.0 & Self::FIELD_EXITED != 0
    }

    pub(super) const fn body_gap_consumed(self) -> bool {
        self.0 & Self::BODY_GAP_CONSUMED != 0
    }
}

/// Native field state left behind by `roff_term_pre_mc()`.
///
/// CVS clears `NOBREAK` and `NOSPACE` after flushing, but deliberately keeps
/// `BRIND`, `HANG`, and the list field geometry for a following request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NoBreakField {
    output_end_before_separator: usize,
    resumed_output_start: usize,
    resumed_execution_epoch: u64,
    field_width: usize,
    body_width: usize,
    /// `rmargin - offset` of the head field (term.c:124-125 uses it as the
    /// pass target after a request cleared `TERMP_NOBREAK`).
    field_capacity_columns: u16,
    trailspace_cells: usize,
    separator_cells: usize,
    style: DefinitionFieldStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DefinitionFieldStyle {
    Tag,
    Hang,
}

impl InlineBuilder {
    /// Every definition HEAD eventually reaches `term_fill()`, including
    /// inset/diag/ohang heads without a NOBREAK field. Track accepted and
    /// rejected word-end slices in their shared native input buffer.
    pub(in crate::mandoc) fn begin_definition_head_consumption(&mut self) {
        self.definition_state_mut();
    }

    /// CVS `mdoc_term.c` enters `NODE_LINE` before each no-fill child, but
    /// `term_newln()` flushes an active `NOBREAK` definition field. `BRIND` may
    /// start a new row when a tag overruns its width; HANG keeps that row.
    pub(in crate::mandoc) fn no_fill_source_line(&mut self) {
        let field = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            });
        if let Some((start, gap, body, field_width, flags)) = field {
            self.flush_definition_field(start, gap, body, field_width, flags, false);
            if self.execution.pending_field_spaces > 0 {
                self.definition_state_mut().pending_gap_origin = PendingFieldGapOrigin::SourceLine;
            }
            // print_mdoc_node() runs term_newln() at each no-fill NODE_LINE.
            // Even if that flush has no buffered glyph, term_newln() sets
            // NOSPACE for the next term_word(). The retained trailspace is
            // separate and can still position the next HANG field.
            self.execution.boundary = PendingBoundary::Tight;
            self.definition_state_mut()
                .hang_row
                .suppress_next_auto_space = true;
        } else {
            self.hard_break();
        }
    }

    /// Execute an explicit formatter line request inside a definition HEAD.
    ///
    /// CVS keeps `LIST_tag/LIST_hang` in a NOBREAK field until `term_newln()`
    /// has settled that field.  A plain `hard_break()` loses BRIND geometry,
    /// so `.br`, `.ti`, and the break phase of `.sp` must use this entrypoint.
    pub(in crate::mandoc) fn control_line_break(&mut self) -> bool {
        if let Some(field) = self.take_no_break_field() {
            let capacity = field.field_capacity_columns;
            self.note_field_control_cleared_no_break(true, capacity);
            self.settle_no_break_field_line(field, 0);
            return true;
        }
        let Some((start, gap, body, field_width_columns, flags)) = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            })
        else {
            let had_cell = self.has_formatter_cell();
            self.hard_break();
            return had_cell;
        };
        self.note_field_control_cleared_no_break(
            flags.contains(FieldFlag::Brind),
            field_width_columns,
        );
        self.flush_definition_field(start, gap, body, field_width_columns, flags, true)
    }

    /// `roff_term_pre_br()` clears `TERMP_NOBREAK` (with `TERMP_BRIND`) for the
    /// rest of this field (roff_term.c:71-78); the enclosing request node
    /// returns before any flag restore (mdoc_term.c:394-396).
    fn note_field_control_cleared_no_break(&mut self, brind: bool, capacity: u16) {
        if !brind {
            return;
        }
        let definition = self.definition_state_mut();
        definition.no_break_cleared = true;
        definition.cleared_field_capacity_columns = capacity;
    }

    // The NOBREAK flush commits field, row, and BRIND state in native order.
    #[allow(clippy::too_many_lines)]
    pub(super) fn flush_definition_field(
        &mut self,
        field_output_start: usize,
        gap_cells: u8,
        body_width_columns: u16,
        field_width_columns: u16,
        flags: FieldFlags,
        exit_field: bool,
    ) -> bool {
        let had_marker_passes = self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.transition != HangRowTransition::Initial);
        let pending_native_gap = self.execution.pending_field_spaces > 0;
        let native_field_discarded = self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.field_discarded);
        let native_field_printable = !flags.wraps()
            && !native_field_discarded
            && (self
                .execution
                .definition
                .as_ref()
                .is_some_and(|state| state.hang_row.field_printable)
                || self.pending_hang_glyph_width().is_some());
        self.flush_native_hang_field(gap_cells, body_width_columns, exit_field);
        if !self.has_formatter_cell() {
            // `roff_term_pre_br()` applies BRIND even when `term_newln()` had
            // no tcol bytes or device row to flush.  It moves the next word
            // to the body margin and clears NOBREAK/BRIND.  HANG itself
            // survives, so a hang head and its body remain on that same row;
            // a tag head instead finishes as an ordinary line field.
            if exit_field {
                // A source-line term_newln() may have already flushed this
                // field and left its trailspace for the next word. An
                // explicit .br consumes that pending gap while BRIND moves
                // the offset; it does not print another field's padding.
                self.definition_state_mut().pending_indent =
                    (!pending_native_gap).then_some(usize::from(body_width_columns));
                self.execution.pending_field_spaces = 0;
                // `roff_term_pre_br()` changes the device offset even for
                // an empty field. It does not advance `p->viscol`; the
                // offset was recorded in `hang_row.field_offset` above.
                self.execution.boundary = PendingBoundary::Tight;
                if let Some(execution) = &mut self.execution.author_execution {
                    execution.field_output_start = self.nodes.len();
                    if flags.wraps() {
                        self.execution
                            .definition
                            .as_mut()
                            .expect("definition field session")
                            .outcome
                            .mark_field_exited();
                        execution.break_effect = AuthorBreakEffect::Line;
                    } else {
                        self.execution
                            .definition
                            .as_mut()
                            .expect("definition field session")
                            .outcome
                            .mark_body_gap_consumed();
                        execution.break_effect = AuthorBreakEffect::Field {
                            gap_cells: 0,
                            body_width_columns,
                            field_width_columns,
                            flags,
                        };
                    }
                }
            }
            return false;
        }
        self.flush_zero_advance();
        if native_field_discarded {
            let mut field = self
                .nodes
                .split_off(field_output_start.min(self.nodes.len()));
            retain_unprinted_field_targets(&mut field);
            self.nodes.extend(field);
        }
        let field = self.nodes.get(field_output_start..).unwrap_or_default();
        // term_fill() returns nbr=0 for a HANG field containing only \p,
        // ordinary breakable blanks, or invisible controls. Its IR padding
        // may look printable, but term_flushln() keeps the device row open.
        let field_is_printable = if native_field_discarded {
            false
        } else if flags.wraps() {
            has_printable_character(field)
        } else {
            native_field_printable
        };
        let field_width = mant_ir::geometry::text_width(&super::super::plain_text(field));
        if !flags.wraps() && !field_is_printable {
            // term_fill() returns nbr=0 for a HANG field with only ordinary
            // blanks and controls. Drop only this field's breakable padding:
            // the fixed cells from the preceding field still position BODY.
            let mut unprinted = self
                .nodes
                .split_off(field_output_start.min(self.nodes.len()));
            trim_trailing_breakable_spaces(&mut unprinted, usize::MAX);
            self.nodes.extend(unprinted);
        }
        let body_width = usize::from(body_width_columns);
        let overruns = field_is_printable
            && flags.wraps()
            && field_width.saturating_add(usize::from(gap_cells)) > body_width;

        let mut deferred_field_cells = 0;
        if overruns {
            self.hard_break();
            if exit_field {
                self.definition_state_mut().pending_indent = Some(body_width);
            }
        } else if field_is_printable {
            let cells = if exit_field && flags.wraps() {
                body_width.saturating_sub(field_width)
            } else if exit_field {
                body_width
                    .saturating_sub(field_width)
                    .max(usize::from(gap_cells))
            } else {
                usize::from(gap_cells)
            };
            if !exit_field && !flags.wraps() {
                // term_flushln() retains trailspace as minbl. A following
                // formatter word materializes it, while roff_term_pre_br()
                // can clear it before that word. IR must make the same
                // decision at the consuming event, not at field flush.
                deferred_field_cells = cells;
            } else {
                self.append_fixed_cells(cells);
            }
        } else if exit_field {
            // An explicit empty word and `\&` still execute the NOBREAK
            // field.  There is no row to close, but `roff_term_pre_br()`
            // applies BRIND before the following word.
            //
            // term.c:250-252: when an earlier pass already restarted at the
            // right margin (BRIND), the remaining field budget is zero and a
            // NOBREAK field without HANG closes the row before BODY: the
            // accepted prefixes stay on their own rows and the body starts a
            // new one. HANG ignores the overrun and keeps the shared row.
            // `hang_row.viscol` already carries the `field_offset` floor
            // from this field's own flush, so the restart signal is the
            // pre-flush transition: only accepted or rejected in-word
            // `term_fill()` passes leave it non-initial.
            if had_marker_passes && !row_continues(flags, 0, 0) {
                self.definition_state_mut().pending_indent = Some(body_width);
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_field_exited();
                if let Some(execution) = &mut self.execution.author_execution {
                    execution.break_effect = AuthorBreakEffect::Line;
                }
            } else {
                self.definition_state_mut().pending_indent = Some(body_width);
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_body_gap_consumed();
            }
        } else {
            // term_flushln() restores minbl from trailspace even when
            // term_fill() accepted no graph. Only a later formatter word or
            // roff_term_pre_br() decides whether those device cells print.
            if !flags.wraps()
                && self.execution.word_end_break == WordEndBreak::Pending
                && self
                    .execution
                    .definition
                    .as_ref()
                    .is_some_and(|state| state.hang_row.viscol > 0)
            {
                deferred_field_cells = usize::from(gap_cells);
            }
            self.execution.trailing_output = TrailingOutput::None;
        }

        self.execution.boundary = PendingBoundary::Tight;
        self.execution.empty_word = false;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = deferred_field_cells;
        self.definition_state_mut().pending_gap_origin = PendingFieldGapOrigin::Other;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.formatter_column = FormatterColumn::Origin;
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            if exit_field && flags.wraps() {
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_field_exited();
                execution.break_effect = AuthorBreakEffect::Line;
            } else if exit_field {
                // TERMP_HANG survives `term_newln()`, but the generated field
                // gap has already been emitted for this request.
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_body_gap_consumed();
                execution.break_effect = AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns,
                    field_width_columns,
                    flags,
                };
            }
        }
        // term.c:233-237: the committed flush ends the field; the input
        // buffer restarts empty for whatever follows this row.
        if let Some(definition) = &mut self.execution.definition {
            definition.field_buffer.clear();
            definition.field_word_anchors.clear();
            definition.pending_glyph_fed = false;
            // A mid-field flush is a row event (upstream's `term_newln`
            // printing the buffered word before the restore,
            // mdoc_term.c:1084-1085): the jump stands. Only the field's
            // final flush - past the restore (mdoc_term.c:437-439) -
            // collapses it.
            if exit_field {
                if let Some(jump_node) = definition.row.retract_on_head_close()
                    && let Some(Inline::Text { value }) = self.nodes.get_mut(jump_node)
                {
                    value.clear();
                }
            } else {
                definition.row.commit_on_later_word();
            }
        }
        overruns
    }

    fn flush_native_hang_field(&mut self, gap: u8, body_width: u16, exit_field: bool) {
        let had_cell = self.has_formatter_cell();
        let pending_glyph_width = self.pending_hang_glyph_width();
        if let Some(definition) = &mut self.execution.definition {
            if let Some(width) = pending_glyph_width {
                definition.hang_row.pending_glyph(width);
            }
            if had_cell || pending_glyph_width.is_some() || definition.hang_row.viscol > 0 {
                definition.hang_row.flush(usize::from(gap));
            }
            if exit_field {
                definition.hang_row.field_offset = usize::from(body_width);
            }
        }
    }

    /// `term.c::encode1()` buffers the character following `\\z` in the
    /// current field. It has not reached an IR node yet, but `term_fill()` and
    /// `term_field()` still count its printed width at a field boundary.
    fn pending_hang_glyph_width(&self) -> Option<usize> {
        let mut zero_advance = self.execution.zero_advance.clone();
        let mut pending = Vec::new();
        zero_advance.finish_into(&mut pending);
        let text = super::super::plain_text(&pending);
        text.chars()
            .any(|ch| !ch.is_whitespace() || ch == '\u{a0}')
            .then(|| mant_ir::geometry::text_width(&text))
    }

    /// Whether the next word already has a request-armed concatenation
    /// (`TERMP_NOSPACE`, `roff_term.c:78`) pending.
    pub(in crate::mandoc) fn concat_word_armed(&self) -> bool {
        self.execution.concat_next_word
    }

    /// Record the graph counts at which this word executed a zero-width
    /// breakpoint `\:` (term.c:287-300). Consumed by the HANG field's
    /// width simulation when the word's cells are buffered.
    pub(in crate::mandoc) fn note_word_zero_break_prefixes(&mut self, prefixes: &[usize]) {
        self.execution.word_zero_break_prefixes = prefixes.to_vec();
    }

    /// Arm the next word's concatenation for a filled cleared field
    /// (term.c:250-253 with 205-207): same no-separator word, but the
    /// body starts at the description column rather than against the
    /// last head cell.
    pub(in crate::mandoc) fn note_flushed_at_body_column(&mut self) {
        self.execution.concat_next_word = true;
        self.execution.concat_flush_source = true;
    }

    /// A HANG head that filled its capacity while a request had cleared
    /// `TERMP_NOBREAK` reaches the body column with no trailspace
    /// (term.c:250-253 with 205-207): the body's first word concatenates.
    pub(in crate::mandoc) fn cleared_field_filled_capacity(&self) -> bool {
        self.execution.definition.as_ref().is_some_and(|state| {
            // roff.c::post_hyph() marks a source hyphen ASCII_HYPH and
            // term.c::term_fill() may wrap after it; a discretionary break
            // in the final field leaves the same soft-wrap uncertainty as
            // `\:` (term.c:287-300 shares the arm), so the filled rule
            // cannot prove the body column.
            if state.hang_row.field_discretionary_break {
                return false;
            }
            let capacity = usize::from(state.cleared_field_capacity_columns);
            if !state.no_break_cleared || capacity == 0 {
                return false;
            }
            // Only the FINAL pass decides (term.c:362-366 accepted it as
            // the row term_flushln() leaves open): rows a width pass
            // already ended do not reach the body column. A `\:` inside
            // the word buffered its own ASCII_BREAK cell (term.c:287-300),
            // so an overrun there breaks the pass chain itself and the
            // short remainder upstream stays the final pass. A pass that
            // exactly meets the capacity only proves the body column when
            // a wrap resumed there: upstream decides with the final row's
            // `viscol` (term.c:250-253), and reproducing that exactly also
            // needs the vspace/NOSPACE ledger of the row before the clear
            // (`minbl = trailspace`, term.c:236, and request-armed
            // `TERMP_NOSPACE`, roff_term.c:78) — registered as follow-up
            // work; the resume condition separates the provable cases.
            let mut simulation = state.field_buffer.clone();
            let mut last_width = 0;
            let mut final_pass_started_at_boundary = false;
            while let Some(pass) = simulation.fill_pass(capacity) {
                last_width = pass.accepted_width;
                final_pass_started_at_boundary = simulation.resume_offset() > 0;
                simulation.advance_past(pass.accepted_end);
                simulation.consume_break_blanks();
                if simulation.resume_offset() >= simulation.cells().len() {
                    break;
                }
            }
            last_width > capacity || (last_width == capacity && final_pass_started_at_boundary)
        })
    }

    pub(in crate::mandoc) fn definition_field_exited(&self) -> bool {
        self.execution
            .definition
            .as_ref()
            .is_some_and(|state| state.outcome.field_exited())
    }

    pub(in crate::mandoc) fn definition_body_gap_consumed(&self) -> bool {
        let Some(state) = &self.execution.definition else {
            return false;
        };
        if !state.hang_row.margin_flush_seen
            && state.hang_row.transition == HangRowTransition::WordAfterFlush
            && let Some(AuthorBreakEffect::Field {
                body_width_columns,
                gap_cells,
                flags,
                ..
            }) = self
                .execution
                .author_execution
                .as_ref()
                .map(|author| author.break_effect)
            && flags.contains(FieldFlag::Hang)
        {
            // At HEAD post, term_newln() flushes the final HANG field. Only
            let mut final_row = state.hang_row.clone();
            if let Some(width) = self.pending_hang_glyph_width() {
                final_row.pending_glyph(width);
            }
            let body_column = usize::from(body_width_columns);
            let cumulative_column = final_row.final_column();
            if final_row.field_discarded {
                // No new glyph reached the device. The preceding field's
                // viscol and minbl still locate BODY; source text inside the
                // discarded field cannot create a soft-wrap uncertainty.
                return cumulative_column >= body_column;
            }
            // CVS term.c::term_fill() may wrap at a breakable cell *inside*
            // this final field. Its summed width then says nothing about the
            // last physical row. Retain the word boundary unless the field
            // provably stayed on one row (or had no breakable cell).
            let final_row_proven = !final_row.field_unproven_break
                && (cumulative_column <= body_column
                    || (!final_row.field_discretionary_break
                        && (final_row.field_last_unbreakable_width >= body_column
                            || !final_row.field_breakable)));
            return gap_cells == 0 && final_row_proven && cumulative_column >= body_column;
        }
        state.outcome.body_gap_consumed()
    }

    pub(in crate::mandoc) fn note_discretionary_hang_field_break(&mut self) {
        if let Some(definition) = &mut self.execution.definition {
            definition.hang_row.field_discretionary_break = true;
        }
    }

    pub(in crate::mandoc) fn note_hang_native_graph(&mut self) {
        if let Some(definition) = &mut self.execution.definition
            && !definition.hang_row.field_discarded
        {
            definition.hang_row.field_native_graph = true;
        }
    }

    /// `term.c::term_fill()` stops at a breakable blank after `\p`. When the
    /// field has not supplied a graph yet, even later words in that field are
    /// never printed. The text decoder reports this event inside one word;
    /// `HangNativeRow::word()` handles the same event across words.
    /// Snapshot the pass state a source word's decode begins in. See
    /// `HangNativeRow::field_armed_at_word_start`.
    pub(in crate::mandoc) fn note_hang_word_decode_start(&mut self) {
        if let Some(definition) = &mut self.execution.definition {
            let row = &mut definition.hang_row;
            row.field_armed_at_word_start = row.field_pending_word_end_break;
            row.field_native_graph_at_word_start = row.field_native_graph;
        }
    }

    pub(in crate::mandoc) fn note_hang_break_before_graph(&mut self, accepted_prefix: usize) {
        if let Some(definition) = &mut self.execution.definition
            && definition.field_buffer.backbefore_armed()
        {
            // term.c:901-908: the completed \z glyph's TERMP_BACKBEFORE
            // retreat eats the blank directly before the next graph, so
            // that blank never enters the buffer and this marker's pass
            // cannot reject on it. The row still closes after the accepted
            // prefix (term.c:220); only the wipe is spurious.
            return;
        }
        if let Some(definition) = &mut self.execution.definition {
            definition.suffix_discarded_seen = true;
            let row = &mut definition.hang_row;
            row.field_discarded = true;
            row.field_break_before_graph_prefix = Some(accepted_prefix);
            row.field_unproven_break = true;
        }
    }

    /// Predict `term_flushln()`'s pass loop over the fed buffer: whether any
    /// pass would reject with nbr==0 (term.c:143-146), and the resume
    /// position where the unprinted remainder begins. Dry-runs a clone so
    /// the live buffer keeps its own col until a real flush.
    fn buffer_flush_rejection(&self) -> Option<(usize, bool)> {
        let definition = self.execution.definition.as_ref()?;
        if definition.field_buffer.is_empty()
            || !definition
                .field_buffer
                .cells()
                .iter()
                .any(|cell| matches!(cell, super::field_buffer::FieldCell::BreakMarker))
        {
            // Only marker-driven rejections change decisions here; fields
            // of plain blanks keep their existing accounting.
            return None;
        }
        let mut simulation = definition.field_buffer.clone();
        let mut accepted_any_pass = false;
        loop {
            match simulation.fill_pass(usize::MAX / 2) {
                None if !accepted_any_pass => {
                    // The field rejected from its very first pass: the
                    // word-level accounting already owns this shape (an
                    // armed \p met a separator before any graph). Only
                    // multi-pass chains — an accepted prefix followed by a
                    // rejected remainder — change decisions here.
                    return None;
                }
                None => return Some((simulation.resume_offset(), accepted_any_pass)),
                Some(pass) => {
                    accepted_any_pass = true;
                    simulation.advance_past(pass.accepted_end);
                    simulation.consume_break_blanks();
                    // term.c:177-198: the loop exits when only ignorable
                    // cells (blanks, markers) remain, WITHOUT running
                    // another pass; a trailing armed marker cannot reject
                    // after the last accepted slice.
                    if simulation.resume_offset() >= simulation.cells().len()
                        || simulation.only_ignorable_remainder(false)
                    {
                        return None;
                    }
                }
            }
        }
    }

    pub(in crate::mandoc) fn discard_unprinted_definition_field_output(&mut self) {
        if !self
            .execution
            .definition
            .as_ref()
            .is_some_and(|definition| definition.hang_row.field_discarded)
            && let Some((rejection_resume, accepted_any_pass)) = self.buffer_flush_rejection()
        {
            // The pass loop rejects from the fed buffer (term.c:143-146):
            // everything from the rejected resume position is unprinted.
            // Map that buffer position to the IR range of the word that
            // starts there.
            let definition = self.execution.definition.as_mut().unwrap();
            definition.hang_row.field_discarded = true;
            definition.suffix_discarded_seen = true;
            if accepted_any_pass {
                // term.c:220: every accepted pass before the rejected one
                // ended its device row; only the final pass consults the
                // HANG/NOBREAK tail rule (250-253).
                definition.hang_row.accepted_prefix_before_rejection = true;
            } else {
                // The rejected field never occupied the device row
                // (term.c:233-237 resets col/lastcol): term_newln() at the
                // settling request stays a no-op and the row stays open.
                self.execution.formatter_column = FormatterColumn::Origin;
                self.execution.word_end_break = WordEndBreak::Clear;
                self.execution.pending_breakable_spaces = 0;
                self.execution.trailing_output = TrailingOutput::None;
            }
            definition.field_buffer.wipe_remainder();
            let start = definition
                .field_word_anchors
                .iter()
                .find(|(cell_index, _)| *cell_index >= rejection_resume)
                .map_or_else(
                    || {
                        definition
                            .field_word_anchors
                            .last()
                            .map_or(0, |(_, ir)| *ir)
                    },
                    |(_, ir)| *ir,
                );
            if let Some(author) = &mut self.execution.author_execution {
                author.field_output_start = start;
            }
        }
        let Some((start, exited_field)) = self
            .execution
            .author_execution
            .as_ref()
            .filter(|_| self.execution.definition.is_some())
            .map(|execution| {
                (
                    execution.field_output_start,
                    matches!(execution.break_effect, AuthorBreakEffect::Line),
                )
            })
        else {
            return;
        };
        if !self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.field_discarded)
        {
            return;
        }
        let mut field = self.nodes.split_off(start.min(self.nodes.len()));
        retain_unprinted_field_targets(&mut field);
        self.nodes.extend(field);
        if self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.accepted_prefix_before_rejection)
            && !matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))
        {
            // A prior term_fill() pass printed its accepted prefix; a later
            // nbr=0 discards only the suffix and ends that device line.
            self.nodes.push(Inline::line_break());
        }
        // term.c::term_flushln() clears both BACKAFTER and BACKBEFORE even
        // when term_fill() returns nbr=0. The rejected field can still own a
        // completed \z glyph that has not entered the IR suffix yet.
        self.execution.zero_advance.discard_at_row_end();
        if let Some(definition) = &mut self.execution.definition {
            definition.field_buffer.clear_backtracking();
        }
        self.execution.last_visible_character = last_visible_character(&self.nodes);
        if exited_field {
            // A TAG .br may have already ended NOBREAK, but subsequent words
            // still share one term_fill() input buffer until the next actual
            // line request. Dropping that buffer leaves no current cell.
            self.execution.formatter_column = FormatterColumn::Origin;
            self.execution.word_end_break = WordEndBreak::Clear;
            self.execution.pending_breakable_spaces = 0;
            self.execution.trailing_output = TrailingOutput::None;
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
            }
        }
    }

    pub(in crate::mandoc) fn discarded_exited_definition_buffer(&self) -> bool {
        self.execution
            .definition
            .as_ref()
            .is_some_and(|definition| {
                definition.hang_row.field_discarded
                    && self
                        .execution
                        .author_execution
                        .as_ref()
                        .is_some_and(|execution| {
                            matches!(execution.break_effect, AuthorBreakEffect::Line)
                        })
            })
    }

    pub(in crate::mandoc) fn pending_definition_break_has_no_graph(&self) -> bool {
        self.in_definition_field()
            && self.execution.definition.as_ref().is_some_and(|state| {
                state.hang_row.field_discarded
                    || (state.hang_row.field_pending_word_end_break
                        && !state.hang_row.field_native_graph)
            })
    }

    pub(in crate::mandoc) fn in_definition_field(&self) -> bool {
        self.execution.definition.is_some() && self.execution.author_execution.is_some()
    }

    /// A head field configured with `AuthorBreakEffect::Field` IS a
    /// definition field, even when a paragraph drain retired the lazily
    /// created session state. Re-establish it so marker bookkeeping
    /// (`term.c` buffer rules) applies for the whole head.
    pub(in crate::mandoc) fn ensure_definition_field_session(&mut self) {
        if self
            .execution
            .author_execution
            .as_ref()
            .is_some_and(|author| matches!(author.break_effect, AuthorBreakEffect::Field { .. }))
        {
            self.definition_state_mut();
        }
    }

    pub(in crate::mandoc) fn consumed_pending_hang_word_end_break(&self) -> bool {
        self.execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.consumed_pending_word_end_break)
    }

    pub(in crate::mandoc) fn note_provisional_definition_break(&mut self) {
        if let Some(definition) = &mut self.execution.definition
            && matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))
        {
            definition.hang_row.provisional_trailing_break = Some(self.nodes.len() - 1);
        }
    }

    pub(in crate::mandoc) fn settle_provisional_definition_break(&mut self) {
        let Some(definition) = &mut self.execution.definition else {
            return;
        };
        if definition.hang_row.provisional_trailing_break == self.nodes.len().checked_sub(1)
            && !definition.hang_row.field_discarded
            && self.execution.word_end_break == WordEndBreak::Pending
        {
            // An earlier \p made this possible break, but the final \p had
            // no following graph to complete another term_fill() slice.
            // HEAD post consumes the buffer without closing the device row.
            self.nodes.pop();
        }
        definition.hang_row.provisional_trailing_break = None;
    }

    pub(super) fn append_fixed_cells(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.append_projected(vec![Inline::Text {
            value: " ".repeat(count),
        }]);
        self.execution.trailing_output = TrailingOutput::FixedBlank;
    }

    /// Execute `.ti` through its preceding `roff_term_pre_br()` boundary.
    ///
    /// `ManT` deliberately omits the device-specific temporary offset.  In
    /// particular, the numeric operand is not printable padding: the pinned
    /// renderer applies it to `p->ti` and `tcol->offset` only after flushing
    /// the current field.  Retain the field's own trailspace and boundary,
    /// then discard the temporary device position as documented.
    pub(in crate::mandoc) fn temporary_indent(&mut self) {
        if let Some(field) = self.take_no_break_field() {
            self.restore_no_break_field_projection(field);
            match field.style {
                DefinitionFieldStyle::Tag => {
                    self.force_output_line_break();
                    self.execution
                        .definition
                        .as_mut()
                        .expect("definition field session")
                        .outcome
                        .mark_field_exited();
                }
                DefinitionFieldStyle::Hang => {
                    // `term_newln()` leaves NOSPACE set.  HANG keeps the
                    // device row alive, so a following control-only author
                    // transition must not synthesize an ordinary word blank
                    // between the prior field and its eventual head text.
                    self.execution.boundary = PendingBoundary::Tight;
                    self.execution.pending_field_spaces = 1;
                }
            }
            self.finish_definition_field_control(field, 0, false);
            return;
        }
        let Some((start, gap, body, field_width_columns, flags)) = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            })
        else {
            self.control_line_break();
            return;
        };
        if !self.has_formatter_cell() {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            return;
        }
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        if !has_printable_character(field) {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            return;
        }
        let field_width = mant_ir::geometry::text_width(&super::super::plain_text(field));
        let overruns =
            flags.wraps() && field_width.saturating_add(usize::from(gap)) > usize::from(body);
        if overruns {
            self.hard_break();
        } else {
            self.append_fixed_cells(usize::from(gap));
        }
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            if flags.wraps() {
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_field_exited();
                execution.break_effect = AuthorBreakEffect::Line;
            } else {
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_body_gap_consumed();
                execution.break_effect = AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns: body,
                    field_width_columns,
                    flags,
                };
            }
        }
        self.execution.boundary = PendingBoundary::Tight;
    }

    /// Enter or leave no-fill mode at a physical source-line boundary.
    /// `print_mdoc_node()` performs that boundary in addition to the request's
    /// own `roff_term_pre_br()` dispatch.
    pub(in crate::mandoc) fn fill_mode_boundary(&mut self) {
        // The request's roff_term_pre_br() sets TERMP_NOSPACE after its
        // term_newln() (roff_term.c:75-78): the first word after `.nf`/
        // `.fi` concatenates onto the current row with no auto blank —
        // the reference prints `body linetail text` after `.fi`.
        self.execution.concat_next_word = true;
        self.execution.concat_flush_source = false;
        if let Some(field) = self.take_no_break_field() {
            // print_mdoc_node() runs this fill-mode boundary in addition to
            // the request's own roff_term_pre_br() (roff_term.c:45-58).
            let capacity = field.field_capacity_columns;
            self.note_field_control_cleared_no_break(true, capacity);
            self.settle_no_break_field_line(field, capacity);
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_field_exited();
            return;
        }
        let Some((start, gap, body, field_width_columns, flags)) = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            })
        else {
            self.control_line_break();
            return;
        };
        self.note_field_control_cleared_no_break(
            flags.contains(FieldFlag::Brind),
            field_width_columns,
        );
        // roff.c:3581-3585 with 957-958: every node under ROFF_NOFILL prints
        // with NODE_NOFILL, and mdoc_term.c:314-318 maps that to
        // TERMP_BRNEVER — an infinite term_fill() target (term.c:143-144).
        if !self.has_formatter_cell() {
            return;
        }
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        if !has_printable_character(field) {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            return;
        }
        if flags.wraps() {
            // roff_term.c:73-75: the request's BRIND moved the row origin to
            // the field's right margin and the roff node escapes the
            // save/restore (mdoc_term.c:393-397). The word already flushed
            // this field, so the pending-indent arm cannot carry it; the
            // break itself does.
            let row_indent = if flags.contains(FieldFlag::Brind) {
                field_width_columns
            } else {
                0
            };
            self.hard_break();
            if row_indent > 0
                && let Some(Inline::LineBreak { indent_columns }) = self.nodes.last_mut()
            {
                *indent_columns = row_indent;
            }
            self.definition_state_mut().pending_indent = Some(usize::from(body));
        } else {
            let width = mant_ir::geometry::text_width(&super::super::plain_text(field));
            if flags.contains(FieldFlag::Hang) {
                // HANG kept the row open through the boundary
                // (roff_term.c:76): the following word aligns through
                // `vbl = offset - viscol` at its print (term.c:113-114).
                // Upstream has no `body - width` fixed padding on this
                // row — a deferred print past the element restore zeroes
                // the offset (mdoc_term.c:437-439) and the fill collapses,
                // which the armed-offset state machine carries.
                self.definition_state_mut()
                    .row
                    .arm_jump(field_width_columns);
            } else {
                self.append_fixed_cells(usize::from(body).saturating_sub(width));
            }
        }
        self.execution
            .definition
            .as_mut()
            .expect("definition field session")
            .outcome
            .mark_field_exited();
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            execution.break_effect = if flags.wraps() {
                AuthorBreakEffect::Line
            } else {
                AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns: body,
                    field_width_columns,
                    flags,
                }
            };
        }
        self.execution.boundary = PendingBoundary::Tight;
    }

    /// Execute a visited empty TEXT at its actual node position. Native
    /// `print_man_node()`/`print_mdoc_node()` call `term_newln()` for an active \c;
    /// otherwise `term_vspace()` consumes skipvsp before emitting a blank row.
    pub(in crate::mandoc) fn execute_visited_empty_text(&mut self) {
        if self.final_source_continuation_or(false) {
            self.hard_break();
            // term_newln() does not clear TERMP_NONEWLINE. Another empty
            // source TEXT therefore also takes this branch, without adding
            // a vertical row or consuming skipvsp.
            self.continue_source_line(true);
        } else {
            let rows = self.resolve_vertical_space(1);
            self.vertical_space(usize::from(rows));
            self.asserted_vertical_row |= rows > 0;
            self.execution.completed_vertical_rows =
                self.execution.completed_vertical_rows.saturating_add(rows);
        }
    }

    /// Execute an inline vertical-space request without retaining its
    /// numeric operand as document text.  `term_vspace(n)` first closes an
    /// occupied row, then emits `n` empty rows.
    // term_vspace() must settle the active field before asserting rows.
    #[allow(clippy::too_many_lines)]
    pub(in crate::mandoc) fn vertical_space(&mut self, rows: usize) {
        if let Some(field) = self.take_no_break_field() {
            self.vertical_space_in_definition_field(field, rows);
            self.finish_native_vertical_row(rows);
            if rows > 0
                && self
                    .execution
                    .author_execution
                    .as_ref()
                    .is_some_and(|execution| {
                        matches!(execution.break_effect, AuthorBreakEffect::Line)
                    })
            {
                self.execution
                    .author_execution
                    .as_mut()
                    .unwrap()
                    .field_output_start = self.nodes.len();
            }
            return;
        }
        if rows == 0 {
            // roff_term_pre_sp() calls no term_vspace() for zero rows. Its
            // remaining pre_br() follows the active HANG/TAG field rules.
            // term_newln() does not clear TERMP_NONEWLINE: a preceding \c
            // still suppresses the next no-fill NODE_LINE after this request.
            self.control_line_break();
            self.execution.final_word_join = Some(false);
            return;
        }
        // term_vspace() first runs term_newln(). Settle an unprintable HANG
        // field there, before the request's vertical rows are projected.
        self.discard_unprinted_definition_field_output();
        let field = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    body_width_columns,
                    field_width_columns,
                    flags,
                    ..
                } => Some((body_width_columns, field_width_columns, flags)),
                AuthorBreakEffect::Line => None,
            });
        if let Some((body_width_columns, field_width_columns, flags)) = field {
            self.note_field_control_cleared_no_break(
                flags.contains(FieldFlag::Brind),
                field_width_columns,
            );
            if !self.has_formatter_cell() {
                // `term_vspace()` always emits its requested empty row, but
                // its leading `term_newln()` leaves a bare BACKAFTER armed
                // when neither tcol nor viscol is occupied.  The following
                // BRIND phase still ends a tag field; HANG keeps its run-in
                // body contract.  Preserve those independent effects.
                // term.c:486-498: term_vspace() runs one conditional
                // term_newln() and then one unconditional endline per
                // requested row, so the row close consumes one break and
                // `rows` blank rows remain.
                self.retain_line_breaks(rows + 1);
                self.execution.boundary = PendingBoundary::Tight;
                if flags.wraps() {
                    self.execution
                        .definition
                        .as_mut()
                        .expect("definition field session")
                        .outcome
                        .mark_field_exited();
                    if let Some(execution) = &mut self.execution.author_execution {
                        execution.field_output_start = self.nodes.len();
                        execution.break_effect = AuthorBreakEffect::Line;
                    }
                }
                self.execution.final_word_join = Some(false);
                self.execution.final_source_continuation = Some(false);
                self.finish_native_vertical_row(rows);
                return;
            }
            // `roff_term_pre_sp()` executes term_vspace() before the final
            // BRIND transition. HANG can suppress term_newln(), but the
            // vertical request still ends the row; field padding is trailing
            // geometry and must not leak onto the empty row.
            // roff_term_pre_sp() executes term_vspace() before the final
            // BRIND transition. The CVS-pinned occupied-head rows keep
            // term_newln()'s close consuming the first requested row for
            // wrappable fields; HANG suppresses that close only when the
            // field did not overrun (term.c:250-252).
            let start = self
                .author_execution
                .as_ref()
                .map_or(self.nodes.len(), |execution| execution.field_output_start);
            self.flush_zero_advance();
            let width = mant_ir::geometry::text_width(&super::super::plain_text(
                self.nodes.get(start..).unwrap_or_default(),
            ));
            let trailspace = self
                .author_execution
                .as_ref()
                .map_or(0, |execution| match execution.break_effect {
                    AuthorBreakEffect::Field { gap_cells, .. } => usize::from(gap_cells),
                    AuthorBreakEffect::Line => 0,
                });
            let term_newln_ended_row =
                flags.wraps() && width.saturating_add(trailspace) > usize::from(body_width_columns);
            self.hard_break();
            self.retain_line_breaks(if term_newln_ended_row {
                rows
            } else {
                rows.saturating_sub(1)
            });
            self.definition_state_mut().pending_indent = Some(usize::from(body_width_columns));
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
                if flags.wraps() {
                    self.execution
                        .definition
                        .as_mut()
                        .expect("definition field session")
                        .outcome
                        .mark_field_exited();
                    execution.break_effect = AuthorBreakEffect::Line;
                } else {
                    self.execution
                        .definition
                        .as_mut()
                        .expect("definition field session")
                        .outcome
                        .mark_body_gap_consumed();
                    execution.break_effect = AuthorBreakEffect::Field {
                        gap_cells: 0,
                        body_width_columns,
                        field_width_columns,
                        flags,
                    };
                }
            }
            // term.c:233-237: the committed flush ends the field; the
            // input buffer restarts empty for whatever follows this row.
            if let Some(definition) = &mut self.execution.definition {
                definition.field_buffer.clear();
                definition.field_word_anchors.clear();
                definition.pending_glyph_fed = false;
            }
        } else {
            self.hard_break();
            self.retain_line_breaks(rows);
        }
        self.finish_native_vertical_row(rows);
        if self.execution.definition.is_some()
            && self
                .execution
                .author_execution
                .as_ref()
                .is_some_and(|execution| matches!(execution.break_effect, AuthorBreakEffect::Line))
        {
            self.execution
                .author_execution
                .as_mut()
                .unwrap()
                .field_output_start = self.nodes.len();
        }
        self.execution.final_word_join = Some(false);
        self.execution.final_source_continuation = Some(false);
    }

    fn finish_native_vertical_row(&mut self, rows: usize) {
        if rows == 0 {
            return;
        }
        if let Some(definition) = &mut self.execution.definition {
            // term_vspace() emits an endline after the current field. Unlike
            // a HANG term_newln(), the next word starts a new device row.
            definition.hang_row.endline();
            definition.vertical_started_row = true;
        }
    }

    fn vertical_space_in_definition_field(&mut self, field: NoBreakField, rows: usize) {
        let capacity = field.field_capacity_columns;
        self.note_field_control_cleared_no_break(true, capacity);
        if rows == 0 {
            // roff_term_pre_sp() skips term_vspace() for zero rows, then runs
            // roff_term_pre_br(). HANG's term_flushln() retains the same
            // physical row; only a positive vertical request ends it.
            self.settle_no_break_field_line(field, 0);
            self.execution.final_word_join = Some(false);
            return;
        }
        self.restore_no_break_field_projection(field);
        self.force_output_line_break();
        if field.style == DefinitionFieldStyle::Tag {
            self.retain_line_breaks(rows);
        } else {
            self.retain_line_breaks(rows.saturating_sub(1));
        }
        // For tag fields the request closes the device row before
        // `roff_term_pre_br()` consumes BRIND; the node-local offset is
        // restored by mdoc traversal, so later head content resumes at
        // the list origin. HANG deliberately keeps its run-in body origin.
        self.definition_state_mut().pending_indent = match field.style {
            DefinitionFieldStyle::Tag => None,
            DefinitionFieldStyle::Hang => Some(field.body_width),
        };
        if field.style == DefinitionFieldStyle::Tag {
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_field_exited();
        }
        self.finish_definition_field_control(field, 0, true);
        self.execution.final_word_join = Some(false);
        self.execution.final_source_continuation = Some(false);
    }

    /// Commit the current formatter cell without ending its visual row.
    ///
    /// The pinned CVS renderer uses this for `.mc`: pending `\z` content is
    /// materialized, while `TERMP_NOBREAK` keeps the next source word on the
    /// same line and clears `TERMP_NOSPACE`. Device margin geometry is outside
    /// the IR, so the next word observes one ordinary boundary.
    pub(in crate::mandoc) fn no_break_flush(&mut self) {
        // roff_term_pre_mc() only calls term_flushln() after the formatter
        // has advanced the current output column. A completed `\zX` glyph
        // has entered the buffer; a bare armed `\z` has not.
        let executed_field_word = self
            .definition
            .as_ref()
            .and_then(|state| state.no_break)
            .is_some_and(|field| self.execution.execution_epoch != field.resumed_execution_epoch);
        if !self.has_formatter_cell() && !executed_field_word {
            return;
        }
        if let Some(field) = self.take_no_break_field() {
            self.continue_no_break_definition_field(field);
            return;
        }
        // The first .mc flush is not yet a NoBreakField, but it still runs
        // term_fill() before changing NOBREAK/NOSPACE.
        self.discard_unprinted_definition_field_output();
        if self.no_break_definition_field() {
            return;
        }
        self.flush_zero_advance();
        self.execution.boundary = PendingBoundary::Ordinary;
        self.execution.empty_word = false;
        if let TrailingOutput::BreakableBlank(count) = self.execution.trailing_output {
            trim_trailing_breakable_spaces(&mut self.nodes, count);
        }
        self.execution.trailing_output = TrailingOutput::None;
        // An invisible formatter word still advanced `p->col`. Under `.mc`'s
        // `TERMP_NOBREAK` flush, a continued following word starts after that
        // cell even though there is no glyph to carry the distance in IR.
        // A `TERMP_NOBREAK` field owns one committed separator before the next
        // field.  It is formatter geometry, not a cancellable word boundary:
        // `.Sm off`, `.Ns`, and delimiter flags may suppress an additional
        // automatic blank but cannot move the already flushed field back.
        // `term_fill()` and `term_field()` discard trailing breakable word
        // padding before committing a NOBREAK field.  The field separator is
        // different formatter geometry: it survives `.Sm off`, `.Ns`, and
        // delimiter flags and replaces the next word's automatic boundary.
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 1;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.formatter_column = FormatterColumn::Origin;
        // `TERMP_NOBREAK` only changes this flush; it does not create
        // TERMP_NONEWLINE.  Preserve any already-executed `\c` continuation,
        // while releasing its tight word boundary like CVS clears NOSPACE.
        self.execution.final_word_join = Some(false);
    }

    /// Flush another formatter cell while a prior `.mc` field remains live.
    ///
    /// CVS clears NOBREAK and NOSPACE after each request, but BRIND/HANG,
    /// trailspace, and the list field geometry survive.  Consequently a later
    /// `.mc` must not fall back to the ordinary one-cell path merely because
    /// `AuthorBreakEffect` changed after the first flush.
    fn continue_no_break_definition_field(&mut self, mut field: NoBreakField) {
        let resumed_has_cell = self.restore_no_break_field_projection(field);
        if field.style == DefinitionFieldStyle::Hang {
            self.definition_state_mut()
                .hang_row
                .flush(field.trailspace_cells);
            self.definition_state_mut().hang_row.margin_flush_seen = true;
        }
        if !resumed_has_cell {
            // Whitespace-only and zero-width formatter words make
            // `term_flushln()` run, but `term_fill()` commits no field.
            // Restore the one separator that was waiting for the next real
            // field instead of consuming it or manufacturing a second one.
            field.output_end_before_separator = self.nodes.len();
            self.append_field_separator(field.separator_cells);
            self.execution.boundary = PendingBoundary::Tight;
            field.resumed_output_start = self.nodes.len();
            field.resumed_execution_epoch = self.execution.execution_epoch;
            self.definition_state_mut().no_break = Some(field);
            self.reset_after_no_break_field();
            // The complete native field boundary is already represented by
            // the retained separator.  Do not let the block/source handoff
            // append a second ordinary word blank before the next field.
            self.execution.final_word_join = Some(true);
            return;
        }

        let resumed = self
            .nodes
            .get(field.resumed_output_start..)
            .unwrap_or_default();
        let resumed_width = mant_ir::geometry::text_width(&super::super::plain_text(resumed));
        let row_width = self.current_formatter_row_width();
        let overrun = row_width.saturating_add(field.trailspace_cells) > field.body_width;
        let output_end_before_separator = self.nodes.len();

        let separator_cells = if field.style == DefinitionFieldStyle::Tag && overrun {
            self.hard_break();
            // `roff_term_pre_mc()` clears NOSPACE after the NOBREAK flush,
            // so the first word on the new device row owns one ordinary
            // boundary. No word has written that cell yet: HEAD post may
            // close the empty buffer without printing another row.
            1
        } else if overrun {
            1
        } else {
            field.trailspace_cells.saturating_add(1)
        };
        if field.style == DefinitionFieldStyle::Tag && overrun {
            self.execution.pending_field_spaces = separator_cells;
        } else {
            self.append_field_separator(separator_cells);
        }
        self.execution.boundary = PendingBoundary::Tight;

        field.output_end_before_separator = output_end_before_separator;
        field.resumed_output_start = self.nodes.len();
        field.resumed_execution_epoch = self.execution.execution_epoch;
        field.field_width = resumed_width;
        field.separator_cells = separator_cells;
        self.definition_state_mut().no_break = Some(field);
        self.reset_after_no_break_field();
        if field.style == DefinitionFieldStyle::Tag && overrun {
            // reset_after_no_break_field() clears the previous field's
            // buffered geometry. The new row's separator belongs to its
            // *next* term_word(), so carry only that new pending cell on.
            self.execution.pending_field_spaces = separator_cells;
        }
    }

    fn reset_after_no_break_field(&mut self) {
        self.execution.empty_word = false;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.final_word_join = Some(false);
    }

    fn current_formatter_row_width(&self) -> usize {
        let start = self
            .nodes
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak { .. }))
            .map_or(0, |index| index + 1);
        mant_ir::geometry::text_width(&super::super::plain_text(&self.nodes[start..]))
    }

    fn append_field_separator(&mut self, count: usize) {
        self.append_fixed_cells(count);
        if count > 0 {
            self.execution.trailing_output = TrailingOutput::FieldBlank(count);
        }
    }

    fn no_break_definition_field(&mut self) -> bool {
        let Some((start, gap, body, field_width_columns, flags)) = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            })
        else {
            return false;
        };
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        let width = mant_ir::geometry::text_width(&super::super::plain_text(field));
        if !flags.wraps() {
            self.definition_state_mut().hang_row.flush(usize::from(gap));
            self.definition_state_mut().hang_row.margin_flush_seen = true;
        }
        let overrun = width.saturating_add(usize::from(gap)) > usize::from(body);
        let output_end_before_separator = self.nodes.len();
        if flags.wraps() {
            if overrun {
                self.hard_break();
                // Clearing NOSPACE leaves a pending boundary for the next
                // term_word(), not an occupied row before HEAD post.
                self.execution.pending_field_spaces = 1;
                self.execution.boundary = PendingBoundary::Tight;
            } else {
                self.append_field_separator(usize::from(gap).saturating_add(1));
                self.execution.boundary = PendingBoundary::Tight;
            }
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_field_exited();
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
                execution.break_effect = AuthorBreakEffect::Line;
            }
        } else {
            // HANG prevents the flush from ending the physical row.  A field
            // that still has room retains trailspace plus the ordinary next
            // word boundary; an overrun field retains only that boundary.
            self.execution.boundary = PendingBoundary::Ordinary;
            self.execution.pending_field_spaces = if overrun {
                1
            } else {
                usize::from(gap).saturating_add(1)
            };
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
            }
        }
        self.definition_state_mut().no_break = Some(NoBreakField {
            output_end_before_separator,
            resumed_output_start: self.nodes.len(),
            resumed_execution_epoch: self.execution.execution_epoch,
            field_width: width,
            field_capacity_columns: field_width_columns,
            body_width: usize::from(body),
            trailspace_cells: usize::from(gap),
            separator_cells: if overrun {
                1
            } else {
                usize::from(gap).saturating_add(1)
            },
            style: if flags.wraps() {
                DefinitionFieldStyle::Tag
            } else {
                DefinitionFieldStyle::Hang
            },
        });
        self.execution.empty_word = false;
        self.execution.pending_breakable_spaces = 0;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.final_word_join = Some(false);
        true
    }

    fn take_no_break_field(&mut self) -> Option<NoBreakField> {
        if self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.no_break.is_some())
        {
            // Every request that flushes the field after .mc uses this
            // entrypoint. Consume term_fill()'s accepted prefix and reject
            // its pending suffix before .br/.sp/.ce or another .mc can move
            // the output owner or reset field flags.
            self.discard_unprinted_definition_field_output();
        }
        let field = self
            .execution
            .definition
            .as_mut()
            .and_then(|state| state.no_break.take());
        if field.is_some()
            && let Some(state) = &mut self.execution.definition
        {
            // This request itself ran term_flushln() for the field, so the
            // item post drain no longer owes the run-in continuation a
            // separate decision.
            state.run_in_continuation = false;
        }
        field
    }

    fn restore_no_break_field_projection(&mut self, field: NoBreakField) -> bool {
        // A prior `.mc` can leave the device row occupied (`viscol > 0`) even
        // after the current field buffer was reset.  Every control reaching
        // this path therefore executes a real `term_flushln()`: settle a
        // completed zero-advance glyph and, critically, clear a bare
        // BACKAFTER request before the next word runs.
        self.flush_zero_advance();
        let resumed = self
            .nodes
            .get(field.resumed_output_start..)
            .unwrap_or_default();
        let resumed_text = super::super::plain_text(resumed);
        // `term_fill()` commits a fixed/non-breaking blank glyph, but drops
        // an ordinary whitespace-only formatter word.  Both occupy the Rust
        // projection, so printable text alone cannot distinguish them.
        let resumed_has_cell = resumed_text.chars().any(|ch| !ch.is_whitespace())
            || (has_printable_character(resumed)
                && self.execution.trailing_output == TrailingOutput::FixedBlank);
        if !resumed_has_cell {
            trim_trailing_breakable_spaces(&mut self.nodes, field.separator_cells);
            let mut index = self.nodes.len();
            while index > field.output_end_before_separator {
                index -= 1;
                if matches!(&self.nodes[index], Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } if value.chars().all(char::is_whitespace))
                {
                    self.nodes.remove(index);
                }
            }
            self.execution.last_visible_character = last_visible_character(&self.nodes);
            self.execution.trailing_output = if self.execution.last_visible_character.is_some() {
                TrailingOutput::NonBlank
            } else {
                TrailingOutput::None
            };
        }
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        resumed_has_cell
    }

    fn settle_no_break_field_line(&mut self, field: NoBreakField, row_indent: u16) {
        // Sample before the restore: its flush settles a pending
        // zero-advance glyph and thereby closes the row (mdoc_term.c:1085).
        let zero_pending = self.execution.zero_advance.has_pending_glyph();
        let resumed_visible = self.restore_no_break_field_projection(field);
        match field.style {
            DefinitionFieldStyle::Tag => {
                // roff_term.c:73-75: a fill-mode boundary (`.nf`/`.fi`, the
                // same pre_br dispatch, roff_term.c:52) moves the row origin
                // to the field's right margin; the `.br` family already
                // materializes it through the pending-indent arm below.
                if row_indent > 0 {
                    self.definition_state_mut().row.indent_columns = row_indent;
                }
                self.force_output_line_break();
                if !resumed_visible {
                    self.definition_state_mut().pending_indent = Some(field.body_width);
                }
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_field_exited();
            }
            DefinitionFieldStyle::Hang => {
                // A pending zero-advance glyph settles through the
                // boundary's term_newln() and closes the row
                // (mdoc_term.c:1085), so no jump remains.
                let row_open = self.execution.definition.as_ref().is_some_and(|state| {
                    state.hang_row.viscol > 0
                        || state.field_buffer.cells().iter().any(|cell| {
                            matches!(
                                cell,
                                super::field_buffer::FieldCell::Graph { .. }
                                    | super::field_buffer::FieldCell::NonBreakingBlank
                            )
                        })
                }) && !zero_pending;
                if row_open && row_indent > 0 {
                    // HANG kept the row open (roff_term.c:76 clears NOBREAK
                    // and BRIND only), so the next word does not break: it
                    // jumps to the field's right margin through the same
                    // `vbl = offset - viscol` fill (term.c:113-114).
                    let state = self.definition_state_mut();
                    state.row.arm_jump(row_indent);
                } else if !resumed_visible {
                    self.definition_state_mut().pending_indent =
                        Some(field.body_width.saturating_sub(field.field_width).max(1));
                }
                self.execution.boundary = PendingBoundary::Tight;
            }
        }
        self.finish_definition_field_control(field, 0, true);
    }

    fn finish_definition_field_control(
        &mut self,
        field: NoBreakField,
        hang_gap_cells: u8,
        consume_body_gap: bool,
    ) {
        if field.style == DefinitionFieldStyle::Hang {
            let had_cell = self.has_formatter_cell();
            let definition = self.definition_state_mut();
            if had_cell || definition.hang_row.viscol > 0 {
                definition.hang_row.flush(usize::from(hang_gap_cells));
            }
            definition.hang_row.field_offset = field.body_width;
        }
        if consume_body_gap {
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_body_gap_consumed();
        }
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            execution.break_effect = match field.style {
                DefinitionFieldStyle::Tag => AuthorBreakEffect::Line,
                DefinitionFieldStyle::Hang => AuthorBreakEffect::Field {
                    gap_cells: hang_gap_cells,
                    body_width_columns: u16::try_from(field.body_width).unwrap_or(u16::MAX),
                    field_width_columns: field.field_capacity_columns,
                    flags: FieldFlags::hang(),
                },
            };
        }
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.empty_word = false;
        self.execution.final_word_join = Some(false);
    }

    /// Collapse a jump still uncommitted at the item post: the element
    /// restore already zeroed the offset (mdoc_term.c:437-439), so the
    /// buffered word prints with `vbl = 0`.
    pub(in crate::mandoc) fn retract_head_close_jump(&mut self) {
        if let Some(definition) = &mut self.execution.definition
            && let Some(jump_node) = definition.row.retract_on_head_close()
            && let Some(Inline::Text { value }) = self.nodes.get_mut(jump_node)
        {
            value.clear();
        }
    }

    /// Take the row indent a fill-mode boundary left behind. The indent
    /// lives for one row break: the document node boundary upstream
    /// restores the authored geometry (mdoc_term.c:329-330, 437-439).
    pub(in crate::mandoc) fn take_definition_row_indent(&mut self) -> u16 {
        let Some(definition) = &mut self.execution.definition else {
            return 0;
        };
        std::mem::replace(&mut definition.row.indent_columns, 0)
    }

    fn force_output_line_break(&mut self) {
        // Consume the pending row indent even when a break already sits at
        // the tail: the boundary moved the upstream row origin regardless
        // (roff_term.c:73-75), and a leaked indent would misplace a later
        // row.
        let row_indent = self.take_definition_row_indent();
        if matches!(self.nodes.last(), Some(Inline::LineBreak { .. })) {
            return;
        }
        self.flush_zero_advance();
        self.nodes.push(Inline::line_break_indented(row_indent));
        self.execution.last_visible_character = Some('\n');
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.boundary = PendingBoundary::Ordinary;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        self.execution.formatter_column = FormatterColumn::Origin;
    }
}

fn retain_unprinted_field_targets(inlines: &mut Vec<Inline>) {
    inlines.retain_mut(|inline| match inline {
        Inline::Anchor { .. } => true,
        // term_fill() returned nbr=0: a buffered \p line request in this
        // field never reached the device, even inside a semantic Link.
        Inline::LineBreak { .. }
        | Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Equation { .. } => false,
        Inline::Link { children, .. } => {
            retain_unprinted_field_targets(children);
            true
        }
        Inline::Strong { children } | Inline::Emphasis { children } => {
            retain_unprinted_field_targets(children);
            !children.is_empty()
        }
    });
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

#[cfg(test)]
mod head_row_state_tests {
    use super::HeadRowState;

    // Every case below pins a print-deferral fact verified against the
    // fixed CVS reference by the engine roff_lowering contracts
    // (definition_fields.rs: hang fill boundary fit/overrun, the bare-zero
    // handoff, and the wrapping probes); these unit tests keep the state
    // machine's own lifecycle observable.

    #[test]
    fn a_later_word_commits_the_jump() {
        // LONGTEXT .mc .nf A An -split An Bob: 'A' emits the fill, 'Bob's
        // row event prints it before the restore — the fill stands.
        let mut row = HeadRowState::default();
        row.arm_jump(14);
        assert_eq!(row.emit_armed(0, 8), 6);
        row.close_word();
        row.commit_on_later_word();
        assert_eq!(row.retract_on_head_close(), None);
    }

    #[test]
    fn the_item_post_retracts_an_uncommitted_jump() {
        // LONGTEXT .mc .No \z .nf An -split An Bob: 'Bob' is the only word
        // after the boundary; the item post prints it past the element
        // restore (mdoc_term.c:437-439) and the fill collapses.
        let mut row = HeadRowState::default();
        row.arm_jump(14);
        assert_eq!(row.emit_armed(0, 8), 6);
        row.close_word();
        assert_eq!(row.retract_on_head_close(), Some(0));
    }

    #[test]
    fn a_width_exact_head_emits_no_fill() {
        // 6n == len(LONGTEXT): offset == viscol at print time, vbl = 0.
        let mut row = HeadRowState::default();
        row.arm_jump(8);
        assert_eq!(row.emit_armed(0, 8), 0);
        assert_eq!(row.retract_on_head_close(), None);
    }

    #[test]
    fn an_undelivered_arm_does_not_retract_text() {
        // Nothing emitted: no node index exists to clear.
        let mut row = HeadRowState::default();
        row.arm_jump(6);
        assert_eq!(row.emit_armed(0, 8), 0);
        row.close_word();
        assert_eq!(row.retract_on_head_close(), None);
    }
}
