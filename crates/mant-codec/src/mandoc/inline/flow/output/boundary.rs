use super::super::WordProjectionPolicy;
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
        self.execution.pending_word_spaces.clear();
        self.execution.pending_field_spaces = 0;
    }

    pub(in crate::mandoc) fn append(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(
            &mut incoming,
            false,
            true,
            false,
            0,
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
            0,
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
        projected_separator: usize,
        projection: WordProjectionPolicy,
    ) {
        self.note_produced_formatter_cell(occupies_row);
        self.append_at_boundary(
            &mut incoming,
            true,
            occupies_row,
            trailing_output == TrailingOutput::FixedBlank,
            projected_separator,
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
        projected_separator: usize,
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
        // A marker-blank retreat already emitted these native cells after
        // the cached glyph. Only the remaining write receipt can add cells
        // before the new payload; never replay them before that old glyph.
        projection.native_separator = projection
            .native_separator
            .saturating_sub(projected_separator);
        let starts_output_row = incoming_has_line_break
            || projection.split_word.is_some()
            || !projection.closes_before.is_empty();
        let mut payload = projection
            .split_word
            .take()
            .unwrap_or_else(|| std::mem::take(incoming));
        let layout_start = self.append_recorded_word_boundary(
            boundary,
            word,
            empty_word,
            add_space && projected_separator == 0,
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

    fn append_recorded_word_boundary(
        &mut self,
        boundary: PendingBoundary,
        word: bool,
        empty_word: bool,
        add_space: bool,
        projection: &mut super::record::WordPassProjection,
        payload: &mut Vec<Inline>,
    ) -> usize {
        let accepted_row_break = !projection.closes_before.is_empty();
        let native_separator = projection.native_separator;
        let layout_start = self.nodes.len();
        // Plain MC's one projected next-word blank is an advance reservation,
        // unlike a definition field's minbl. Once an empty word actually
        // buffers that blank, transfer ownership to the word cells. The
        // following word then obeys its own NOSPACE/NONOSPACE registers
        // (roff_term.c:147-150; term.c:573-589), including .Sm off.
        let transferred_automatic = empty_word
            && native_separator > 0
            && self.execution.pending_field_gap_origin
                == super::super::definition::PendingFieldGapOrigin::AutomaticWord
            && self.execution.pending_field_spaces > 0;
        if transferred_automatic {
            let spaces = std::mem::take(&mut self.execution.pending_field_spaces);
            self.defer_current_word_spaces(spaces);
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
            self.execution.plain_input.anchors.last()
        }
        .map(|anchor| anchor.owner.clone());
        let earlier_spaces = if !accepted_row_break && !empty_word {
            self.materialize_pending_word_spaces()
        } else {
            0
        };
        if word
            && (self.execution.pending_field_spaces == 0
                || accepted_row_break
                || !projection.prints_padding)
            && let Some(marker) = native_anchor_marker.as_ref()
        {
            // An accepted marker pass consumes incoming blank positioning
            // at its real endline (term.c:205-217). It bypasses the padding
            // branch below, but still needs this word's stable output owner.
            self.nodes.push(Inline::anchor(marker.clone()));
        }
        if accepted_row_break {
            self.append_restarted_word_padding(
                projection,
                empty_word,
                native_anchor_marker.as_deref(),
                payload,
            )
        } else {
            // Literal rows preserve the actual buffered separator even
            // after a zero-width or empty word. An IR character predicate
            // cannot establish NOSPACE (term.c:573-589); BACKBEFORE may
            // already have consumed the blank. Filled responsive words keep
            // their projected zero-advance joins, but authored leading blanks
            // do not replace term_word()'s separate automatic separator.
            let (spacing_boundary, add_space) = if transferred_automatic {
                (boundary, false)
            } else {
                self.recorded_spacing_boundary(boundary, word, empty_word, add_space, projection)
            };
            // term.c::term_field() skips NBRZW, markers and tab/blank
            // positioning before it writes vbl at an actual Graph. An
            // invisible accepted word must not print and then retire the
            // same minbl again at the next pre_br (term.c:389-427).
            let field_padding =
                self.execution.pending_field_spaces > 0 && projection.prints_padding;
            let add_space = add_space && (native_separator > 0 || earlier_spaces == 0);
            let virtual_padding = !empty_word
                && native_separator == 0
                && self.projects_automatic_space(spacing_boundary, add_space);
            let authored_in_payload = native_separator > 0
                && !field_padding
                && self.projects_automatic_space(spacing_boundary, add_space)
                && reattach_authored_separator(
                    payload,
                    native_anchor_marker.as_deref(),
                    projection.authored_separator.take(),
                );
            if word
                && (field_padding || virtual_padding)
                && let Some(marker) = native_anchor_marker.as_ref()
            {
                self.mark_native_field_prefix(marker);
            }
            if field_padding {
                self.append_device_field_padding();
            } else {
                self.append_boundary_spacing(
                    spacing_boundary,
                    add_space,
                    empty_word,
                    if transferred_automatic {
                        0
                    } else {
                        native_separator
                    },
                    authored_in_payload,
                );
            }
            if word
                && (field_padding || virtual_padding)
                && let Some(marker) = native_anchor_marker
            {
                // Device minbl is field geometry, not a native buffer cell.
                // The word's accepted scalar interval begins after that pad.
                self.nodes.push(Inline::anchor(marker));
            }
            layout_start
        }
    }

    fn append_restarted_word_padding(
        &mut self,
        projection: &mut super::record::WordPassProjection,
        empty_word: bool,
        native_anchor_marker: Option<&str>,
        payload: &mut Vec<Inline>,
    ) -> usize {
        // A consumed \p separator closes the already accepted prefix.
        // Its blank is part of the break, not a new formatter word cell.
        trim_trailing_breakable_spaces(&mut self.nodes, usize::MAX);
        for &end in &projection.closes_before {
            let row_indent = self.take_definition_row_indent();
            super::push_row_break(&mut self.nodes, row_indent);
            self.execution.acknowledge_word_input_pass(end);
        }
        if matches!(
            projection.leading_padding,
            super::record::RestartedPadding::WithEarlierOwners { .. }
        ) && let Some(marker) = native_anchor_marker
        {
            // The borrowed payload is older than the queued empty
            // words. Its held source blanks and recovery text retain
            // their place before those independently owned separators.
            self.nodes.extend(take_word_prefix(payload, marker));
        }
        let layout_start = self.nodes.len();
        // The pass can stop skipping ordinary blanks at TABREF.
        // Only cells before the restarted cursor were consumed.
        let earlier_cells = if empty_word {
            0
        } else {
            self.materialize_pending_word_spaces()
        };
        // A borrowed glyph can put older separators in the same native
        // range. The deferred receipts already printed their owners.
        let leading_cells = projection.leading_padding.remaining(earlier_cells);
        if (earlier_cells > 0 || projection.leading_padding.current_word_cells() > 0)
            && let Some(marker) = native_anchor_marker
        {
            self.nodes.push(Inline::anchor(marker));
        }
        if leading_cells > 0 {
            let authored_in_payload = projection.leading_padding.current_word_cells() > 0
                && reattach_authored_separator(
                    payload,
                    native_anchor_marker,
                    projection.authored_separator.take(),
                );
            let generated = leading_cells - usize::from(authored_in_payload);
            if generated > 0 {
                push_text(&mut self.nodes, " ".repeat(generated));
            }
        }
        layout_start
    }

    fn recorded_spacing_boundary(
        &self,
        boundary: PendingBoundary,
        word: bool,
        empty_word: bool,
        add_space: bool,
        projection: &super::record::WordPassProjection,
    ) -> (PendingBoundary, bool) {
        let native_separator = projection.native_separator;
        if projection.consumed_initial_separator {
            // The word wrote a blank, but an accepted pass or BACKBEFORE
            // consumed it. A presentation fallback cannot count it again.
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
                if native_separator > 0 {
                    PendingBoundary::Preserved
                } else {
                    PendingBoundary::Tight
                },
                native_separator > 0,
            )
        } else if empty_word && self.execution.pending_field_spaces > 0 && native_separator > 0 {
            // A graphless word cannot print deferred vbl, but its
            // term_word() automatic blank is a distinct native cell
            // (term.c:573-589,389-427). Preserve it until a later graph
            // prints the field; pending positioning cannot consume it.
            (PendingBoundary::Preserved, true)
        } else if word
            && native_separator > 0
            && ((empty_word && self.execution.settled_boundary_glyph)
                || !boundary.is_tight()
                || self.execution.zero_advance_joined
                || projection.preserve_retreated_separator)
        {
            // term_word()573-589 buffers this separator independently
            // of projected glyphs and authored leading/trailing blanks.
            // Its write receipt, rather than the last visible IR scalar,
            // proves the cell survived BACKBEFORE. Marker-blank projection
            // has already subtracted its own cells. Only the actual 2→1
            // write/retreat receipt additionally proves that a virtual
            // boundary kept one SENTENCE cell after the cached glyph;
            // other tight presentation joins retain their existing policy.
            (PendingBoundary::Preserved, true)
        } else {
            (boundary, add_space)
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
            self.execution.plain_input.anchors.last_mut()
        };
        if let Some(anchor) = anchor {
            anchor.projected_field_prefix = true;
        }
    }

    fn append_device_field_padding(&mut self) {
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

    fn append_boundary_spacing(
        &mut self,
        boundary: PendingBoundary,
        add_space: bool,
        empty_word: bool,
        automatic_spaces: usize,
        authored_in_payload: bool,
    ) {
        if !self.projects_automatic_space(boundary, add_space) {
            return;
        }
        if empty_word {
            if automatic_spaces > 0 {
                self.defer_current_word_spaces(automatic_spaces);
                self.execution.formatter_column = FormatterColumn::Advanced;
                self.note_produced_formatter_cell(true);
            }
        } else {
            let automatic_spaces = automatic_spaces.max(1);
            let generated = automatic_spaces - usize::from(authored_in_payload);
            if generated > 0 {
                self.push_field_owned_breakable_space(" ".repeat(generated));
            }
            let prior = match self.execution.trailing_output {
                TrailingOutput::BreakableBlank(count) => count,
                _ => 0,
            };
            self.execution.trailing_output =
                TrailingOutput::BreakableBlank(prior.saturating_add(automatic_spaces));
            self.execution.last_visible_character = Some(' ');
            self.execution.has_printable_content = true;
            self.execution.formatter_column = FormatterColumn::Advanced;
        }
    }

    fn projects_automatic_space(&self, boundary: PendingBoundary, add_space: bool) -> bool {
        add_space
            && !boundary.is_tight()
            && (self.execution.spacing.enabled()
                || matches!(
                    boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued | PendingBoundary::Kept
                ))
    }

    fn defer_current_word_spaces(&mut self, count: usize) {
        let anchor = if self.in_definition_field() {
            self.execution
                .definition
                .as_mut()
                .and_then(|state| state.field_word_anchors.last_mut())
        } else {
            self.execution.plain_input.anchors.last_mut()
        };
        if let Some(anchor) = anchor {
            anchor.content = anchor.start;
            self.execution
                .pending_word_spaces
                .push(super::super::DeferredWordSpace {
                    owner: self.execution.native_owner_serial,
                    start: anchor.start,
                    count,
                });
        }
    }

    /// Project the earlier word intervals before starting the next word's
    /// scalar range. Stable markers can re-enter an owner after wrapping,
    /// just like the existing pending-glyph projection.
    pub(super) fn materialize_pending_word_spaces(&mut self) -> usize {
        let mut spaces = std::mem::take(&mut self.execution.pending_word_spaces);
        let mut count = 0usize;
        for pending in spaces.drain(..) {
            use super::super::field_buffer::FieldCell;
            let (buffer, _) = self.execution.word_input();
            let start = pending.start.max(buffer.resume_offset());
            let end = pending
                .start
                .saturating_add(pending.count)
                .min(buffer.cells().len());
            let width = buffer
                .cells()
                .get(start..end)
                .unwrap_or_default()
                .iter()
                .filter(|cell| {
                    matches!(
                        cell,
                        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
                    )
                })
                .count();
            if width == 0 {
                continue;
            }
            self.nodes.push(Inline::anchor(format!(
                "{}{}",
                super::INTERNAL_FIELD_WORD,
                pending.owner
            )));
            self.push_field_owned_breakable_space(" ".repeat(width));
            count = count.saturating_add(width);
        }
        self.execution.pending_word_spaces = spaces;
        if count > 0 {
            let prior = match self.execution.trailing_output {
                TrailingOutput::BreakableBlank(prior) => prior,
                _ => 0,
            };
            self.execution.trailing_output =
                TrailingOutput::BreakableBlank(prior.saturating_add(count));
        }
        count
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

fn take_word_prefix(payload: &mut Vec<Inline>, owner: &str) -> Vec<Inline> {
    let Some(index) = payload
        .iter()
        .position(|node| matches!(node, Inline::Anchor { id, .. } if id.as_str() == owner))
    else {
        return Vec::new();
    };
    let suffix = payload.split_off(index);
    std::mem::replace(payload, suffix)
}

/// An authored surrogate must enter with the word's payload, after the
/// annotation opens and any older borrowed owner has been emitted.
fn reattach_authored_separator(
    payload: &mut Vec<Inline>,
    owner: Option<&str>,
    separator: Option<Inline>,
) -> bool {
    let Some(separator) = separator else {
        return false;
    };
    let position = owner
        .and_then(|owner| {
            payload
                .iter()
                .position(|node| super::native_passes::native_word_marker(node) == Some(owner))
        })
        .map_or(0, |position| position + 1);
    payload.insert(position, separator);
    true
}
