use super::super::{
    FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput, WordEndBreak,
    first_visible_character, has_printable_character, last_visible_character, needs_boundary_space,
    push_text,
};
use super::has_non_whitespace_glyph;
use super::line_break_count;
use super::trim_trailing_breakable_spaces;

impl InlineBuilder {
    /// A compact semantic spelling can omit an authored empty trailing word
    /// after its generated punctuation.  The omitted word still establishes
    /// one ordinary boundary, but padding already queued on its left must not
    /// be replayed in addition to the following word's own boundary.
    pub(in crate::mandoc) fn consume_compacted_pending_padding(&mut self) {
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
    }

    pub(in crate::mandoc) fn append(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(&mut incoming, false, true, false);
    }

    /// A native text node is a word event even if decoding yields no glyphs.
    /// Unlike a target/control-only append, it consumes the pending boundary.
    pub(in crate::mandoc) fn append_word(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(&mut incoming, true, true, false);
    }

    /// A decoded empty word can be a real literal row, an empty macro
    /// argument, or a pure formatter transition. Keep those execution facts
    /// separate without changing filled-flow inter-word spacing.
    pub(in crate::mandoc) fn append_word_with_literal_row(
        &mut self,
        mut incoming: Vec<Inline>,
        occupies_row: bool,
        trailing_output: TrailingOutput,
    ) {
        self.note_produced_formatter_cell(occupies_row);
        self.append_at_boundary(
            &mut incoming,
            true,
            occupies_row,
            trailing_output == TrailingOutput::FixedBlank,
        );
        self.materialize_boundary_before_pending_glyph();
        if trailing_output != TrailingOutput::None {
            self.execution.trailing_output = trailing_output;
        }
    }

    // The register choreography mirrors print_mdoc_node()/term_word(); the
    // length is the sequence itself.
    #[allow(clippy::too_many_lines)]
    pub(super) fn append_at_boundary(
        &mut self,
        incoming: &mut Vec<Inline>,
        word: bool,
        occupies_row: bool,
        incoming_starts_with_fixed_blank: bool,
    ) {
        if incoming.is_empty() && !word {
            return;
        }

        if word && self.execution.wipe_remainder {
            // The flush unit was definitively rejected (term.c:143-146 with
            // 233-237): the words after the rejection point are unprinted
            // input. Their formatter registers still advance exactly like
            // `term_word()` calls whose buffer dies at the next flush.
            self.execution.execution_epoch = self.execution.execution_epoch.wrapping_add(1);
            self.execution.boundary = PendingBoundary::Ordinary;
            self.execution.word_end_break = WordEndBreak::Clear;
            self.execution.native_word_writes = None;
            return;
        }

        let incoming_first = first_visible_character(incoming);
        let incoming_last = last_visible_character(incoming);
        let incoming_has_printable = has_printable_character(incoming);
        let incoming_has_glyph = has_non_whitespace_glyph(incoming);
        let incoming_has_line_break = line_break_count(incoming) > 0;
        if incoming_has_printable || word {
            self.execution.execution_epoch = self.execution.execution_epoch.wrapping_add(1);
        }
        if incoming_first.is_none() && !incoming_has_printable && !word {
            self.nodes.append(incoming);
            return;
        }
        if (incoming_has_printable || word)
            && (self.pending_definition_indent().is_some()
                || self.execution.pending_line_indent > 0)
        {
            let cells = self
                .definition
                .as_mut()
                .and_then(|state| state.pending_indent.take())
                .unwrap_or_else(|| std::mem::take(&mut self.execution.pending_line_indent));
            self.append_fixed_cells(cells);
            self.execution.boundary = PendingBoundary::Tight;
        }
        let empty_word = word && incoming_first.is_none() && !incoming_has_printable;
        let boundary = std::mem::replace(&mut self.execution.boundary, PendingBoundary::Ordinary);
        let fixed_blank_boundary = (incoming_starts_with_fixed_blank
            && incoming_first.is_some_and(char::is_whitespace))
            || self.execution.trailing_output == TrailingOutput::FixedBlank;
        let concat_next_word = std::mem::take(&mut self.execution.concat_next_word);
        let concat_flush_source = std::mem::take(&mut self.execution.concat_flush_source);
        if concat_next_word {
            self.execution.concat_consumed_for_body = true;
            self.execution.flush_consumed_for_body = concat_flush_source;
            // TERMP_NOSPACE suppresses every form of the pending auto
            // blank, not only the character-driven one: the deferred
            // field separator must not materialize, and trailing
            // breakable padding a field flush already emitted is
            // unprinted input (term.c:205-207), not content.
            self.execution.pending_field_spaces = 0;
            self.execution.pending_breakable_spaces = 0;
            trim_trailing_breakable_spaces(&mut self.nodes, usize::MAX);
            self.execution.last_visible_character = last_visible_character(&self.nodes);
        }
        if let Some(definition) = &mut self.execution.definition {
            // Emit after the NOSPACE arm's trims: the jump fill is the
            // upstream `vbl` pad (term.c:113-114), not a breakable blank.
            // The amount is `offset - viscol` at print time, exactly as
            // upstream computes it when the carrying word flushes.
            let viscol = u16::try_from(definition.hang_row.viscol).unwrap_or(u16::MAX);
            let fill = definition.row.emit_armed(self.nodes.len(), viscol);
            if fill > 0 {
                self.nodes.push(Inline::Text {
                    value: " ".repeat(usize::from(fill)),
                });
            }
        }
        let add_space = if concat_next_word {
            false
        } else if empty_word
            && self.execution.formatter_column == FormatterColumn::Origin
            && self.execution.last_visible_character == Some('\n')
            && !self.execution.empty_word
        {
            // term_newln() selects NOSPACE for the first word of the new
            // physical row. A second empty term_word() can then buffer its
            // automatic separator even while the last visible glyph remains
            // the preceding row's newline.
            false
        } else if fixed_blank_boundary {
            self.execution.has_printable_content
        } else if (matches!(boundary, PendingBoundary::Continued)
            && incoming_first.is_some_and(char::is_whitespace))
            || empty_word
            || self.execution.empty_word
        {
            self.execution.has_printable_content
                || self.execution.trailing_output != TrailingOutput::None
                || (empty_word && self.execution.empty_word)
        } else {
            needs_boundary_space(self.execution.last_visible_character, incoming_first)
        };
        let super::record::WordPassProjection {
            closes_before: accepted_row_break,
            leading_cells,
            native_separator,
            split_word,
        } = if word {
            self.record_hang_word(incoming, boundary)
        } else {
            super::record::WordPassProjection::default()
        };
        if accepted_row_break > 0 {
            // A consumed \p separator closes the already accepted prefix.
            // Its blank is part of the break, not a new formatter word cell.
            self.execution.pending_breakable_spaces = 0;
            trim_trailing_breakable_spaces(&mut self.nodes, usize::MAX);
            for _ in 0..accepted_row_break {
                let row_indent = self.take_definition_row_indent();
                self.nodes.push(Inline::line_break_indented(row_indent));
            }
            if leading_cells > 0 {
                push_text(&mut self.nodes, " ".repeat(leading_cells));
            }
        } else {
            // Literal rows preserve the actual buffered separator even
            // after a zero-width or empty word. An IR-visible-character
            // predicate cannot establish NOSPACE (term.c:573-589). Use the
            // post-encode receipt: BACKBEFORE may already have consumed the
            // blank. Filled responsive projection keeps its frozen padding
            // policy; only authored no-fill rows expose these native cells.
            let (spacing_boundary, add_space) = if word && self.execution.no_fill_word_active {
                (
                    if native_separator {
                        PendingBoundary::Preserved
                    } else {
                        PendingBoundary::Tight
                    },
                    native_separator,
                )
            } else {
                (boundary, add_space)
            };
            self.append_boundary_spacing(spacing_boundary, add_space, word, empty_word);
        }
        // Plain words anchor into the same native flush unit the field
        // path uses; the marker routes by session exactly like the cell
        // recording in `record_native_word`.
        let native_anchor_marker = if self.in_definition_field() {
            self.execution
                .definition
                .as_ref()
                .and_then(|state| state.field_word_anchors.last())
        } else {
            self.execution.flush_unit_anchors.last()
        }
        .map(|(_, marker, _)| marker.clone());
        if word && let Some(marker) = native_anchor_marker {
            self.nodes.push(Inline::anchor(marker));
        }
        let starts_output_row =
            incoming_has_line_break || split_word.is_some() || accepted_row_break > 0;
        match split_word {
            Some(split) => self.nodes.extend(split),
            None => self.nodes.append(incoming),
        }
        if starts_output_row {
            self.note_definition_output_row();
        }
        if let Some(definition) = &mut self.execution.definition {
            definition.row.close_word();
        }
        if incoming_last.is_some() {
            self.execution.last_visible_character = incoming_last;
            // Generic projected words are formatter glyphs, not trim-eligible
            // source padding. Raw text execution supplies its precise class
            // through `append_word_with_literal_row()` above.
            self.execution.trailing_output = if incoming_last.is_some_and(char::is_whitespace) {
                TrailingOutput::BoundaryBlank
            } else {
                TrailingOutput::NonBlank
            };
        }
        self.execution.has_printable_content |= incoming_has_printable;
        if incoming_has_glyph {
            self.execution.visible_glyph_epoch = self.execution.visible_glyph_epoch.wrapping_add(1);
            self.execution.completed_vertical_rows = 0;
        }
        if incoming_has_line_break {
            self.execution.formatter_column =
                if incoming_has_printable && !matches!(incoming_last, Some('\n')) {
                    FormatterColumn::Advanced
                } else {
                    FormatterColumn::Origin
                };
        } else if incoming_has_printable || occupies_row {
            self.execution.formatter_column = FormatterColumn::Advanced;
        }
        self.execution.empty_word = empty_word;
    }

    pub(super) fn append_boundary_spacing(
        &mut self,
        boundary: PendingBoundary,
        add_space: bool,
        formatter_word: bool,
        empty_word: bool,
    ) {
        let materialized_field_separator =
            formatter_word && self.execution.pending_field_spaces > 0;
        if !empty_word && self.execution.pending_breakable_spaces > 0 {
            let spaces = " ".repeat(self.execution.pending_breakable_spaces);
            self.push_field_owned_breakable_space(spaces);
            let prior = match self.execution.trailing_output {
                TrailingOutput::BreakableBlank(count) => count,
                _ => 0,
            };
            self.execution.trailing_output = TrailingOutput::BreakableBlank(
                prior.saturating_add(self.execution.pending_breakable_spaces),
            );
            self.execution.pending_breakable_spaces = 0;
        }
        if materialized_field_separator {
            let count = self.execution.pending_field_spaces;
            push_text(&mut self.nodes, " ".repeat(count));
            self.execution.pending_field_spaces = 0;
            self.execution.trailing_output = TrailingOutput::FieldBlank(count);
            self.execution.last_visible_character = Some(' ');
            self.execution.formatter_column = FormatterColumn::Advanced;
        }
        if !(self.execution.spacing.enabled()
            || matches!(
                boundary,
                PendingBoundary::Preserved | PendingBoundary::Continued | PendingBoundary::Kept
            ))
            || boundary.is_tight()
            || !add_space
            || materialized_field_separator
        {
            return;
        }
        if empty_word {
            // A word boundary is real, but trailing formatter padding is not
            // authored term content. Materialize it at the next glyph. CVS
            // term_word() has already buffered this cell, so a line request
            // must still close the otherwise invisible physical row.
            self.execution.pending_breakable_spaces =
                self.execution.pending_breakable_spaces.saturating_add(1);
            self.execution.formatter_column = FormatterColumn::Advanced;
            self.note_produced_formatter_cell(true);
        } else {
            self.push_field_owned_breakable_space(" ".to_owned());
            let prior = match self.execution.trailing_output {
                TrailingOutput::BreakableBlank(count) => count,
                _ => 0,
            };
            self.execution.trailing_output =
                TrailingOutput::BreakableBlank(prior.saturating_add(1));
            self.execution.last_visible_character = Some(' ');
            self.execution.has_printable_content = true;
            self.execution.formatter_column = FormatterColumn::Advanced;
        }
    }

    fn push_field_owned_breakable_space(&mut self, spaces: String) {
        if self
            .execution
            .author_execution
            .as_ref()
            .is_some_and(|execution| execution.field_output_start == self.nodes.len())
        {
            // Keep the new field's blank separate from the preceding field's
            // fixed padding. term_fill() may discard the new field entirely.
            self.nodes.push(Inline::Text { value: spaces });
        } else {
            push_text(&mut self.nodes, spaces);
        }
    }
}
