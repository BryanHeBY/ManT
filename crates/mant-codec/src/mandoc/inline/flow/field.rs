use super::{
    AuthorBreakEffect, DefinitionFieldStyle, FormatterColumn, Inline, InlineBuilder, NoBreakField,
    PendingBoundary, TrailingOutput, WordEndBreak, has_printable_character, last_visible_character,
    trim_trailing_breakable_spaces,
};

impl InlineBuilder {
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
            self.author_execution
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

    pub(super) fn flush_definition_field(
        &mut self,
        field_output_start: usize,
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
                    execution.field_output_start = self.nodes.len();
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
        let field = self.nodes.get(field_output_start..).unwrap_or_default();
        let field_is_printable = has_printable_character(field);
        let field_width = mant_ir::geometry::text_width(&super::super::plain_text(field));
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
        self.pending_field_spaces = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.formatter_column = FormatterColumn::Origin;
        if let Some(execution) = &mut self.author_execution {
            execution.field_output_start = self.nodes.len();
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
        self.trailing_output = TrailingOutput::FixedBlank;
    }

    /// Execute `.ti` after the definition field's `roff_term_pre_br()` step.
    /// The temporary absolute offset is independent from whether NOBREAK
    /// actually ended a line; HANG keeps the current field and uses only its
    /// ordinary trailspace.
    pub(in crate::mandoc) fn temporary_indent(&mut self, count: usize) {
        if let Some(field) = self.take_no_break_field() {
            self.restore_no_break_field_projection(field);
            match field.style {
                DefinitionFieldStyle::Tag => {
                    self.force_output_line_break();
                    self.pending_line_indent = count;
                    self.definition_outcome.mark_field_exited();
                }
                DefinitionFieldStyle::Hang => {
                    // `term_newln()` leaves NOSPACE set.  HANG keeps the
                    // device row alive, so a following control-only author
                    // transition must not synthesize an ordinary word blank
                    // between the prior field and its eventual head text.
                    self.boundary = PendingBoundary::Tight;
                    self.pending_field_spaces = 1;
                }
            }
            self.finish_definition_field_control(field, 0, false);
            return;
        }
        let Some((start, gap, body, wraps)) =
            self.author_execution
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
            self.pending_line_indent = count;
        } else {
            self.append_fixed_cells(if wraps { count } else { usize::from(gap) });
        }
        if let Some(execution) = &mut self.author_execution {
            execution.field_output_start = self.nodes.len();
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
            self.settle_no_break_field_line(field);
            self.definition_outcome.mark_field_exited();
            return;
        }
        let Some((start, gap, body, wraps)) =
            self.author_execution
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
            self.pending_definition_indent = Some(usize::from(body));
        } else {
            let width = mant_ir::geometry::text_width(&super::super::plain_text(field));
            self.append_fixed_cells(usize::from(body).saturating_sub(width));
        }
        self.definition_outcome.mark_field_exited();
        if let Some(execution) = &mut self.author_execution {
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
        self.boundary = PendingBoundary::Tight;
    }

    /// Execute an inline vertical-space request without retaining its
    /// numeric operand as document text.  `term_vspace(n)` first closes an
    /// occupied row, then emits `n` empty rows.
    pub(in crate::mandoc) fn vertical_space(&mut self, rows: usize) {
        if let Some(field) = self.take_no_break_field() {
            self.restore_no_break_field_projection(field);
            self.force_output_line_break();
            if field.style == DefinitionFieldStyle::Tag {
                self.retain_line_breaks(rows);
            } else {
                self.retain_line_breaks(rows.saturating_sub(1));
            }
            self.pending_definition_indent = Some(field.body_width);
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
                        execution.field_output_start = self.nodes.len();
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
            self.pending_definition_indent = Some(usize::from(body_width_columns));
            if let Some(execution) = &mut self.author_execution {
                execution.field_output_start = self.nodes.len();
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
        if !self.has_formatter_cell() {
            return;
        }
        if self.no_break_definition_field() {
            return;
        }
        let literal_flow = self.source_cursor.is_some();
        let continued = self.final_source_continuation_or(false);
        let realizes_literal_word_end_break =
            literal_flow && self.word_end_break == WordEndBreak::Pending && !continued;
        self.flush_zero_advance();
        // A visible glyph always makes the field printable.  CVS also keeps
        // an otherwise invisible `\&` cell when `\c` suppresses the source
        // line flush; without that continuation, the invisible-only field is
        // discarded before `.mc` and contributes no relative separator.
        if realizes_literal_word_end_break {
            if !self
                .nodes
                .iter()
                .rev()
                .take_while(|node| !matches!(node, Inline::LineBreak))
                .any(|node| has_printable_character(std::slice::from_ref(node)))
            {
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
        self.pending_field_spaces = usize::from(!realizes_literal_word_end_break);
        self.word_end_break = WordEndBreak::Clear;
        self.formatter_column = FormatterColumn::Origin;
        // TERMP_NOBREAK only changes this flush; it does not create
        // TERMP_NONEWLINE.  Preserve any already-executed `\c` continuation,
        // while releasing its tight word boundary like CVS clears NOSPACE.
        self.final_word_join = Some(false);
    }

    fn no_break_definition_field(&mut self) -> bool {
        let Some((start, gap, body, wraps)) =
            self.author_execution
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
        let overrun = width.saturating_add(usize::from(gap)) > usize::from(body);
        let output_end_before_separator = self.nodes.len();
        if wraps {
            if overrun {
                self.hard_break();
                // Clearing NOSPACE after `.mc` leaves one ordinary boundary
                // at the list origin; BRIND is not executed by this request.
                self.pending_definition_indent = Some(1);
            } else {
                self.append_fixed_cells(usize::from(gap).saturating_add(1));
                self.boundary = PendingBoundary::Tight;
            }
            self.definition_outcome.mark_field_exited();
            if let Some(execution) = &mut self.author_execution {
                execution.field_output_start = self.nodes.len();
                execution.break_effect = AuthorBreakEffect::Line;
            }
        } else {
            // HANG prevents the flush from ending the physical row.  A field
            // that still has room retains trailspace plus the ordinary next
            // word boundary; an overrun field retains only that boundary.
            self.boundary = PendingBoundary::Ordinary;
            self.pending_field_spaces = if overrun {
                1
            } else {
                usize::from(gap).saturating_add(1)
            };
            if let Some(execution) = &mut self.author_execution {
                execution.field_output_start = self.nodes.len();
            }
        }
        self.no_break_field = Some(NoBreakField {
            output_end_before_separator,
            resumed_output_start: self.nodes.len(),
            field_width: width,
            body_width: usize::from(body),
            separator_cells: if overrun {
                usize::from(!wraps)
            } else {
                usize::from(gap).saturating_add(1)
            },
            style: if wraps {
                DefinitionFieldStyle::Tag
            } else {
                DefinitionFieldStyle::Hang
            },
            overrun,
        });
        self.empty_word = false;
        self.pending_breakable_spaces = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.formatter_column = FormatterColumn::Origin;
        self.final_word_join = Some(false);
        true
    }

    fn take_no_break_field(&mut self) -> Option<NoBreakField> {
        self.no_break_field.take()
    }

    fn restore_no_break_field_projection(&mut self, field: NoBreakField) -> bool {
        // A prior `.mc` can leave the device row occupied (`viscol > 0`) even
        // after the current field buffer was reset.  Every control reaching
        // this path therefore executes a real `term_flushln()`: settle a
        // completed zero-advance glyph and, critically, clear a bare
        // BACKAFTER request before the next word runs.
        self.flush_zero_advance();
        let resumed_visible = super::super::plain_text(
            self.nodes
                .get(field.resumed_output_start..)
                .unwrap_or_default(),
        )
        .chars()
        .any(|ch| !ch.is_whitespace());
        if !field.overrun && !resumed_visible {
            trim_trailing_breakable_spaces(&mut self.nodes, field.separator_cells);
            let mut index = self.nodes.len();
            while index > field.output_end_before_separator {
                index -= 1;
                if matches!(&self.nodes[index], Inline::Text { value } | Inline::Code { value } if value.chars().all(char::is_whitespace))
                {
                    self.nodes.remove(index);
                }
            }
            self.last_visible_character = last_visible_character(&self.nodes);
            self.trailing_output = if self.last_visible_character.is_some() {
                TrailingOutput::NonBlank
            } else {
                TrailingOutput::None
            };
        }
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
        resumed_visible
    }

    fn settle_no_break_field_line(&mut self, field: NoBreakField) {
        let resumed_visible = self.restore_no_break_field_projection(field);
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
                self.boundary = PendingBoundary::Tight;
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
        if consume_body_gap {
            self.definition_outcome.mark_body_gap_consumed();
        }
        if let Some(execution) = &mut self.author_execution {
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
        self.word_end_break = WordEndBreak::Clear;
        self.formatter_column = FormatterColumn::Origin;
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
        self.pending_field_spaces = 0;
        self.formatter_column = FormatterColumn::Origin;
    }
}
