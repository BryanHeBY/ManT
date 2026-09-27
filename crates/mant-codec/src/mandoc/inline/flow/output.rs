use super::{
    FilledBoundary, Font, FormatterColumn, InboundExecution, Inline, InlineBuilder, KeepPhase,
    OutputRollback, OutputTransaction, PendingBoundary, PreservedInlineState, TrailingOutput,
    WordEndBreak, first_visible_character, has_printable_character, last_visible_character,
    needs_boundary_space, push_text,
};

impl InlineBuilder {
    pub(in crate::mandoc) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub(in crate::mandoc) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Save presentation-only state before executing an operand whose
    /// rendered spelling may be replaced. Font, spacing, and zero-advance
    /// state are intentionally not part of this snapshot: they are execution
    /// effects and must survive an eventual compact-output fallback.
    pub(in crate::mandoc) fn begin_output_transaction(&self) -> OutputTransaction {
        OutputTransaction {
            rollback: OutputRollback {
                node_count: self.nodes.len(),
                last_visible_character: self.last_visible_character,
                has_printable_content: self.has_printable_content,
                trailing_output: self.trailing_output,
                pending_breakable_spaces: self.pending_breakable_spaces,
                pending_field_spaces: self.pending_field_spaces,
            },
            inbound: InboundExecution {
                boundary: self.boundary,
                final_source_continuation: self.final_source_continuation,
            },
        }
    }

    /// Whether output since `checkpoint` contains a glyph a reader can use
    /// as a semantic link label. Whitespace-only output is not a label: link
    /// identity must fall back to its visible target instead of wrapping an
    /// invisible click region.
    pub(in crate::mandoc) fn output_since_has_non_whitespace_glyph(
        &self,
        transaction: &OutputTransaction,
    ) -> bool {
        fn contains_glyph(nodes: &[Inline]) -> bool {
            nodes.iter().any(|node| match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    value.chars().any(|character| !character.is_whitespace())
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => contains_glyph(children),
                Inline::Anchor { .. } | Inline::LineBreak => false,
            })
        }
        contains_glyph(&self.nodes[transaction.rollback.node_count..])
    }

    fn rollback_compacted_output(&mut self, rollback: &OutputRollback) {
        self.nodes.truncate(rollback.node_count);
        self.last_visible_character = rollback.last_visible_character;
        self.has_printable_content = rollback.has_printable_content;
        self.trailing_output = rollback.trailing_output;
        self.pending_breakable_spaces = rollback.pending_breakable_spaces;
        self.pending_field_spaces = rollback.pending_field_spaces;
    }

    /// Drop a compactly hidden operand's output while preserving the
    /// formatter transitions it performed.  In particular, `\\c` changes the
    /// next word's boundary and a literal row advances its source cursor;
    /// those are execution facts even though a semantic macro may replace the
    /// operand's visible spelling.
    pub(in crate::mandoc) fn discard_output_preserving_execution(
        &mut self,
        transaction: &OutputTransaction,
    ) {
        let retained_layout =
            retained_hidden_layout(&self.nodes[transaction.rollback.node_count..]);
        self.rollback_compacted_output(&transaction.rollback);
        // Semantic compaction may replace an operand's glyphs, never its
        // layout. A word-end `\\p` is realized by the shared builder; retain
        // that result outside a subsequently visible fallback link so a
        // cursor that already consumed the source row cannot erase it.
        self.append_retained_layout(retained_layout);
    }

    /// Wrap the output emitted since `checkpoint` without replaying its
    /// formatter execution. Links and other semantic wrappers are IR
    /// annotations over an already-executed source stream.
    pub(in crate::mandoc) fn wrap_output_since(
        &mut self,
        transaction: &OutputTransaction,
        wrap: impl FnOnce(Vec<Inline>) -> Vec<Inline>,
    ) {
        let output = self.nodes.split_off(transaction.rollback.node_count);
        self.nodes.extend(wrap(output));
    }

    /// Replace the visible glyphs emitted since `checkpoint` without
    /// replaying formatter execution.
    ///
    /// Semantic macros such as `.Bx` execute authored operands and then use
    /// a generated spelling for presentation.  The replacement occupies the
    /// already-executed formatter word: leading inter-word padding and real
    /// line boundaries survive, while pending `\p`, `\c`, font, KEEP, and
    /// zero-advance state remain exactly where source execution left them.
    pub(in crate::mandoc) fn replace_output_since(
        &mut self,
        transaction: &OutputTransaction,
        replacement: &str,
    ) {
        let output = self.nodes.split_off(transaction.rollback.node_count);
        let retained = retained_replacement_layout(output);
        let boundary_materialized =
            has_printable_character(&retained) || line_break_count(&retained) > 0;
        let inbound_continued = transaction.inbound.boundary.is_tight()
            || transaction.inbound.boundary == PendingBoundary::Continued
            || transaction
                .inbound
                .final_source_continuation
                .unwrap_or(false);
        let replacement_word_spaces = if inbound_continued {
            // An explicit empty formatter word consumes incoming `\c` before
            // CVS post_bx()'s generated Ns + BSD sequence.  Padding queued by
            // that empty word is therefore not presentation content.
            transaction.rollback.pending_breakable_spaces
        } else {
            self.pending_breakable_spaces
        };

        // These fields summarize projected output rather than formatter
        // execution.  Rewind only them before installing the replacement;
        // boundary, cursor, word-end break, KEEP, and zero-advance state must
        // remain the post-execution values.
        self.last_visible_character = transaction.rollback.last_visible_character;
        self.has_printable_content = transaction.rollback.has_printable_content;
        self.append_projected(retained);
        if !boundary_materialized && replacement_word_spaces > 0 {
            self.append_projected(vec![Inline::Text {
                value: " ".repeat(replacement_word_spaces),
            }]);
        } else if !boundary_materialized
            && !inbound_continued
            && self.empty_word
            && transaction.rollback.has_printable_content
            && self.spacing.enabled()
            && !self.boundary.is_tight()
        {
            self.append_projected(vec![Inline::Text {
                value: " ".to_owned(),
            }]);
        }
        // The replacement occupies the already executed word. Any deferred
        // empty-word padding has now been materialized exactly once and must
        // not leak into the following source word.
        self.empty_word = false;
        self.trailing_output = TrailingOutput::None;
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
        // The validator-generated replacement is a formatter word, not an
        // inert IR splice. It consumes authored `\z`/`\c` state while a
        // deferred `\p` remains pending until the next real word boundary.
        // It occupies the source word being replaced, so that deferred break
        // belongs after the replacement rather than before it.
        self.tighten_next_boundary();
        self.append_generated_word(replacement);
    }

    /// A compact semantic spelling can omit an authored empty trailing word
    /// after its generated punctuation.  The omitted word still establishes
    /// one ordinary boundary, but padding already queued on its left must not
    /// be replayed in addition to the following word's own boundary.
    pub(in crate::mandoc) fn consume_compacted_pending_padding(&mut self) {
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
    }

    /// Preserve a formatter-requested line boundary without creating empty
    /// leading, repeated, or trailing rows around the paragraph.
    pub(in crate::mandoc) fn hard_break(&mut self) {
        // term_newln() flushes only an occupied terminal cell. A completed
        // `\zX` glyph and a buffered `\p` both advanced the native buffer;
        // a bare armed `\z` did not and remains ordered before the next word.
        if !self.has_formatter_cell() {
            return;
        }
        self.flush_zero_advance();
        let current_row_has_printable = self
            .nodes
            .iter()
            .rev()
            .take_while(|node| !matches!(node, Inline::LineBreak))
            .any(|node| has_printable_character(std::slice::from_ref(node)));
        if self.formatter_column == FormatterColumn::Advanced && !current_row_has_printable {
            // ESCAPE_IGNORE (`\&`) advances the native buffer without a
            // visible glyph. A real line boundary must retain that physical
            // row, even though renderer-neutral IR has no character for it.
            self.nodes.push(Inline::Text {
                value: String::new(),
            });
        }
        self.boundary = PendingBoundary::Ordinary;
        self.empty_word = false;
        self.trailing_output = TrailingOutput::None;
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.formatter_column = FormatterColumn::Origin;
        if !matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.push(Inline::LineBreak);
            self.last_visible_character = Some('\n');
        }
        if let Some(cursor) = &mut self.source_cursor {
            cursor.explicit_line_break(false);
        }
        self.final_word_join = Some(false);
        self.final_source_continuation = Some(false);
    }

    pub(in crate::mandoc) fn has_formatter_cell(&self) -> bool {
        self.formatter_column == FormatterColumn::Advanced
            || self.zero_advance.has_buffered_glyph()
            || self.word_end_break == WordEndBreak::Pending
    }

    pub(in crate::mandoc) fn has_invisible_formatter_cell(&self) -> bool {
        self.formatter_column == FormatterColumn::Advanced
            && !self
                .nodes
                .iter()
                .rev()
                .take_while(|node| !matches!(node, Inline::LineBreak))
                .any(|node| has_printable_character(std::slice::from_ref(node)))
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
        self.append_at_boundary(
            &mut incoming,
            true,
            occupies_row,
            trailing_output == TrailingOutput::FixedBlank,
        );
        self.materialize_boundary_before_pending_glyph();
        if trailing_output != TrailingOutput::None {
            self.trailing_output = trailing_output;
        }
    }

    /// Keep the formatter's write order when this word armed BACKBEFORE.
    ///
    /// `term_word()` buffers the word boundary before decoding `\zX`.  The
    /// following word then overwrites its own boundary with X; postponing the
    /// first boundary until that point incorrectly moves it after X.  Commit
    /// only the already executed padding here and leave the glyph pending.
    fn materialize_boundary_before_pending_glyph(&mut self) {
        if !self.zero_advance.has_pending_glyph() {
            return;
        }
        let breakable = std::mem::take(&mut self.pending_breakable_spaces);
        let field = std::mem::take(&mut self.pending_field_spaces);
        let count = breakable.saturating_add(field);
        if count == 0 {
            return;
        }
        self.append_projected(vec![Inline::Text {
            value: " ".repeat(count),
        }]);
        self.trailing_output = if field > 0 {
            TrailingOutput::FieldBlank(field)
        } else {
            TrailingOutput::BreakableBlank(breakable)
        };
        self.formatter_column = FormatterColumn::Advanced;
    }

    pub(in crate::mandoc) fn execute_empty_word(&mut self) {
        self.begin_word_projection(true);
        self.append_word(Vec::new());
    }

    /// Generated glyphs use the effective font just like authored text, but
    /// are not reparsed as roff source (names can contain literal escapes).
    pub(in crate::mandoc) fn append_text(&mut self, value: &str) {
        self.begin_word_projection(!value.is_empty());
        let kept_zero_boundary = self.boundary == PendingBoundary::Kept
            && value
                .chars()
                .next()
                .is_some_and(super::super::is_formatter_word_blank)
            && self.zero_advance.has_pending_glyph();
        let mut projected = Vec::new();
        self.zero_advance
            .append_generated_text(value, &mut projected, self.font.display_current());
        if kept_zero_boundary {
            // TERMP_KEEP inserts a non-breaking formatter blank.  A pending
            // BACKBEFORE glyph consumes that cell, so retain the glyph while
            // suppressing both the virtual blank and ordinary boundary.
            self.boundary = PendingBoundary::Tight;
        }
        // Every formatter-generated spelling is a real `term_word()` call.
        // Even when a preceding bare `\z` buffers its only glyph, the word
        // must consume PrefixJoin/Tight and establish the boundary seen by
        // the following source operand.
        self.append_word(projected);
        // Generated formatter words (for example Lk's colon or enclosure
        // delimiters) cannot themselves carry source `\\c`; they consume a
        // preceding continuation before the next source operand runs.
        if !value.is_empty() {
            self.final_word_join = Some(false);
        }
    }

    /// Execute validator-generated text as a complete formatter word.
    ///
    /// This differs from appending generated punctuation inside an existing
    /// word: `term_word()` consumes an authored `\c`, participates in deferred
    /// `\p` handling, and closes physical source continuation before the next
    /// source node. CVS uses this path for the generated `BSD` child of `.Bx`.
    pub(in crate::mandoc) fn append_generated_word(&mut self, value: &str) {
        self.begin_word_projection(!value.is_empty());
        let mut projected = Vec::new();
        self.zero_advance
            .append_generated_text(value, &mut projected, self.font.display_current());
        self.append_word(projected);
        if !value.is_empty() {
            if let Some(cursor) = &mut self.source_cursor {
                cursor.continue_line(false);
            }
            self.final_word_join = Some(false);
            self.final_source_continuation = Some(false);
        }
    }

    /// Start an ordinary formatter word after a source text node. If a prior
    /// `\z` glyph is pending, model term.c's implicit blank before decoding
    /// the next glyph: preserve the pending glyph but suppress this reader's
    /// own inter-word space. Tight joins (for example alternating `.BR`
    /// operands) deliberately bypass this transition and overstrike instead.
    pub(in crate::mandoc) fn begin_word_projection(&mut self, next_is_visible: bool) {
        if next_is_visible {
            // term_word() clears skipvsp before consuming the word itself.
            self.vertical_space_debt = 0;
            self.execution_epoch = self.execution_epoch.wrapping_add(1);
        }
        let continued_word = next_is_visible
            && self.final_source_continuation_or(false)
            && !self.boundary.is_tight();
        if next_is_visible {
            self.final_word_join = Some(false);
            self.final_source_continuation = Some(false);
        }
        if next_is_visible && self.keep.keeping() && !self.boundary.is_tight() {
            self.boundary = PendingBoundary::Kept;
            if let Some(glyph) = self.zero_advance.resolve_at_word_boundary() {
                // CVS writes TERMP_KEEP's implicit NBRSP before the next
                // glyph. It settles BACKBEFORE without becoming visible, so
                // retain the glyph and join the incoming formatter word.
                self.append_projected(vec![glyph]);
                self.boundary = PendingBoundary::Tight;
            }
        }
        if next_is_visible
            && !self.boundary.is_nonbreaking()
            && self.word_end_break == WordEndBreak::Pending
            && (self.spacing.enabled()
                || matches!(
                    self.boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued
                ))
        {
            if let Some(glyph) = self.zero_advance.resolve_at_word_boundary() {
                // The formatter's automatic word blank settles BACKBEFORE
                // before a buffered `\\p` can become a line boundary.  Keep
                // the glyph and join the incoming word at that position.
                self.append_projected(vec![glyph]);
                self.boundary = PendingBoundary::Tight;
            } else {
                self.hard_break();
            }
        }
        if continued_word && !self.boundary.is_tight() {
            // TERMP_NONEWLINE suppresses the physical source break while an
            // `.Ec`-style release permits the normal formatter blank.  An
            // authored leading blank is retained separately by the incoming
            // word, producing the two spaces emitted by CVS in both filled
            // and literal flows.
            self.boundary = PendingBoundary::Continued;
        }
        if next_is_visible && self.keep.phase == KeepPhase::PreKeep {
            // term_word() inserts the leading boundary using the *previous*
            // KEEP value, then promotes PREKEEP. Execute that ordering before
            // decoding the word: a bare BACKAFTER survives the ordinary blank,
            // while BACKBEFORE consumes it and retains its buffered glyph.
            if !self.boundary.is_nonbreaking()
                && (self.formatter_column == FormatterColumn::Advanced
                    || self.zero_advance.has_pending_glyph())
                && (self.spacing.enabled() || matches!(self.boundary, PendingBoundary::Preserved))
            {
                if let Some(glyph) = self.zero_advance.resolve_at_word_boundary() {
                    self.append_projected(vec![glyph]);
                } else {
                    self.append_projected(vec![Inline::Text {
                        value: " ".to_owned(),
                    }]);
                    self.formatter_column = FormatterColumn::Advanced;
                }
                self.boundary = PendingBoundary::Tight;
            }
            self.keep.phase = KeepPhase::Keep;
        }
        if !next_is_visible
            || self.boundary.is_nonbreaking()
            || !(self.has_printable_content || self.zero_advance.has_pending_glyph())
            || !(self.spacing.enabled() || matches!(self.boundary, PendingBoundary::Preserved))
        {
            return;
        }
        let Some(glyph) = self.zero_advance.resolve_at_word_boundary() else {
            return;
        };
        self.append_projected(vec![glyph]);
        self.boundary = PendingBoundary::Tight;
    }

    /// Only macros that select a font establish this scope. Transparent
    /// macros and text operands must not invent a push/pop of their own.
    pub(in crate::mandoc) fn with_font_scope(
        &mut self,
        font: Font,
        append: impl FnOnce(&mut Self),
    ) {
        let saved = self.font.push_scope(font);
        append(self);
        self.font.pop_scope(saved);
    }

    /// Style newly appended content without creating a new formatter state.
    /// The transform must preserve visible characters and line boundaries.
    /// In particular, a wrapper ending after `\\c` must not consume its
    /// pending join merely because its content is represented as a subtree.
    pub(in crate::mandoc) fn append_scope(
        &mut self,
        append: impl FnOnce(&mut Self),
        style: impl FnOnce(Vec<Inline>) -> Vec<Inline>,
    ) {
        // Appending must still see the prefix, especially a preceding hard
        // break used for deduplication. Style only the new suffix afterwards.
        let start = self.nodes.len();
        append(self);
        let inner = self.nodes.split_off(start);
        self.nodes.extend(style(inner));
    }

    /// Append content using the formatter-level boundary selected by the
    /// block lowering pass.
    pub(in crate::mandoc) fn append_filled(
        &mut self,
        incoming: Vec<Inline>,
        boundary: FilledBoundary,
    ) {
        match boundary {
            FilledBoundary::SameLine => self.append(incoming),
            FilledBoundary::Word => {
                let mut incoming = incoming;
                self.append_at_boundary(&mut incoming, false, true, false);
            }
            FilledBoundary::LineBreak => {
                self.hard_break();
                self.append(incoming);
            }
        }
    }

    fn append_at_boundary(
        &mut self,
        incoming: &mut Vec<Inline>,
        word: bool,
        occupies_row: bool,
        incoming_starts_with_fixed_blank: bool,
    ) {
        if incoming.is_empty() && !word {
            return;
        }
        let incoming_first = first_visible_character(incoming);
        let incoming_last = last_visible_character(incoming);
        let incoming_has_printable = has_printable_character(incoming);
        let incoming_has_line_break = line_break_count(incoming) > 0;
        if incoming_has_printable || word {
            self.execution_epoch = self.execution_epoch.wrapping_add(1);
        }
        if incoming_first.is_none() && !incoming_has_printable && !word {
            self.nodes.append(incoming);
            return;
        }
        if let Some(cursor) = &mut self.source_cursor {
            let pending = cursor.pending();
            let empty_row = pending && incoming.is_empty() && word && occupies_row;
            if cursor.word(occupies_row) {
                self.nodes.push(Inline::LineBreak);
                self.last_visible_character = Some('\n');
                self.boundary = PendingBoundary::Ordinary;
                self.trailing_output = TrailingOutput::None;
                self.pending_breakable_spaces = 0;
                self.pending_field_spaces = 0;
                self.empty_word = false;
            }
            // The previous physical-line decision is consumed before CVS
            // term_word() clears TERMP_NONEWLINE for the current word.
            cursor.continue_line(false);
            if empty_row {
                // Empty executed rows are content, including at a display's
                // start/end. Do not turn them into inter-word padding.
                self.nodes.push(Inline::Text {
                    value: String::new(),
                });
                // `term_word()` has nevertheless advanced the native output
                // column.  Later `.ti` and `.mc` requests must observe this
                // formatter cell even though it has no visible glyph.
                self.formatter_column = FormatterColumn::Advanced;
                return;
            }
            if pending && !occupies_row && incoming.is_empty() {
                return;
            }
        }
        if (incoming_has_printable || word)
            && (self.pending_definition_indent().is_some() || self.pending_line_indent > 0)
        {
            let cells = self
                .definition
                .as_mut()
                .and_then(|state| state.pending_indent.take())
                .unwrap_or_else(|| std::mem::take(&mut self.pending_line_indent));
            self.append_fixed_cells(cells);
            self.boundary = PendingBoundary::Tight;
        }
        let empty_word = word && incoming_first.is_none() && !incoming_has_printable;
        let boundary = std::mem::replace(&mut self.boundary, PendingBoundary::Ordinary);
        let fixed_blank_boundary = (incoming_starts_with_fixed_blank
            && incoming_first.is_some_and(char::is_whitespace))
            || self.trailing_output == TrailingOutput::FixedBlank;
        let add_space = if fixed_blank_boundary {
            self.has_printable_content
        } else if (matches!(boundary, PendingBoundary::Continued)
            && incoming_first.is_some_and(char::is_whitespace))
            || empty_word
            || self.empty_word
        {
            self.has_printable_content || self.trailing_output != TrailingOutput::None
        } else {
            needs_boundary_space(self.last_visible_character, incoming_first)
        };
        self.append_boundary_spacing(boundary, add_space, word, empty_word);
        let appended_start = self.nodes.len();
        self.nodes.append(incoming);
        if line_break_count(&self.nodes[appended_start..]) > 0 {
            // `incoming` has moved, so inspect the tail already appended.
            // An explicit break is an executed row transition, independent
            // of the next source node's physical line number.
            if let Some(cursor) = &mut self.source_cursor {
                cursor.explicit_line_break(incoming_last != Some('\n'));
            }
        }
        if incoming_last.is_some() {
            self.last_visible_character = incoming_last;
            // Generic projected words are formatter glyphs, not trim-eligible
            // source padding. Raw text execution supplies its precise class
            // through `append_word_with_literal_row()` above.
            self.trailing_output = if incoming_last.is_some_and(char::is_whitespace) {
                TrailingOutput::BoundaryBlank
            } else {
                TrailingOutput::NonBlank
            };
        }
        self.has_printable_content |= incoming_has_printable;
        if incoming_has_line_break {
            self.formatter_column =
                if incoming_has_printable && !matches!(incoming_last, Some('\n')) {
                    FormatterColumn::Advanced
                } else {
                    FormatterColumn::Origin
                };
        } else if incoming_has_printable || occupies_row {
            self.formatter_column = FormatterColumn::Advanced;
        }
        self.empty_word = empty_word;
    }

    fn append_boundary_spacing(
        &mut self,
        boundary: PendingBoundary,
        add_space: bool,
        formatter_word: bool,
        empty_word: bool,
    ) {
        let materialized_field_separator = formatter_word && self.pending_field_spaces > 0;
        if !empty_word && self.pending_breakable_spaces > 0 {
            push_text(&mut self.nodes, " ".repeat(self.pending_breakable_spaces));
            let prior = match self.trailing_output {
                TrailingOutput::BreakableBlank(count) => count,
                _ => 0,
            };
            self.trailing_output =
                TrailingOutput::BreakableBlank(prior.saturating_add(self.pending_breakable_spaces));
            self.pending_breakable_spaces = 0;
        }
        if materialized_field_separator {
            let count = self.pending_field_spaces;
            push_text(&mut self.nodes, " ".repeat(count));
            self.pending_field_spaces = 0;
            self.trailing_output = TrailingOutput::FieldBlank(count);
            self.last_visible_character = Some(' ');
            self.formatter_column = FormatterColumn::Advanced;
        }
        if !(self.spacing.enabled()
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
            // authored term content. Materialize it at the next glyph.
            if self.last_visible_character != Some('\n') {
                self.pending_breakable_spaces = self.pending_breakable_spaces.saturating_add(1);
            }
        } else {
            push_text(&mut self.nodes, " ".to_owned());
            let prior = match self.trailing_output {
                TrailingOutput::BreakableBlank(count) => count,
                _ => 0,
            };
            self.trailing_output = TrailingOutput::BreakableBlank(prior.saturating_add(1));
            self.last_visible_character = Some(' ');
            self.has_printable_content = true;
            self.formatter_column = FormatterColumn::Advanced;
        }
    }

    pub(in crate::mandoc) fn finish(mut self) -> Vec<Inline> {
        self.flush_zero_advance();
        self.finish_nodes()
    }

    /// Finish one native formatter line and return a bare `\\z` request only
    /// when no formatter cell was occupied.
    ///
    /// CVS `term_newln()` calls `term_flushln()` only after text, a pending
    /// glyph, or another formatter event occupied the current cell.  That
    /// flush clears both backtracking flags.  An otherwise empty line leaves
    /// `TERMP_BACKAFTER` live for the next formatter word.  Returning the
    /// surviving flag together with the committed output prevents callers
    /// from exporting state before this boundary has executed.
    pub(in crate::mandoc) fn finish_formatter_line(mut self) -> (Vec<Inline>, bool) {
        let surviving_armed = if self.has_formatter_cell() {
            false
        } else {
            self.zero_advance.take_armed()
        };
        self.flush_zero_advance();
        (self.finish_nodes(), surviving_armed)
    }

    /// Return an inner scope without forcing a pending `\\z` glyph to become
    /// visible.  CVS mandoc carries its backtracking flags through nested
    /// `term_word()` calls, so the caller must continue the state in the
    /// surrounding inline stream before committing it at a real boundary.
    pub(in crate::mandoc) fn finish_preserving_execution(
        mut self,
    ) -> (Vec<Inline>, PreservedInlineState) {
        let formatter_cell_occupied = self.has_formatter_cell();
        let state = PreservedInlineState {
            zero_advance: std::mem::take(&mut self.zero_advance),
            word_end_break: self.word_end_break == WordEndBreak::Pending,
            source_continuation: self.final_source_continuation,
            formatter_cell_occupied,
            pending_line_indent: self.pending_line_indent,
            pending_definition_indent: self.pending_definition_indent(),
            last_executed_source_line: self.last_executed_source_line,
        };
        (self.finish_nodes(), state)
    }

    fn finish_nodes(&mut self) -> Vec<Inline> {
        self.word_end_break = WordEndBreak::Clear;
        if let Some(cursor) = &self.source_cursor {
            if !cursor.row_occupied()
                && let Some(last) = self
                    .nodes
                    .iter()
                    .rposition(|node| !matches!(node, Inline::Anchor { .. }))
                && matches!(self.nodes[last], Inline::LineBreak)
            {
                self.nodes.remove(last);
            }
            return std::mem::take(&mut self.nodes);
        }
        while matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.pop();
        }
        std::mem::take(&mut self.nodes)
    }

    pub(super) fn flush_zero_advance(&mut self) {
        let mut pending = Vec::new();
        self.zero_advance.finish_into(&mut pending);
        if !pending.is_empty()
            && (self.pending_breakable_spaces > 0 || self.pending_field_spaces > 0)
        {
            let spaces = std::mem::take(&mut self.pending_breakable_spaces)
                .saturating_add(std::mem::take(&mut self.pending_field_spaces));
            self.append_projected(vec![Inline::Text {
                value: " ".repeat(spaces),
            }]);
        }
        self.append_projected(pending);
    }

    pub(super) fn append_projected(&mut self, mut incoming: Vec<Inline>) {
        if incoming.is_empty() {
            return;
        }
        let last = last_visible_character(&incoming);
        let printable = has_printable_character(&incoming);
        self.nodes.append(&mut incoming);
        if last.is_some() {
            self.last_visible_character = last;
            self.trailing_output = if last.is_some_and(char::is_whitespace) {
                TrailingOutput::BoundaryBlank
            } else {
                TrailingOutput::NonBlank
            };
        }
        self.has_printable_content |= printable;
    }

    fn append_retained_layout(&mut self, retained: Vec<Inline>) {
        if retained.is_empty() {
            return;
        }
        if last_visible_character(&retained) == Some('\n') {
            self.last_visible_character = Some('\n');
            self.trailing_output = TrailingOutput::None;
        }
        self.nodes.extend(retained);
    }

    pub(super) fn retain_line_breaks(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.nodes
            .extend(std::iter::repeat_n(Inline::LineBreak, count));
        if let Some(cursor) = &mut self.source_cursor {
            cursor.explicit_line_break(false);
        }
        self.last_visible_character = Some('\n');
        self.formatter_column = FormatterColumn::Origin;
        self.empty_word = false;
        self.trailing_output = TrailingOutput::None;
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
    }
}

/// Count formatter-breakable ASCII blanks at the end of the current field.
/// Generated fixed cells reset this accounting at their call sites, while
/// non-breaking spaces remain distinguishable by their Unicode value.
pub(in crate::mandoc::inline) fn trailing_ascii_spaces(nodes: &[Inline]) -> usize {
    fn visit(nodes: &[Inline], count: &mut usize) -> bool {
        for node in nodes.iter().rev() {
            match node {
                Inline::Anchor { .. } => {}
                Inline::LineBreak => return false,
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    let trailing = value.chars().rev().take_while(|&ch| ch == ' ').count();
                    *count = count.saturating_add(trailing);
                    if trailing != value.chars().count() {
                        return false;
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    if !visit(children, count) {
                        return false;
                    }
                }
            }
        }
        true
    }

    let mut count = 0;
    visit(nodes, &mut count);
    count
}

/// Apply CVS `term_field()` trailing-blank trimming without touching fixed
/// run-in cells that were generated by the formatter rather than by a word.
pub(super) fn trim_trailing_breakable_spaces(nodes: &mut Vec<Inline>, count: usize) {
    fn trim(nodes: &mut Vec<Inline>, remaining: &mut usize) -> bool {
        let mut index = nodes.len();
        while index > 0 && *remaining > 0 {
            index -= 1;
            let remove = match &mut nodes[index] {
                Inline::Anchor { .. } => continue,
                Inline::LineBreak => return false,
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    let original_len = value.len();
                    while *remaining > 0 && value.ends_with(' ') {
                        value.pop();
                        *remaining -= 1;
                    }
                    let remove = value.is_empty() && original_len > 0;
                    if !remove && !value.ends_with(' ') {
                        return false;
                    }
                    remove
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    if !trim(children, remaining) {
                        return false;
                    }
                    children.is_empty()
                }
            };
            if remove {
                nodes.remove(index);
            }
        }
        *remaining > 0
    }

    let mut remaining = count;
    trim(nodes, &mut remaining);
}

/// Count layout instructions recursively before discarding a compact operand.
/// Styling and link wrappers are presentation-only, but their explicit line
/// breaks are formatter output and must survive without retaining the hidden
/// operand's glyphs.
fn line_break_count(nodes: &[Inline]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Inline::LineBreak => 1,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => line_break_count(children),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::Anchor { .. } => 0,
        })
        .sum()
}

/// Keep identity anchors and explicit layout emitted by hidden source in
/// their original order. These nodes are projected facts, not formatter
/// events: the source cursor and execution registers already reflect them.
fn retained_hidden_layout(nodes: &[Inline]) -> Vec<Inline> {
    let mut retained = Vec::new();
    for node in nodes {
        match node {
            Inline::Anchor { .. } => retained.push(node.clone()),
            Inline::LineBreak => retained.push(Inline::LineBreak),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => retained.extend(retained_hidden_layout(children)),
            Inline::Text { .. } | Inline::Code { .. } | Inline::Equation { .. } => {}
        }
    }
    retained
}

/// Retain layout surrounding a compact semantic replacement.
///
/// Padding before the first authored glyph belongs to the caller.  Explicit
/// line boundaries are execution output and likewise cannot be discarded.
/// Glyphs and styling inside the replaced source spelling are omitted.
fn retained_replacement_layout(nodes: Vec<Inline>) -> Vec<Inline> {
    fn leading_whitespace(value: &str) -> Option<Inline> {
        let end = value
            .char_indices()
            .take_while(|(_, character)| character.is_whitespace())
            .last()
            .map_or(0, |(index, character)| index + character.len_utf8());
        (end > 0).then(|| Inline::Text {
            value: value[..end].to_owned(),
        })
    }

    let mut retained = Vec::new();
    let mut before_first_glyph = true;
    for node in nodes {
        match node {
            Inline::LineBreak => {
                retained.push(Inline::LineBreak);
                before_first_glyph = true;
            }
            Inline::Anchor { .. } => retained.push(node),
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. }
                if before_first_glyph =>
            {
                if let Some(prefix) = leading_whitespace(&value) {
                    retained.push(prefix);
                }
                if value.chars().any(|character| !character.is_whitespace()) {
                    before_first_glyph = false;
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. }
                if before_first_glyph =>
            {
                if has_printable_character(&children) {
                    before_first_glyph = false;
                }
            }
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::Strong { .. }
            | Inline::Emphasis { .. }
            | Inline::Link { .. } => {}
        }
    }
    retained
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_layout_summary_ignores_trailing_anchor() {
        let mut builder = InlineBuilder::with_spacing(true);
        builder.append_text("prefix");
        builder.append_retained_layout(vec![Inline::LineBreak, Inline::anchor("mark")]);

        assert_eq!(builder.last_visible_character, Some('\n'));
        assert_eq!(builder.trailing_output, TrailingOutput::None);
        assert!(matches!(
            builder.nodes.as_slice(),
            [Inline::Text { value }, Inline::LineBreak, Inline::Anchor { .. }] if value == "prefix"
        ));
    }
}
