use super::super::native_field::{FieldFlags, row_continues};
use super::super::{
    AuthorBreakEffect, FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput,
    WordEndBreak, has_printable_character, last_visible_character, trim_trailing_breakable_spaces,
};
use super::state::{DefinitionFieldStyle, HangRowTransition, NoBreakField, PendingFieldGapOrigin};

impl InlineBuilder {
    // The NOBREAK flush commits field, row, and BRIND state in native order.
    #[allow(clippy::too_many_lines)]
    pub(in crate::mandoc::inline::flow) fn flush_definition_field(
        &mut self,
        field_output_start: usize,
        gap_cells: u8,
        body_width_columns: u16,
        field_width_columns: u16,
        flags: FieldFlags,
        exit_field: bool,
    ) -> bool {
        self.commit_definition_row_origin();
        self.discard_unprinted_definition_field_output();
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
        let field_width = mant_ir::geometry::text_width(&super::super::super::plain_text(field));
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
                definition.row.commit_on_source_flush();
            }
        }
        overruns
    }

    pub(super) fn flush_native_hang_field(&mut self, gap: u8, body_width: u16, exit_field: bool) {
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

    /// All field acceptance decisions come from the native cells, including
    /// a first-pass nbr=0. No old word-level flag can override this result.
    pub(super) fn native_field_flush_receipt(
        &self,
    ) -> Option<super::super::field_buffer::FlushReceipt> {
        let definition = self.execution.definition.as_ref()?;
        (!definition.field_buffer.is_empty()).then(|| {
            let target = if definition.no_break_cleared && !self.execution.no_fill_word_active {
                usize::from(definition.cleared_field_capacity_columns).max(1)
            } else {
                usize::MAX / 2
            };
            definition.field_buffer.flush_receipt(target, false)
        })
    }

    pub(super) fn vertical_space_in_definition_field(&mut self, field: NoBreakField, rows: usize) {
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
    pub(super) fn continue_no_break_definition_field(&mut self, mut field: NoBreakField) {
        let native = self.native_field_device_with_resume(true, Some(field));
        let resumed_has_cell = self.restore_no_break_field_projection(field);
        if let Some(native) = &native {
            let row = &mut self.definition_state_mut().hang_row;
            row.flush(field.trailspace_cells);
            row.viscol = native.viscol;
            row.margin_flush_seen = true;
        }
        if !resumed_has_cell {
            // Whitespace-only and zero-width formatter words make
            // `term_flushln()` run, but `term_fill()` commits no field.
            // Restore the one separator that was waiting for the next real
            // field instead of consuming it or manufacturing a second one.
            field.output_end_before_separator = self.nodes.len();
            self.append_field_separator(field.separator_cells);
            self.execution.boundary = PendingBoundary::CommittedField;
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
        let resumed_width = native.as_ref().map_or_else(
            || mant_ir::geometry::text_width(&super::super::super::plain_text(resumed)),
            |field| field.width,
        );
        let overrun = native.as_ref().map_or_else(
            || {
                self.current_formatter_row_width()
                    .saturating_add(field.trailspace_cells)
                    > field.body_width
            },
            |field| field.overruns,
        );
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
        self.execution.boundary = PendingBoundary::CommittedField;

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

    pub(super) fn reset_after_no_break_field(&mut self) {
        self.execution.empty_word = false;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.final_word_join = Some(false);
    }

    pub(super) fn current_formatter_row_width(&self) -> usize {
        let start = self
            .nodes
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak { .. }))
            .map_or(0, |index| index + 1);
        mant_ir::geometry::text_width(&super::super::super::plain_text(&self.nodes[start..]))
    }

    pub(super) fn append_field_separator(&mut self, count: usize) {
        self.append_fixed_cells(count);
        if count > 0 {
            self.execution.trailing_output = TrailingOutput::FieldBlank(count);
        }
    }

    pub(super) fn no_break_definition_field(&mut self) -> bool {
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
        let native = self.native_field_device(true);
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        let width = native.as_ref().map_or_else(
            || mant_ir::geometry::text_width(&super::super::super::plain_text(field)),
            |field| field.width,
        );
        if let Some(native) = &native {
            let row = &mut self.definition_state_mut().hang_row;
            row.flush(usize::from(gap));
            row.viscol = native.viscol;
            row.margin_flush_seen = true;
        }
        let overrun = native.as_ref().map_or(
            width.saturating_add(usize::from(gap)) > usize::from(body),
            |field| field.overruns,
        );
        let output_end_before_separator = self.nodes.len();
        if flags.wraps() {
            if overrun {
                self.hard_break();
                // Clearing NOSPACE leaves a pending boundary for the next
                // term_word(), not an occupied row before HEAD post.
                self.execution.pending_field_spaces = 1;
                self.execution.boundary = PendingBoundary::CommittedField;
            } else {
                self.append_field_separator(usize::from(gap).saturating_add(1));
                self.execution.boundary = PendingBoundary::CommittedField;
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
        self.retire_consumed_native_field();
        true
    }

    pub(super) fn take_no_break_field(&mut self) -> Option<NoBreakField> {
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
        // A request consumes this buffer, not the item's BODY lifetime.
        // roff_term.c::roff_term_pre_mc() can leave viscol occupied, and
        // later words may arm BACKAFTER in a fresh buffer. The BODY post
        // still executes its own conditional term_newln().
        self.execution
            .definition
            .as_mut()
            .and_then(|state| state.no_break.take())
    }

    pub(super) fn restore_no_break_field_projection(&mut self, field: NoBreakField) -> bool {
        // A prior `.mc` can leave the device row occupied (`viscol > 0`) even
        // after the current field buffer was reset.  Every control reaching
        // this path therefore executes a real `term_flushln()`: settle a
        // completed zero-advance glyph and, critically, clear a bare
        // BACKAFTER request before the next word runs.
        let native = self.native_field_device_with_resume(false, Some(field));
        self.flush_zero_advance();
        let resumed = self
            .nodes
            .get(field.resumed_output_start..)
            .unwrap_or_default();
        let resumed_text = super::super::super::plain_text(resumed);
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
        if let Some(native) = native {
            let row = &mut self.definition_state_mut().hang_row;
            row.flush(field.trailspace_cells);
            row.viscol = native.viscol;
        }
        self.retire_consumed_native_field();
        resumed_has_cell
    }

    pub(super) fn settle_no_break_field_line(&mut self, field: NoBreakField, row_indent: u16) {
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
                                super::super::field_buffer::FieldCell::Graph { .. }
                                    | super::super::field_buffer::FieldCell::NonBreakingBlank
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
}
pub(super) fn retain_unprinted_field_targets(inlines: &mut Vec<Inline>) {
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
