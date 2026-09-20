use super::{
    AuthorBreakEffect, DefinitionFieldStyle, Inline, InlineBuilder, NoBreakField, PendingBoundary,
    SeparatorKind, TrailingOutput, WordEndBreak, trim_trailing_breakable_spaces,
};

impl InlineBuilder {
    /// Execute CVS `term_newln()` without the additional BRIND transition
    /// performed by `roff_term_pre_br()`.
    ///
    /// In particular, `termp_an_pre()` uses this operation for `.An -split`.
    /// Keeping it distinct from [`Self::control_line_break`] is required:
    /// `.An` may flush the current field, but it must not move the next field
    /// to the definition body margin or clear NOBREAK/BRIND.
    pub(in crate::mandoc) fn conditional_line_break(&mut self) -> bool {
        let had_row = self.has_active_formatter_row();
        if had_row {
            self.prepare_native_flush();
        }
        let outcome = self.formatter_line.term_newline();
        if had_row {
            self.reset_projection_after_flush();
        }
        if let Some(outcome) = outcome {
            self.apply_flush_outcome(outcome);
        }
        self.boundary = PendingBoundary::Tight;
        had_row
    }

    /// Execute an explicit formatter line request inside a definition HEAD.
    ///
    /// CVS keeps `LIST_tag/LIST_hang` in a NOBREAK field until `term_newln()`
    /// has settled that field.  A plain `hard_break()` loses BRIND geometry,
    /// so `.br`, `.ti`, and the break phase of `.sp` must use this entrypoint.
    pub(in crate::mandoc) fn control_line_break(&mut self) -> bool {
        let Some((body_width_columns, wraps)) =
            self.author_execution
                .as_ref()
                .and_then(|execution| match execution.break_effect {
                    AuthorBreakEffect::Field {
                        body_width_columns,
                        wraps,
                        ..
                    } => Some((body_width_columns, wraps)),
                    AuthorBreakEffect::Line => None,
                })
        else {
            let had_cell = self.has_formatter_cell();
            self.hard_break();
            return had_cell;
        };
        let had_row = self.has_active_formatter_row();
        if had_row {
            self.prepare_native_flush();
        }
        let outcome = self.formatter_line.roff_break();
        if had_row {
            self.reset_projection_after_flush();
        }
        if let Some(outcome) = outcome {
            self.apply_flush_outcome(outcome);
        }
        if wraps {
            self.definition_outcome.mark_field_exited();
            if let Some(execution) = &mut self.author_execution {
                execution.break_effect = AuthorBreakEffect::Line;
            }
        } else {
            self.definition_outcome.mark_body_gap_consumed();
            if let Some(execution) = &mut self.author_execution {
                execution.break_effect = AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns,
                    wraps,
                };
            }
        }
        self.boundary = PendingBoundary::Tight;
        had_row
    }

    pub(super) fn flush_definition_field(
        &mut self,
        gap_cells: u8,
        body_width_columns: u16,
        wraps: bool,
        exit_field: bool,
    ) -> bool {
        if !self.has_formatter_cell() {
            // `roff_term_pre_br()` applies BRIND even when `term_newln()` had
            // no tcol bytes or device row to flush.  It moves the next word
            // to the body margin and clears NOBREAK/BRIND.  HANG itself
            // survives, so a hang head and its body remain on that same row;
            // a tag head instead finishes as an ordinary line field.
            if exit_field {
                self.pending_definition_indent = Some(usize::from(body_width_columns));
                self.boundary = PendingBoundary::Tight;
                if let Some(execution) = &mut self.author_execution {
                    if wraps {
                        self.definition_outcome.mark_field_exited();
                        execution.break_effect = AuthorBreakEffect::Line;
                    } else {
                        self.definition_outcome.mark_body_gap_consumed();
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
        let field_is_printable = self.formatter_line.buffer_is_visible();
        let field_width = self.formatter_line.buffer_width();
        let body_width = usize::from(body_width_columns);
        let overruns = field_is_printable
            && wraps
            && field_width.saturating_add(usize::from(gap_cells)) > body_width;

        if overruns {
            self.hard_break();
            if exit_field {
                self.pending_definition_indent = Some(body_width);
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
            self.append_fixed_cells(cells);
        } else if exit_field {
            // An explicit empty word and `\&` still execute the NOBREAK
            // field.  There is no row to close, but `roff_term_pre_br()`
            // applies BRIND before the following word.
            self.pending_definition_indent = Some(body_width);
            self.definition_outcome.mark_body_gap_consumed();
        } else {
            self.trailing_output = TrailingOutput::None;
        }

        self.boundary = PendingBoundary::Tight;
        self.empty_word = false;
        self.pending_breakable_spaces = 0;
        self.formatter_line.clear_separators();
        self.word_end_break = WordEndBreak::Clear;
        if overruns {
            self.formatter_line.end_row();
        } else {
            self.formatter_line.commit_field();
        }
        if let Some(execution) = &mut self.author_execution {
            if exit_field && wraps {
                self.definition_outcome.mark_field_exited();
                execution.break_effect = AuthorBreakEffect::Line;
            } else if exit_field {
                // TERMP_HANG survives `term_newln()`, but the generated field
                // gap has already been emitted for this request.
                self.definition_outcome.mark_body_gap_consumed();
                execution.break_effect = AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns,
                    wraps,
                };
            }
        }
        overruns
    }

    pub(in crate::mandoc) const fn definition_field_exited(&self) -> bool {
        self.definition_outcome.field_exited()
    }

    pub(in crate::mandoc) const fn definition_body_gap_consumed(&self) -> bool {
        self.definition_outcome.body_gap_consumed()
    }

    pub(super) fn append_fixed_cells(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.append_projected(vec![Inline::Text {
            value: " ".repeat(count),
        }]);
        self.trailing_output = TrailingOutput::FixedBlank(count);
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
            self.settle_no_break_field_line(field, false);
            return;
        }
        let Some((gap, body, wraps)) =
            self.author_execution
                .as_ref()
                .and_then(|execution| match execution.break_effect {
                    AuthorBreakEffect::Field {
                        gap_cells,
                        body_width_columns,
                        wraps,
                    } => Some((gap_cells, body_width_columns, wraps)),
                    AuthorBreakEffect::Line => None,
                })
        else {
            self.control_line_break();
            return;
        };
        if !self.has_formatter_cell() {
            self.flush_definition_field(gap, body, wraps, true);
            return;
        }
        self.flush_zero_advance();
        if !self.formatter_line.buffer_is_visible() {
            self.flush_definition_field(gap, body, wraps, true);
            return;
        }
        let field_width = self.formatter_line.buffer_width();
        let overruns = wraps && field_width.saturating_add(usize::from(gap)) > usize::from(body);
        if overruns {
            self.hard_break();
        } else {
            self.append_fixed_cells(usize::from(gap));
        }
        if let Some(execution) = &mut self.author_execution {
            if wraps {
                self.definition_outcome.mark_field_exited();
                execution.break_effect = AuthorBreakEffect::Line;
            } else {
                self.definition_outcome.mark_body_gap_consumed();
                execution.break_effect = AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns: body,
                    wraps,
                };
            }
        }
        self.boundary = PendingBoundary::Tight;
    }

    /// Enter or leave no-fill mode at a physical source-line boundary.
    /// `print_mdoc_node()` performs that boundary in addition to the request's
    /// own `roff_term_pre_br()` dispatch.
    pub(in crate::mandoc) fn fill_mode_boundary(&mut self) {
        if let Some(field) = self.take_no_break_field() {
            self.settle_no_break_field_line(field, true);
            self.definition_outcome.mark_field_exited();
            return;
        }
        let Some((gap, body, wraps)) =
            self.author_execution
                .as_ref()
                .and_then(|execution| match execution.break_effect {
                    AuthorBreakEffect::Field {
                        gap_cells,
                        body_width_columns,
                        wraps,
                    } => Some((gap_cells, body_width_columns, wraps)),
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
        if !self.formatter_line.buffer_is_visible() {
            self.flush_definition_field(gap, body, wraps, true);
            return;
        }
        if wraps {
            self.hard_break();
            self.pending_definition_indent = Some(usize::from(body));
        } else {
            let width = self.formatter_line.buffer_width();
            self.append_fixed_cells(usize::from(body).saturating_sub(width));
        }
        self.definition_outcome.mark_field_exited();
        if let Some(execution) = &mut self.author_execution {
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
        self.boundary = PendingBoundary::Tight;
    }

    /// Execute an inline vertical-space request without retaining its
    /// numeric operand as document text.  `term_vspace(n)` first closes an
    /// occupied row, then emits `n` empty rows.
    pub(in crate::mandoc) fn vertical_space(&mut self, rows: usize) {
        if let Some(field) = self.take_no_break_field() {
            self.flush_zero_advance();
            let field_accepted = self.formatter_line.commit_field();
            self.formatter_line
                .queue_separator(usize::from(field_accepted), SeparatorKind::WordBoundary);
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
            self.pending_definition_indent = match field.style {
                DefinitionFieldStyle::Tag => None,
                DefinitionFieldStyle::Hang => Some(field.body_width),
            };
            if field.style == DefinitionFieldStyle::Tag {
                self.definition_outcome.mark_field_exited();
            }
            self.finish_definition_field_control(field, 0, true);
            self.final_word_join = Some(false);
            self.final_source_continuation = Some(false);
            return;
        }
        let field =
            self.author_execution
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
                self.boundary = PendingBoundary::Tight;
                if wraps {
                    self.definition_outcome.mark_field_exited();
                    if let Some(execution) = &mut self.author_execution {
                        execution.break_effect = AuthorBreakEffect::Line;
                    }
                }
                self.final_word_join = Some(false);
                self.final_source_continuation = Some(false);
                return;
            }
            // `roff_term_pre_sp()` executes term_vspace() before the final
            // BRIND transition. HANG can suppress term_newln(), but the
            // vertical request still ends the row; field padding is trailing
            // geometry and must not leak onto the empty row.
            self.flush_zero_advance();
            let width = self.formatter_line.buffer_width();
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
            self.pending_definition_indent = Some(usize::from(body_width_columns));
            if let Some(execution) = &mut self.author_execution {
                if wraps {
                    self.definition_outcome.mark_field_exited();
                    execution.break_effect = AuthorBreakEffect::Line;
                } else {
                    self.definition_outcome.mark_body_gap_consumed();
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
        self.final_word_join = Some(false);
        self.final_source_continuation = Some(false);
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
        if !self.formatter_line.cursor_advanced()
            && !self.zero_advance.has_buffered_glyph()
            && !self.word_end_break.eq(&WordEndBreak::Pending)
        {
            return;
        }
        if self.author_execution.as_ref().is_some_and(|execution| {
            matches!(execution.break_effect, AuthorBreakEffect::Field { .. })
        }) {
            self.no_break_definition_flush();
            return;
        }
        let literal_flow = self.source_cursor.is_some();
        let continued = self.final_source_continuation_or(false);
        let realizes_literal_word_end_break =
            literal_flow && self.word_end_break == WordEndBreak::Pending && !continued;
        self.flush_zero_advance();
        let field_was_visible = self.formatter_line.buffer_is_visible();
        // A visible glyph always makes the field printable.  CVS also keeps
        // an otherwise invisible `\&` cell when `\c` suppresses the source
        // line flush; without that continuation, the invisible-only field is
        // discarded before `.mc` and contributes no relative separator.
        if realizes_literal_word_end_break {
            if !field_was_visible {
                self.nodes.push(Inline::Text {
                    value: String::new(),
                });
            }
            self.nodes.push(Inline::LineBreak);
            self.last_visible_character = Some('\n');
            if let Some(cursor) = &mut self.source_cursor {
                cursor.explicit_line_break(false);
            }
        }
        self.boundary = PendingBoundary::Ordinary;
        self.empty_word = false;
        if let TrailingOutput::BreakableBlank(count) = self.trailing_output {
            trim_trailing_breakable_spaces(&mut self.nodes, count);
        }
        self.trailing_output = TrailingOutput::None;
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
        self.pending_breakable_spaces = 0;
        self.word_end_break = WordEndBreak::Clear;
        if realizes_literal_word_end_break {
            self.formatter_line.end_row();
        } else {
            let field_accepted = self
                .formatter_line
                .margin_flush()
                .is_some_and(|outcome| outcome.field_accepted);
            self.formatter_line
                .queue_separator(usize::from(field_accepted), SeparatorKind::WordBoundary);
        }
        // TERMP_NOBREAK only changes this flush; it does not create
        // TERMP_NONEWLINE.  Preserve any already-executed `\c` continuation,
        // while releasing its tight word boundary like CVS clears NOSPACE.
        self.final_word_join = Some(false);
    }

    /// Execute `roff_term_pre_mc()` against the live CVS-shaped field state.
    /// BRIND, HANG, trailspace, minbl and geometry remain in the machine;
    /// there is no secondary field snapshot to reconstruct later.
    fn no_break_definition_flush(&mut self) {
        self.prepare_native_flush();
        let outcome = self.formatter_line.margin_flush();
        self.reset_projection_after_flush();
        if let Some(outcome) = outcome {
            self.apply_flush_outcome(outcome);
        }
        self.boundary = PendingBoundary::Ordinary;
        self.empty_word = false;
        self.pending_breakable_spaces = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.final_word_join = Some(false);
    }

    /// Flush another formatter cell while a prior `.mc` field remains live.
    ///
    /// CVS clears NOBREAK and NOSPACE after each request, but BRIND/HANG,
    /// trailspace, and the list field geometry survive.  Consequently a later
    /// `.mc` must not fall back to the ordinary one-cell path merely because
    /// `AuthorBreakEffect` changed after the first flush.
    fn continue_no_break_definition_field(&mut self, mut field: NoBreakField) {
        let resumed_has_cell =
            self.formatter_line.buffer_is_visible() || self.zero_advance.has_buffered_glyph();
        self.flush_zero_advance();
        if !resumed_has_cell {
            // `term_fill()` commits no field for an empty word or `\&`.
            // `minbl` therefore remains latent for the next real field.
            self.formatter_line.discard_empty_field();
            self.pending_breakable_spaces = 0;
            self.word_end_break = WordEndBreak::Clear;
            self.trailing_output = TrailingOutput::None;
            self.boundary = PendingBoundary::Tight;
            self.no_break_field = Some(field);
            self.final_word_join = Some(true);
            return;
        }

        let resumed_width = self.formatter_line.buffer_width();
        let row_width = self.formatter_line.row_width();
        let overrun = row_width.saturating_add(field.trailspace_cells) > field.body_width;
        let field_accepted = self.formatter_line.commit_field();
        if field.style == DefinitionFieldStyle::Tag && overrun {
            self.force_output_line_break();
        }
        let separator_cells = field.trailspace_cells;
        self.formatter_line
            .queue_separator(separator_cells, SeparatorKind::FieldMinimum);
        self.formatter_line
            .queue_separator(usize::from(field_accepted), SeparatorKind::WordBoundary);
        self.boundary = PendingBoundary::Tight;

        field.field_width = resumed_width;
        self.no_break_field = Some(field);
        self.reset_after_no_break_field();
    }

    fn reset_after_no_break_field(&mut self) {
        self.empty_word = false;
        self.pending_breakable_spaces = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.final_word_join = Some(false);
    }

    fn no_break_definition_field(&mut self) -> bool {
        let Some((gap, body, wraps)) =
            self.author_execution
                .as_ref()
                .and_then(|execution| match execution.break_effect {
                    AuthorBreakEffect::Field {
                        gap_cells,
                        body_width_columns,
                        wraps,
                    } => Some((gap_cells, body_width_columns, wraps)),
                    AuthorBreakEffect::Line => None,
                })
        else {
            return false;
        };
        self.flush_zero_advance();
        let width = self.formatter_line.buffer_width();
        let overrun = width.saturating_add(usize::from(gap)) > usize::from(body);
        if wraps {
            if overrun {
                self.hard_break();
            } else {
                let field_accepted = self.formatter_line.commit_field();
                self.formatter_line
                    .queue_separator(usize::from(field_accepted), SeparatorKind::WordBoundary);
            }
            self.formatter_line
                .queue_separator(usize::from(gap), SeparatorKind::FieldMinimum);
            self.boundary = PendingBoundary::Tight;
            self.definition_outcome.mark_field_exited();
            if let Some(execution) = &mut self.author_execution {
                execution.break_effect = AuthorBreakEffect::Line;
            }
        } else {
            // HANG prevents the flush from ending the physical row.  A field
            // that still has room retains trailspace plus the ordinary next
            // word boundary; an overrun field retains only that boundary.
            self.boundary = PendingBoundary::Ordinary;
            self.formatter_line
                .queue_separator(usize::from(gap), SeparatorKind::FieldMinimum);
            let field_accepted = self.formatter_line.commit_field();
            self.formatter_line
                .queue_separator(usize::from(field_accepted), SeparatorKind::WordBoundary);
        }
        self.no_break_field = Some(NoBreakField {
            field_width: width,
            body_width: usize::from(body),
            trailspace_cells: usize::from(gap),
            style: if wraps {
                DefinitionFieldStyle::Tag
            } else {
                DefinitionFieldStyle::Hang
            },
        });
        self.empty_word = false;
        self.pending_breakable_spaces = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.final_word_join = Some(false);
        true
    }

    fn take_no_break_field(&mut self) -> Option<NoBreakField> {
        self.no_break_field.take()
    }

    fn settle_no_break_field_line(&mut self, field: NoBreakField, consume_body_gap: bool) {
        let resumed_visible =
            self.formatter_line.buffer_is_visible() || self.zero_advance.has_buffered_glyph();
        self.flush_zero_advance();
        self.formatter_line.commit_field();
        match field.style {
            DefinitionFieldStyle::Tag => {
                self.force_output_line_break();
                if !resumed_visible {
                    self.pending_definition_indent = Some(field.body_width);
                }
                self.definition_outcome.mark_field_exited();
            }
            DefinitionFieldStyle::Hang => {
                if !resumed_visible {
                    self.pending_definition_indent =
                        Some(field.body_width.saturating_sub(field.field_width).max(1));
                }
                self.formatter_line
                    .queue_separator(field.trailspace_cells, SeparatorKind::FieldMinimum);
                self.boundary = PendingBoundary::Tight;
            }
        }
        self.finish_definition_field_control(field, 0, consume_body_gap);
    }

    fn finish_definition_field_control(
        &mut self,
        field: NoBreakField,
        hang_gap_cells: u8,
        consume_body_gap: bool,
    ) {
        if consume_body_gap {
            self.definition_outcome.mark_body_gap_consumed();
        }
        if let Some(execution) = &mut self.author_execution {
            execution.break_effect = match field.style {
                DefinitionFieldStyle::Tag => AuthorBreakEffect::Line,
                DefinitionFieldStyle::Hang => AuthorBreakEffect::Field {
                    gap_cells: hang_gap_cells,
                    body_width_columns: u16::try_from(field.body_width).unwrap_or(u16::MAX),
                    wraps: false,
                },
            };
        }
        self.word_end_break = WordEndBreak::Clear;
        self.formatter_line.discard_empty_field();
        self.empty_word = false;
        self.final_word_join = Some(false);
    }

    fn force_output_line_break(&mut self) {
        if matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            return;
        }
        self.flush_zero_advance();
        self.nodes.push(Inline::LineBreak);
        self.last_visible_character = Some('\n');
        self.trailing_output = TrailingOutput::None;
        self.boundary = PendingBoundary::Ordinary;
        self.pending_breakable_spaces = 0;
        self.formatter_line.clear_separators();
        self.formatter_line.end_row();
    }
}
