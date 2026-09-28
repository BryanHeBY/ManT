use super::{
    AuthorBreakEffect, DefinitionFieldStyle, FormatterColumn, HangRowTransition, Inline,
    InlineBuilder, NoBreakField, PendingBoundary, PendingFieldGapOrigin, TrailingOutput,
    WordEndBreak, has_printable_character, last_visible_character, trim_trailing_breakable_spaces,
};

impl InlineBuilder {
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
                    wraps,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    wraps,
                )),
                AuthorBreakEffect::Line => None,
            });
        if let Some((start, gap, body, wraps)) = field {
            self.flush_definition_field(start, gap, body, wraps, false);
            if self.execution.pending_field_spaces > 0 {
                self.definition_state_mut().pending_gap_origin = PendingFieldGapOrigin::SourceLine;
            }
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
            self.settle_no_break_field_line(field);
            return true;
        }
        let Some((start, gap, body, wraps)) =
            self.execution
                .author_execution
                .as_ref()
                .and_then(|execution| match execution.break_effect {
                    AuthorBreakEffect::Field {
                        gap_cells,
                        body_width_columns,
                        wraps,
                    } => Some((
                        execution.field_output_start,
                        gap_cells,
                        body_width_columns,
                        wraps,
                    )),
                    AuthorBreakEffect::Line => None,
                })
        else {
            let had_cell = self.has_formatter_cell();
            self.hard_break();
            return had_cell;
        };
        self.flush_definition_field(start, gap, body, wraps, true)
    }

    // The NOBREAK flush commits field, row, and BRIND state in native order.
    #[allow(clippy::too_many_lines)]
    pub(super) fn flush_definition_field(
        &mut self,
        field_output_start: usize,
        gap_cells: u8,
        body_width_columns: u16,
        wraps: bool,
        exit_field: bool,
    ) -> bool {
        let pending_native_gap = self.execution.pending_field_spaces > 0;
        let native_field_discarded = self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.field_discarded);
        let native_field_printable = !wraps
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
                    if wraps {
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
                            wraps,
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
        } else if wraps {
            has_printable_character(field)
        } else {
            native_field_printable
        };
        let field_width = mant_ir::geometry::text_width(&super::super::plain_text(field));
        if !wraps && !field_is_printable {
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
            && wraps
            && field_width.saturating_add(usize::from(gap_cells)) > body_width;

        let mut deferred_field_cells = 0;
        if overruns {
            self.hard_break();
            if exit_field {
                self.definition_state_mut().pending_indent = Some(body_width);
            }
        } else if field_is_printable {
            let cells = if exit_field && wraps {
                body_width.saturating_sub(field_width)
            } else if exit_field {
                body_width
                    .saturating_sub(field_width)
                    .max(usize::from(gap_cells))
            } else {
                usize::from(gap_cells)
            };
            if !exit_field && !wraps {
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
            self.definition_state_mut().pending_indent = Some(body_width);
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_body_gap_consumed();
        } else {
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
            if exit_field && wraps {
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
                    wraps,
                };
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
                wraps: false,
            }) = self
                .execution
                .author_execution
                .as_ref()
                .map(|author| author.break_effect)
        {
            // At HEAD post, term_newln() flushes the final HANG field. Only
            // a row that has reached BODY's origin and has no trailing field
            // space has consumed its separator. An active trailspace remains
            // `minbl` for the BODY even when the HEAD overran its margin.
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
        if let Some(definition) = &mut self.execution.definition {
            definition.hang_row.field_native_graph = true;
        }
    }

    /// `term.c::term_fill()` stops at a breakable blank after `\p`. When the
    /// field has not supplied a graph yet, even later words in that field are
    /// never printed. The text decoder reports this event inside one word;
    /// `HangNativeRow::word()` handles the same event across words.
    pub(in crate::mandoc) fn note_hang_break_before_graph(&mut self) {
        if let Some(definition) = &mut self.execution.definition {
            let row = &mut definition.hang_row;
            row.field_discarded |= !row.field_native_graph;
            row.field_unproven_break = true;
        }
    }

    pub(in crate::mandoc) fn discard_unprinted_definition_field_output(&mut self) {
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
        // term.c::term_flushln() clears both BACKAFTER and BACKBEFORE even
        // when term_fill() returns nbr=0. The rejected field can still own a
        // completed \z glyph that has not entered the IR suffix yet.
        self.execution.zero_advance.discard_at_row_end();
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
        let Some((start, gap, body, wraps)) =
            self.execution
                .author_execution
                .as_ref()
                .and_then(|execution| match execution.break_effect {
                    AuthorBreakEffect::Field {
                        gap_cells,
                        body_width_columns,
                        wraps,
                    } => Some((
                        execution.field_output_start,
                        gap_cells,
                        body_width_columns,
                        wraps,
                    )),
                    AuthorBreakEffect::Line => None,
                })
        else {
            self.control_line_break();
            return;
        };
        if !self.has_formatter_cell() {
            self.flush_definition_field(start, gap, body, wraps, true);
            return;
        }
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        if !has_printable_character(field) {
            self.flush_definition_field(start, gap, body, wraps, true);
            return;
        }
        let field_width = mant_ir::geometry::text_width(&super::super::plain_text(field));
        let overruns = wraps && field_width.saturating_add(usize::from(gap)) > usize::from(body);
        if overruns {
            self.hard_break();
        } else {
            self.append_fixed_cells(usize::from(gap));
        }
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            if wraps {
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
                    wraps,
                };
            }
        }
        self.execution.boundary = PendingBoundary::Tight;
    }

    /// Enter or leave no-fill mode at a physical source-line boundary.
    /// `print_mdoc_node()` performs that boundary in addition to the request's
    /// own `roff_term_pre_br()` dispatch.
    pub(in crate::mandoc) fn fill_mode_boundary(&mut self) {
        if let Some(field) = self.take_no_break_field() {
            self.settle_no_break_field_line(field);
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_field_exited();
            return;
        }
        let Some((start, gap, body, wraps)) =
            self.execution
                .author_execution
                .as_ref()
                .and_then(|execution| match execution.break_effect {
                    AuthorBreakEffect::Field {
                        gap_cells,
                        body_width_columns,
                        wraps,
                    } => Some((
                        execution.field_output_start,
                        gap_cells,
                        body_width_columns,
                        wraps,
                    )),
                    AuthorBreakEffect::Line => None,
                })
        else {
            self.control_line_break();
            return;
        };
        if !self.has_formatter_cell() {
            return;
        }
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        if !has_printable_character(field) {
            self.flush_definition_field(start, gap, body, wraps, true);
            return;
        }
        if wraps {
            self.hard_break();
            self.definition_state_mut().pending_indent = Some(usize::from(body));
        } else {
            let width = mant_ir::geometry::text_width(&super::super::plain_text(field));
            self.append_fixed_cells(usize::from(body).saturating_sub(width));
        }
        self.execution
            .definition
            .as_mut()
            .expect("definition field session")
            .outcome
            .mark_field_exited();
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            execution.break_effect = if wraps {
                AuthorBreakEffect::Line
            } else {
                AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns: body,
                    wraps,
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
            self.control_line_break();
            self.execution.final_word_join = Some(false);
            self.execution.final_source_continuation = Some(false);
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
                    wraps,
                    ..
                } => Some((body_width_columns, wraps)),
                AuthorBreakEffect::Line => None,
            });
        if let Some((body_width_columns, wraps)) = field {
            if !self.has_formatter_cell() {
                // `term_vspace()` always emits its requested empty row, but
                // its leading `term_newln()` leaves a bare BACKAFTER armed
                // when neither tcol nor viscol is occupied.  The following
                // BRIND phase still ends a tag field; HANG keeps its run-in
                // body contract.  Preserve those independent effects.
                self.retain_line_breaks(rows);
                self.execution.boundary = PendingBoundary::Tight;
                if wraps {
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
                wraps && width.saturating_add(trailspace) > usize::from(body_width_columns);
            self.hard_break();
            self.retain_line_breaks(if term_newln_ended_row {
                rows
            } else {
                rows.saturating_sub(1)
            });
            self.definition_state_mut().pending_indent = Some(usize::from(body_width_columns));
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
                if wraps {
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
                        wraps,
                    };
                }
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
        if rows == 0 {
            // roff_term_pre_sp() skips term_vspace() for zero rows, then runs
            // roff_term_pre_br(). HANG's term_flushln() retains the same
            // physical row; only a positive vertical request ends it.
            self.settle_no_break_field_line(field);
            self.execution.final_word_join = Some(false);
            self.execution.final_source_continuation = Some(false);
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
        // TERMP_NOBREAK flush, a continued following word starts after that
        // cell even though there is no glyph to carry the distance in IR.
        // A TERMP_NOBREAK field owns one committed separator before the next
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
        // TERMP_NOBREAK only changes this flush; it does not create
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
            // so the first word on the new device row still owns one normal
            // formatter boundary at the list origin.
            1
        } else if overrun {
            1
        } else {
            field.trailspace_cells.saturating_add(1)
        };
        self.append_field_separator(separator_cells);
        self.execution.boundary = PendingBoundary::Tight;

        field.output_end_before_separator = output_end_before_separator;
        field.resumed_output_start = self.nodes.len();
        field.resumed_execution_epoch = self.execution.execution_epoch;
        field.field_width = resumed_width;
        field.separator_cells = separator_cells;
        self.definition_state_mut().no_break = Some(field);
        self.reset_after_no_break_field();
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
            .rposition(|node| matches!(node, Inline::LineBreak))
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
        let Some((start, gap, body, wraps)) =
            self.execution
                .author_execution
                .as_ref()
                .and_then(|execution| match execution.break_effect {
                    AuthorBreakEffect::Field {
                        gap_cells,
                        body_width_columns,
                        wraps,
                    } => Some((
                        execution.field_output_start,
                        gap_cells,
                        body_width_columns,
                        wraps,
                    )),
                    AuthorBreakEffect::Line => None,
                })
        else {
            return false;
        };
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        let width = mant_ir::geometry::text_width(&super::super::plain_text(field));
        if !wraps {
            self.definition_state_mut().hang_row.flush(usize::from(gap));
            self.definition_state_mut().hang_row.margin_flush_seen = true;
        }
        let overrun = width.saturating_add(usize::from(gap)) > usize::from(body);
        let output_end_before_separator = self.nodes.len();
        if wraps {
            if overrun {
                self.hard_break();
                // Clearing NOSPACE after `.mc` leaves one ordinary boundary
                // at the list origin; BRIND is not executed by this request.
                self.append_field_separator(1);
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
            body_width: usize::from(body),
            trailspace_cells: usize::from(gap),
            separator_cells: if overrun {
                1
            } else {
                usize::from(gap).saturating_add(1)
            },
            style: if wraps {
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
        self.execution
            .definition
            .as_mut()
            .and_then(|state| state.no_break.take())
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

    fn settle_no_break_field_line(&mut self, field: NoBreakField) {
        let resumed_visible = self.restore_no_break_field_projection(field);
        match field.style {
            DefinitionFieldStyle::Tag => {
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
                if !resumed_visible {
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
                    wraps: false,
                },
            };
        }
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.empty_word = false;
        self.execution.final_word_join = Some(false);
    }

    fn force_output_line_break(&mut self) {
        if matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            return;
        }
        self.flush_zero_advance();
        self.nodes.push(Inline::LineBreak);
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
        Inline::LineBreak | Inline::Text { .. } | Inline::Code { .. } | Inline::Equation { .. } => {
            false
        }
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
