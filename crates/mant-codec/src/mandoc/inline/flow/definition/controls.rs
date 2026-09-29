use super::super::native_field::{FieldFlag, FieldFlags};
use super::super::{
    AuthorBreakEffect, FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput,
    WordEndBreak, has_printable_character, last_visible_character,
};
use super::flush::retain_unprinted_field_targets;
use super::state::{DefinitionFieldStyle, HangRowTransition, NoBreakField, PendingFieldGapOrigin};

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
    pub(super) fn note_field_control_cleared_no_break(&mut self, brind: bool, capacity: u16) {
        if !brind {
            return;
        }
        let definition = self.definition_state_mut();
        definition.no_break_cleared = true;
        definition.cleared_field_capacity_columns = capacity;
    }

    /// `term.c::encode1()` buffers the character following `\\z` in the
    /// current field. It has not reached an IR node yet, but `term_fill()` and
    /// `term_field()` still count its printed width at a field boundary.
    pub(super) fn pending_hang_glyph_width(&self) -> Option<usize> {
        let mut zero_advance = self.execution.zero_advance.clone();
        let mut pending = Vec::new();
        zero_advance.finish_into(&mut pending);
        let text = super::super::super::plain_text(&pending);
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

    /// Whether an author-split row end restarted the head field (its
    /// breaks do not close the head before BODY).
    pub(in crate::mandoc) fn definition_author_restarted(&self) -> bool {
        self.execution
            .definition
            .as_ref()
            .is_some_and(|state| state.outcome.is_field_restarted())
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

    pub(in crate::mandoc::inline::flow) fn append_fixed_cells(&mut self, count: usize) {
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
        let field_width = mant_ir::geometry::text_width(&super::super::super::plain_text(field));
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
            let width = mant_ir::geometry::text_width(&super::super::super::plain_text(field));
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
            let width = mant_ir::geometry::text_width(&super::super::super::plain_text(
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

    pub(super) fn finish_native_vertical_row(&mut self, rows: usize) {
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

    pub(super) fn finish_definition_field_control(
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

    pub(in crate::mandoc) fn force_output_line_break(&mut self) {
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
