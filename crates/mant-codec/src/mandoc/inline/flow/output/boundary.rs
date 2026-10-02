use super::super::{
    FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput, WordEndBreak,
    first_visible_character, has_printable_character, last_visible_character, needs_boundary_space,
    push_text,
};
use super::has_rendered_formatter_glyph;
use super::line_break_count;
use super::trim_trailing_breakable_spaces;

/// Observations of the operand being appended, taken before its Vec is drained.
/// Native acceptance and formatter registers remain in their existing owners.
#[derive(Clone, Copy)]
struct AppendedWordContent {
    last: Option<char>,
    printable: bool,
    glyphs: bool,
    line_break: bool,
}

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
        if trailing_output != TrailingOutput::None {
            self.execution.trailing_output = trailing_output;
        }
    }

    // The register choreography mirrors print_mdoc_node()/term_word(); the
    // length is the sequence itself.
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
        let incoming_has_glyph = has_rendered_formatter_glyph(incoming);
        let incoming_has_line_break = line_break_count(incoming) > 0;
        if incoming_has_printable || word {
            self.execution.execution_epoch = self.execution.execution_epoch.wrapping_add(1);
        }
        if incoming_first.is_none() && !incoming_has_printable && !word {
            self.nodes.append(incoming);
            return;
        }
        let (empty_word, boundary, fixed_blank_boundary, concat_next_word) = self
            .prepare_projection_boundary(
                incoming_first,
                incoming_has_printable,
                incoming_starts_with_fixed_blank,
                word,
            );
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
        let projection = if word {
            self.record_hang_word(incoming, boundary)
        } else {
            super::record::WordPassProjection::default()
        };
        self.append_recorded_word_boundary(
            boundary,
            word,
            empty_word,
            add_space,
            incoming_first,
            &projection,
        );
        // The native word boundary belongs to its surrounding flow. Only
        // the operand itself belongs to an optional semantic annotation;
        // authored leading blanks therefore stay inside that owner.
        if word {
            // A word containing only a cached glyph has no immediate IR
            // content. Its already-executed separator still precedes this
            // content marker (term_word() before encode1()), including when
            // that separator otherwise waits for the next visible word.
            self.materialize_boundary_before_pending_glyph();
            for marker in self.pending_output_scope_prefixes.drain(..) {
                self.nodes.push(Inline::anchor(marker));
            }
        }
        let starts_output_row = incoming_has_line_break
            || projection.split_word.is_some()
            || projection.closes_before > 0;
        match projection.split_word {
            Some(split) => self.nodes.extend(split),
            None => self.nodes.append(incoming),
        }
        if starts_output_row {
            self.note_definition_output_row();
        }
        self.observe_appended_word(
            AppendedWordContent {
                last: incoming_last,
                printable: incoming_has_printable,
                glyphs: incoming_has_glyph,
                line_break: incoming_has_line_break,
            },
            occupies_row,
            empty_word,
        );
    }

    fn prepare_projection_boundary(
        &mut self,
        incoming_first: Option<char>,
        incoming_has_printable: bool,
        incoming_starts_with_fixed_blank: bool,
        word: bool,
    ) -> (bool, PendingBoundary, bool, bool) {
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
        (empty_word, boundary, fixed_blank_boundary, concat_next_word)
    }

    fn observe_appended_word(
        &mut self,
        content: AppendedWordContent,
        occupies_row: bool,
        empty_word: bool,
    ) {
        let AppendedWordContent {
            last: incoming_last,
            printable: incoming_has_printable,
            glyphs: incoming_has_glyph,
            line_break: incoming_has_line_break,
        } = content;
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
        if incoming_has_glyph && !self.execution.current_native_word_is_rejected() {
            // The observation belongs to this actual word. A later generated
            // colon/URI can be rejected while the preceding description
            // prefix survives (mdoc_term.c::termp_lk_pre, term.c::term_fill).
            // Inspecting only that final word after the handler returns
            // would retroactively hide the already accepted first glyph.
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

    fn append_recorded_word_boundary(
        &mut self,
        boundary: PendingBoundary,
        word: bool,
        empty_word: bool,
        add_space: bool,
        incoming_first: Option<char>,
        projection: &super::record::WordPassProjection,
    ) {
        let accepted_row_break = projection.closes_before;
        let leading_cells = projection.leading_cells;
        let native_separator = projection.native_separator;
        // Plain MC's one projected next-word blank is an advance reservation,
        // unlike a definition field's minbl. Once an empty word actually
        // buffers that blank, transfer ownership to the word cells. The
        // following word then obeys its own NOSPACE/NONOSPACE registers
        // (roff_term.c:147-150; term.c:573-589), including .Sm off.
        let transferred_automatic = empty_word
            && native_separator
            && self.execution.pending_field_gap_origin
                == super::super::definition::PendingFieldGapOrigin::AutomaticWord
            && self.execution.pending_field_spaces > 0;
        if transferred_automatic {
            self.execution.pending_breakable_spaces = self
                .execution
                .pending_breakable_spaces
                .saturating_add(std::mem::take(&mut self.execution.pending_field_spaces));
            self.execution.pending_field_gap_origin =
                super::super::definition::PendingFieldGapOrigin::Other;
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
        .map(|anchor| anchor.owner.clone());
        if word
            && (self.execution.pending_field_spaces == 0
                || accepted_row_break > 0
                || !projection.prints_padding)
            && let Some(marker) = native_anchor_marker.as_ref()
        {
            // An accepted marker pass consumes incoming blank positioning
            // at its real endline (term.c:205-217). It bypasses the padding
            // branch below, but still needs this word's stable output owner.
            self.nodes.push(Inline::anchor(marker.clone()));
        }
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
            // after a zero-width or empty word. An IR character predicate
            // cannot establish NOSPACE (term.c:573-589); BACKBEFORE may
            // already have consumed the blank. Filled responsive words keep
            // their projected zero-advance joins, but authored leading blanks
            // do not replace term_word()'s separate automatic separator.
            let (spacing_boundary, add_space) = if transferred_automatic {
                (boundary, false)
            } else if word
                && self.execution.no_fill_word_active
                && boundary == PendingBoundary::CommittedField
            {
                // The committed field already represents its positioning
                // and the following ordinary word blank. term_word() must
                // still buffer that blank after MC cleared NOSPACE, but
                // no-fill cannot project the same cell a second time
                // (roff_term.c:147-150; term.c:573-589,389-427).
                (boundary, false)
            } else if word && self.execution.no_fill_word_active {
                (
                    if native_separator {
                        PendingBoundary::Preserved
                    } else {
                        PendingBoundary::Tight
                    },
                    native_separator,
                )
            } else if empty_word && self.execution.pending_field_spaces > 0 && native_separator {
                // A graphless word cannot print deferred vbl, but its
                // term_word() automatic blank is a distinct native cell
                // (term.c:573-589,389-427). Preserve it until a later graph
                // prints the field; pending positioning cannot consume it.
                (PendingBoundary::Preserved, true)
            } else if word && self.execution.pending_breakable_spaces > 0 && native_separator {
                // Earlier empty words already wrote native cells on this
                // row. A following word writes its own automatic blank
                // even when no visible character represents those cells
                // yet (term.c:573-589).
                (PendingBoundary::Preserved, true)
            } else if word
                && !self.in_definition_field()
                && incoming_first.is_some_and(super::super::super::is_formatter_word_blank)
                && native_separator
                && !boundary.is_tight()
            {
                (boundary, true)
            } else {
                (boundary, add_space)
            };
            // term.c::term_field() skips NBRZW, markers and tab/blank
            // positioning before it writes vbl at an actual Graph. An
            // invisible accepted word must not print and then retire the
            // same minbl again at the next pre_br (term.c:389-427).
            let field_padding =
                self.execution.pending_field_spaces > 0 && projection.prints_padding;
            if word
                && field_padding
                && let Some(marker) = native_anchor_marker.as_ref()
            {
                self.mark_native_field_prefix(marker);
            }
            self.append_boundary_spacing(spacing_boundary, add_space, empty_word, field_padding);
            if word
                && field_padding
                && let Some(marker) = native_anchor_marker
            {
                // Device minbl is field geometry, not a native buffer cell.
                // The word's accepted scalar interval begins after that pad.
                self.nodes.push(Inline::anchor(marker));
            }
        }
    }

    fn mark_native_field_prefix(&mut self, owner: &str) {
        let serial = owner
            .strip_prefix(super::INTERNAL_FIELD_WORD)
            .expect("native word owner");
        self.nodes.push(Inline::anchor(format!(
            "{}{serial}",
            super::INTERNAL_FIELD_PREFIX
        )));
        let anchor = if self.in_definition_field() {
            self.execution
                .definition
                .as_mut()
                .and_then(|field| field.field_word_anchors.last_mut())
        } else {
            self.execution.flush_unit_anchors.last_mut()
        };
        if let Some(anchor) = anchor {
            anchor.projected_field_prefix = true;
        }
    }

    pub(super) fn append_boundary_spacing(
        &mut self,
        boundary: PendingBoundary,
        add_space: bool,
        empty_word: bool,
        materialized_field_separator: bool,
    ) {
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
            let separator_start = self.nodes.len();
            // This generated range remains provisional until term_field()
            // prints a graph (term.c:389-427). Keep it separate from the
            // committed prefix so a graphless control flush can retire only
            // these cells, using NoBreakField's saved owner boundaries.
            self.nodes.push(Inline::Text {
                value: " ".repeat(count),
            });
            self.bind_materialized_field_separator(separator_start, count);
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
