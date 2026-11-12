use super::super::WordProjectionPolicy;
use super::super::{
    FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput, WordEndBreak,
    first_visible_character, has_printable_character, last_visible_character, needs_boundary_space,
    push_text,
};
use super::has_rendered_formatter_glyph;
use super::line_break_count;
use super::trim_trailing_breakable_spaces;

mod spacing;
mod word;

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
        self.execution.pending_word_spaces.clear();
        self.execution.pending_field_spaces = 0;
    }

    pub(in crate::mandoc) fn append(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(
            &mut incoming,
            false,
            true,
            false,
            WordProjectionPolicy::Printed,
        );
    }

    /// A native text node is a word event even if decoding yields no glyphs.
    /// Unlike a target/control-only append, it consumes the pending boundary.
    pub(in crate::mandoc) fn append_word(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(
            &mut incoming,
            true,
            true,
            false,
            WordProjectionPolicy::Printed,
        );
    }

    /// A decoded empty word can be a real literal row, an empty macro
    /// argument, or a pure formatter transition. Keep those execution facts
    /// separate without changing filled-flow inter-word spacing.
    pub(in crate::mandoc) fn append_word_with_literal_row(
        &mut self,
        mut incoming: Vec<Inline>,
        occupies_row: bool,
        trailing_output: TrailingOutput,
        projection: WordProjectionPolicy,
    ) {
        self.note_produced_formatter_cell(occupies_row);
        self.append_at_boundary(
            &mut incoming,
            true,
            occupies_row,
            trailing_output == TrailingOutput::FixedBlank,
            projection,
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
        projection: WordProjectionPolicy,
    ) {
        if incoming.is_empty() && !word {
            return;
        }

        if word && self.execution.native_word_boundary.is_none() {
            self.begin_native_word_registers(false);
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
            self.execution.native_word_boundary = None;
            self.execution.native_word_sentence = false;
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
        let mut projection = if word {
            self.record_hang_word(incoming, boundary, projection)
        } else {
            super::record::WordPassProjection::default()
        };
        let starts_output_row = incoming_has_line_break
            || projection.split_word.is_some()
            || !projection.closes_before.is_empty();
        if projection
            .split_word
            .as_deref()
            .is_some_and(super::native_passes::starts_with_native_break)
        {
            // The accepted native pass can begin the projected fragment
            // with a line break even when its last unprinted source blanks
            // already reside in the previous operand's output (term.c:205-
            // 207,389-427). Retire that tail before appending the split.
            super::native_passes::trim_native_breakable_tail(&mut self.nodes);
        }
        let mut payload = projection
            .split_word
            .take()
            .unwrap_or_else(|| std::mem::take(incoming));
        self.route_section_borrowed_prefix(&mut payload);
        let layout_start = self.append_recorded_word_boundary(
            boundary,
            word,
            empty_word,
            add_space,
            &mut projection,
            &mut payload,
        );
        if word {
            self.materialize_boundary_before_pending_glyph();
        }
        self.append_source_payload(payload, word, layout_start);
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

    fn append_source_payload(&mut self, nodes: Vec<Inline>, word: bool, layout_start: usize) {
        let motion_origin = word
            .then(|| self.execution.zero_advance.leading_motion_origin())
            .flatten();
        if !word
            || (motion_origin.is_none() && self.execution.pending_output_scope_prefixes.is_empty())
        {
            self.nodes.extend(nodes);
            return;
        }
        // term_word() writes the automatic boundary first. ESCAPE_HORIZ
        // can then generate layout cells before the author's first payload
        // (term.c:678); its typed receipt keeps those cells outside a label
        // without classifying identical authored SP/NBSP by character kind.
        let (prefix, suffix, found) = super::projection::split_output_scope_prefix(
            nodes,
            crate::mandoc::inline::text_execution::CONTENT_AFTER_MOTION,
        );
        let mut motion = None;
        let content = if found {
            if let Some(origin) = motion_origin {
                // An earlier invisible word may already have opened the
                // range. Give this word's leading motion and its positioning
                // cells their own origin so they remain outside the label
                // without moving intervening native line closures.
                let mut cells = self.nodes.split_off(layout_start);
                cells.extend(prefix);
                motion = Some((origin, cells));
            } else {
                self.nodes.extend(prefix);
            }
            suffix
        } else {
            prefix
        };
        // An NBRZW word opens the annotation range for its native internal
        // separators and hard boundaries, but it does not turn a later
        // word's leading motion into authored label content.
        if !self.execution.current_native_word_is_rejected()
            && (has_printable_character(&content)
                || self
                    .execution
                    .zero_advance
                    .buffered_glyph_belongs_to_current_word())
        {
            self.execution.zero_advance.note_authored_member();
        }
        for marker in std::mem::take(&mut self.execution.pending_output_scope_prefixes) {
            self.nodes.push(Inline::anchor(marker));
        }
        if let Some((origin, cells)) = motion {
            self.nodes
                .extend(crate::mandoc::inline::ownership::mark_layout_origin(
                    origin, cells,
                ));
        }
        self.nodes.extend(content);
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
            self.execution.pending_word_spaces.clear();
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
}
