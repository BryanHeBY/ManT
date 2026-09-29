use super::{
    AuthorBreakEffect, FilledBoundary, Font, FormatterColumn, InboundExecution, Inline,
    InlineBuilder, KeepPhase, OutputRollback, OutputTransaction, PendingBoundary,
    PreservedInlineState, TrailingOutput, WordEndBreak, has_printable_character,
    last_visible_character,
};

// A private boundary carried only while one authored Link spans two native
// term_flushln() fields. It is removed before any IR owner is returned.
const INTERNAL_LINK_SPLIT: &str = "\0mant:field-link-split";

mod boundary;
mod feed;
mod record;
mod split;

impl InlineBuilder {
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
                last_visible_character: self.execution.last_visible_character,
                has_printable_content: self.execution.has_printable_content,
                trailing_output: self.execution.trailing_output,
                pending_breakable_spaces: self.execution.pending_breakable_spaces,
                pending_field_spaces: self.execution.pending_field_spaces,
            },
            inbound: InboundExecution {
                boundary: self.execution.boundary,
                final_source_continuation: self.execution.final_source_continuation,
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
                Inline::Anchor { .. } | Inline::LineBreak { .. } => false,
            })
        }
        contains_glyph(&self.nodes[transaction.rollback.node_count..])
    }

    fn rollback_compacted_output(&mut self, rollback: &OutputRollback) {
        self.nodes.truncate(rollback.node_count);
        if let Some(author) = &mut self.execution.author_execution {
            author.field_output_start = author.field_output_start.min(self.nodes.len());
        }
        self.execution.last_visible_character = rollback.last_visible_character;
        self.execution.has_printable_content = rollback.has_printable_content;
        self.execution.trailing_output = rollback.trailing_output;
        self.execution.pending_breakable_spaces = rollback.pending_breakable_spaces;
        self.execution.pending_field_spaces = rollback.pending_field_spaces;
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
        wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        self.wrap_output_from(transaction.rollback.node_count, wrap);
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
            self.execution.pending_breakable_spaces
        };

        // These fields summarize projected output rather than formatter
        // execution.  Rewind only them before installing the replacement;
        // boundary, cursor, word-end break, KEEP, and zero-advance state must
        // remain the post-execution values.
        self.execution.last_visible_character = transaction.rollback.last_visible_character;
        self.execution.has_printable_content = transaction.rollback.has_printable_content;
        self.append_projected(retained);
        if !boundary_materialized && replacement_word_spaces > 0 {
            self.append_projected(vec![Inline::Text {
                value: " ".repeat(replacement_word_spaces),
            }]);
        } else if !boundary_materialized
            && !inbound_continued
            && self.execution.empty_word
            && transaction.rollback.has_printable_content
            && self.execution.spacing.enabled()
            && !self.execution.boundary.is_tight()
        {
            self.append_projected(vec![Inline::Text {
                value: " ".to_owned(),
            }]);
        }
        // The replacement occupies the already executed word. Any deferred
        // empty-word padding has now been materialized exactly once and must
        // not leak into the following source word.
        self.execution.empty_word = false;
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        // The validator-generated replacement is a formatter word, not an
        // inert IR splice. It consumes authored `\z`/`\c` state while a
        // deferred `\p` remains pending until the next real word boundary.
        // It occupies the source word being replaced, so that deferred break
        // belongs after the replacement rather than before it.
        self.tighten_next_boundary();
        self.append_generated_word(replacement);
    }

    /// Preserve a formatter-requested line boundary without creating empty
    /// leading, repeated, or trailing rows around the paragraph.
    pub(in crate::mandoc) fn hard_break(&mut self) {
        let exited_discarded_buffer = self.discarded_exited_definition_buffer();
        let exited_definition_row = self
            .execution
            .definition
            .as_ref()
            .is_some_and(|definition| {
                definition.hang_row.viscol > 0
                    && self
                        .execution
                        .author_execution
                        .as_ref()
                        .is_some_and(|execution| {
                            matches!(execution.break_effect, super::AuthorBreakEffect::Line)
                        })
            });
        self.discard_unprinted_definition_field_output();
        if !self.execution.has_printable_content {
            self.execution.leading_line_boundary = super::LeadingLineBoundary::BeforeVisibleWord;
        }
        // term_newln() flushes only an occupied terminal cell. A completed
        // `\zX` glyph and a buffered `\p` both advanced the native buffer;
        // a bare armed `\z` did not and remains ordered before the next word.
        if !self.has_formatter_cell() && !exited_definition_row && !exited_discarded_buffer {
            if self.external_head_row_pending {
                // term_newln() still flushes the detached native tag row.
                // term_flushln() clears a bare \\z before the next BODY word.
                self.execution.zero_advance.discard_at_row_end();
                self.external_head_row_pending = false;
            }
            return;
        }
        self.flush_zero_advance();
        self.external_head_row_pending = false;
        if self.execution.completed_vertical_rows > 0
            && self.execution.formatter_column == FormatterColumn::Advanced
            && !self
                .nodes
                .iter()
                .rev()
                .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
                .any(|node| has_non_whitespace_glyph(std::slice::from_ref(node)))
        {
            // term_newln() commits a whitespace-only formatter row even
            // though term_fill() prints no glyphs. When a later empty TEXT
            // requests another term_vspace(), that committed row must remain
            // in the completed-row count after formatter_column resets.
            self.execution.completed_vertical_rows =
                self.execution.completed_vertical_rows.saturating_add(1);
        }
        let current_row_has_printable = self
            .nodes
            .iter()
            .rev()
            .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
            .any(|node| has_printable_character(std::slice::from_ref(node)));
        if (self.execution.formatter_column == FormatterColumn::Advanced
            || (self.execution.word_end_break == WordEndBreak::Pending
                && matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))))
            && !current_row_has_printable
        {
            // ESCAPE_IGNORE (`\&`) and ESCAPE_BREAK (`\p`) can occupy a
            // fresh native buffer after an earlier line was already closed.
            // Its own term_newln() must remain distinct from that break.
            self.nodes.push(Inline::Text {
                value: String::new(),
            });
        }
        self.execution.boundary = PendingBoundary::Ordinary;
        self.execution.empty_word = false;
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.formatter_column = FormatterColumn::Origin;
        if (exited_discarded_buffer && !exited_definition_row)
            || !matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))
        {
            let row_indent = self.take_definition_row_indent();
            self.nodes.push(Inline::line_break_indented(row_indent));
            self.execution.last_visible_character = Some('\n');
            if std::env::var_os("MANT_DBG_D1").is_some() {
                eprintln!("hard_break PUSHED lb");
            }
        }
        self.execution.final_word_join = Some(false);
        if let Some(definition) = &mut self.execution.definition {
            // A committed hard break closes the native device row even when
            // the HEAD is still in its TAG field. Otherwise an overrun tag
            // leaves stale viscol for a later discarded buffer.
            definition.hang_row.endline();
        }
        if let Some(author) = &mut self.execution.author_execution {
            // term_flushln() commits its accepted prefix before it starts
            // another field. A later nbr=0 may discard only the new suffix.
            author.field_output_start = self.nodes.len();
        }
        // term_newln() does not clear TERMP_NONEWLINE; the next word does.
    }

    pub(in crate::mandoc) fn has_formatter_cell(&self) -> bool {
        self.execution.formatter_column == FormatterColumn::Advanced
            || self.execution.zero_advance.has_buffered_glyph()
            || self.execution.word_end_break == WordEndBreak::Pending
    }

    pub(in crate::mandoc) fn has_invisible_formatter_cell(&self) -> bool {
        self.execution.formatter_column == FormatterColumn::Advanced
            && !self
                .nodes
                .iter()
                .rev()
                .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
                .any(|node| has_printable_character(std::slice::from_ref(node)))
    }

    /// Keep the formatter's write order when this word armed BACKBEFORE.
    ///
    /// `term_word()` buffers the word boundary before decoding `\zX`.  The
    /// following word then overwrites its own boundary with X; postponing the
    /// first boundary until that point incorrectly moves it after X.  Commit
    /// only the already executed padding here and leave the glyph pending.
    fn materialize_boundary_before_pending_glyph(&mut self) {
        if !self.execution.zero_advance.has_pending_glyph() {
            return;
        }
        let breakable = std::mem::take(&mut self.execution.pending_breakable_spaces);
        let field = std::mem::take(&mut self.execution.pending_field_spaces);
        let count = breakable.saturating_add(field);
        if count == 0 {
            return;
        }
        self.append_projected(vec![Inline::Text {
            value: " ".repeat(count),
        }]);
        self.execution.trailing_output = if field > 0 {
            TrailingOutput::FieldBlank(field)
        } else {
            TrailingOutput::BreakableBlank(breakable)
        };
        self.execution.formatter_column = FormatterColumn::Advanced;
    }

    pub(in crate::mandoc) fn execute_empty_word(&mut self) {
        self.begin_word_projection(true);
        self.append_word(Vec::new());
    }

    /// Generated glyphs use the effective font just like authored text, but
    /// are not reparsed as roff source (names can contain literal escapes).
    pub(in crate::mandoc) fn append_text(&mut self, value: &str) {
        self.begin_word_projection(!value.is_empty());
        self.append_prepared_text(value);
    }

    /// Enter the next generated formatter word before closing a semantic
    /// output owner. A pending `\z` glyph may become visible at this word's
    /// implicit boundary and still belongs to the preceding authored owner.
    pub(in crate::mandoc) fn prepare_generated_word(&mut self) {
        self.begin_word_projection(true);
    }

    /// Complete a generated word whose native pre-boundary was entered by
    /// `prepare_generated_word()` before an IR wrapper was attached.
    pub(in crate::mandoc) fn append_prepared_text(&mut self, value: &str) {
        // term_word() wrote these cells even when bare BACKAFTER buffers its
        // sole glyph. The next physical row boundary must still flush it.
        self.note_produced_formatter_cell(!value.is_empty());
        let kept_zero_boundary = self.execution.boundary == PendingBoundary::Kept
            && value
                .chars()
                .next()
                .is_some_and(super::super::is_formatter_word_blank)
            && self.execution.zero_advance.has_pending_glyph();
        let mut projected = Vec::new();
        self.execution.zero_advance.append_generated_text(
            value,
            &mut projected,
            self.execution.font.display_current(),
        );
        if kept_zero_boundary {
            // TERMP_KEEP inserts a non-breaking formatter blank.  A pending
            // BACKBEFORE glyph consumes that cell, so retain the glyph while
            // suppressing both the virtual blank and ordinary boundary.
            self.execution.boundary = PendingBoundary::Tight;
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
            self.execution.final_word_join = Some(false);
        }
    }

    /// Execute validator-generated text as a complete formatter word.
    ///
    /// This differs from appending generated punctuation inside an existing
    /// word: `term_word()` consumes an authored `\c`, participates in deferred
    /// `\p` handling, and closes physical source continuation before the next
    /// source node. CVS uses this path for the generated `BSD` child of `.Bx`.
    pub(in crate::mandoc) fn append_generated_word(&mut self, value: &str) {
        self.note_produced_formatter_cell(!value.is_empty());
        self.begin_word_projection(!value.is_empty());
        let mut projected = Vec::new();
        self.execution.zero_advance.append_generated_text(
            value,
            &mut projected,
            self.execution.font.display_current(),
        );
        self.append_word(projected);
        if !value.is_empty() {
            self.execution.final_word_join = Some(false);
            self.execution.final_source_continuation = Some(false);
        }
    }

    /// Start an ordinary formatter word after a source text node. If a prior
    /// `\z` glyph is pending, model term.c's implicit blank before decoding
    /// the next glyph: preserve the pending glyph but suppress this reader's
    /// own inter-word space. Tight joins (for example alternating `.BR`
    /// operands) deliberately bypass this transition and overstrike instead.
    pub(in crate::mandoc) fn begin_word_projection(&mut self, next_is_visible: bool) {
        self.begin_word_projection_with_break(next_is_visible, next_is_visible);
    }

    /// An empty or control-only formatter word still updates registers, but
    /// it cannot by itself make `term_fill()` print after a pending \p.
    pub(in crate::mandoc) fn begin_word_projection_with_break(
        &mut self,
        next_is_visible: bool,
        next_has_glyph: bool,
    ) {
        if next_is_visible {
            // term_word() clears skipvsp before consuming the word itself.
            self.execution.vertical_space_debt = 0;
            self.execution.execution_epoch = self.execution.execution_epoch.wrapping_add(1);
        }
        let continued_word = next_is_visible
            && self.final_source_continuation_or(false)
            && !self.execution.boundary.is_tight();
        if next_is_visible {
            self.execution.final_word_join = Some(false);
            self.execution.final_source_continuation = Some(false);
        }
        if next_is_visible && self.execution.keep.keeping() && !self.execution.boundary.is_tight() {
            self.execution.boundary = PendingBoundary::Kept;
            if let Some(glyph) = self.execution.zero_advance.resolve_at_word_boundary() {
                // CVS writes TERMP_KEEP's implicit NBRSP before the next
                // glyph. It settles BACKBEFORE without becoming visible, so
                // retain the glyph and join the incoming formatter word.
                self.append_projected(vec![glyph]);
                self.execution.boundary = PendingBoundary::Tight;
            }
        }
        if next_is_visible
            && next_has_glyph
            && !self.pending_definition_break_has_no_graph()
            && !self.execution.boundary.is_nonbreaking()
            && self.execution.word_end_break == WordEndBreak::Pending
            && (self.execution.spacing.enabled()
                || matches!(
                    self.execution.boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued
                ))
        {
            if let Some(glyph) = self.execution.zero_advance.resolve_at_word_boundary() {
                // The formatter's automatic word blank settles BACKBEFORE
                // before a buffered `\\p` can become a line boundary.  Keep
                // the glyph and join the incoming word at that position.
                self.append_projected(vec![glyph]);
                self.execution.boundary = PendingBoundary::Tight;
            } else {
                self.hard_break();
            }
        }
        if continued_word && !self.execution.boundary.is_tight() {
            // TERMP_NONEWLINE suppresses the physical source break while an
            // `.Ec`-style release permits the normal formatter blank.  An
            // authored leading blank is retained separately by the incoming
            // word, producing the two spaces emitted by CVS in both filled
            // and literal flows.
            self.execution.boundary = PendingBoundary::Continued;
        }
        if next_is_visible && self.execution.keep.phase == KeepPhase::PreKeep {
            // term_word() inserts the leading boundary using the *previous*
            // KEEP value, then promotes PREKEEP. Execute that ordering before
            // decoding the word: a bare BACKAFTER survives the ordinary blank,
            // while BACKBEFORE consumes it and retains its buffered glyph.
            if !self.execution.boundary.is_nonbreaking()
                && (self.execution.formatter_column == FormatterColumn::Advanced
                    || self.execution.zero_advance.has_pending_glyph())
                && (self.execution.spacing.enabled()
                    || matches!(self.execution.boundary, PendingBoundary::Preserved))
            {
                if let Some(glyph) = self.execution.zero_advance.resolve_at_word_boundary() {
                    self.append_projected(vec![glyph]);
                } else {
                    self.append_projected(vec![Inline::Text {
                        value: " ".to_owned(),
                    }]);
                    self.execution.formatter_column = FormatterColumn::Advanced;
                }
                self.execution.boundary = PendingBoundary::Tight;
            }
            self.execution.keep.phase = KeepPhase::Keep;
        }
        if !next_is_visible
            || self.execution.boundary.is_nonbreaking()
            || !(self.execution.has_printable_content
                || self.execution.zero_advance.has_pending_glyph())
            || !(self.execution.spacing.enabled()
                || matches!(self.execution.boundary, PendingBoundary::Preserved))
        {
            return;
        }
        let Some(glyph) = self.execution.zero_advance.resolve_at_word_boundary() else {
            return;
        };
        self.append_projected(vec![glyph]);
        self.execution.boundary = PendingBoundary::Tight;
    }

    /// Only macros that select a font establish this scope. Transparent
    /// macros and text operands must not invent a push/pop of their own.
    pub(in crate::mandoc) fn with_font_scope(
        &mut self,
        font: Font,
        append: impl FnOnce(&mut Self),
    ) {
        let saved = self.execution.font.push_scope(font);
        append(self);
        self.execution.font.pop_scope(saved);
    }

    /// Wrap one IR suffix without moving the native field's commit boundary.
    /// A wrapper can replace many children by one Link/Emphasis node; when a
    /// real `term_flushln()` fell inside it, keep accepted and pending slices
    /// separate so a later nbr=0 only rejects the pending slice.
    fn wrap_output_from(&mut self, start: usize, mut wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>) {
        let end = self.nodes.len();
        let mut inner = self.nodes.split_off(start);
        let field_start = self
            .execution
            .author_execution
            .as_ref()
            .map(|author| author.field_output_start);
        if let Some(field_start) = field_start.filter(|&field_start| {
            self.execution.definition.is_some() && start < field_start && field_start < end
        }) {
            // Keep the committed prefix and the current native field in
            // distinct wrappers. A later term_fill() rejection must never
            // erase or retain part of the wrong wrapped output owner.
            let pending = inner.split_off(field_start - start);
            let accepted = wrap(inner);
            let pending = wrap(pending);
            let split_link = matches!((accepted.last(), pending.last()),
                (Some(Inline::Link { target: left, title: left_title, .. }),
                 Some(Inline::Link { target: right, title: right_title, .. }))
                    if left == right && left_title == right_title);
            self.nodes.extend(accepted);
            if split_link {
                // This is one authored link across two native fields. Keep
                // its parts separate until the pending field is accepted or
                // rejected, then join the presentation owner at IR drain.
                self.nodes.push(Inline::anchor(INTERNAL_LINK_SPLIT));
            }
            let new_field_start = self.nodes.len();
            self.nodes.extend(pending);
            self.execution
                .author_execution
                .as_mut()
                .unwrap()
                .field_output_start = new_field_start;
        } else {
            self.nodes.extend(wrap(inner));
            if let Some(author) = &mut self.execution.author_execution
                && author.field_output_start >= end
                && end > start
            {
                author.field_output_start = self.nodes.len();
            }
        }
    }

    /// Style newly appended content without creating a new formatter state.
    /// The transform must preserve visible characters and line boundaries.
    /// In particular, a wrapper ending after `\\c` must not consume its
    /// pending join merely because its content is represented as a subtree.
    pub(in crate::mandoc) fn append_scope(
        &mut self,
        append: impl FnOnce(&mut Self),
        style: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        // Appending must still see the prefix, especially a preceding hard
        // break used for deduplication. Style only the new suffix afterwards.
        let start = self.nodes.len();
        append(self);
        self.wrap_output_from(start, style);
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

    pub(in crate::mandoc) fn finish(mut self) -> Vec<Inline> {
        self.flush_zero_advance();
        self.finish_nodes()
    }

    /// A definition HEAD hands its completed source rows to the item layout.
    /// Only a paragraph terminator may discard a trailing `LineBreak`.
    pub(in crate::mandoc) fn finish_preserving_rows(mut self) -> Vec<Inline> {
        self.flush_zero_advance();
        self.drain_ir_nodes()
    }

    /// Drain one IR paragraph without ending the formatter's execution
    /// lifetime.  Only paragraph-local joins and row projections reset; the
    /// source formatter's keep, author, spacing, and vertical-space registers
    /// remain in this same execution state.
    pub(in crate::mandoc) fn take_paragraph_segment(
        &mut self,
        line_request: bool,
    ) -> (Vec<Inline>, bool, u16) {
        // finish_nodes() trims a trailing break because it normally ends an
        // IR paragraph. If another invisible cell is already active after
        // that break, the break closed a real earlier row (term_newln()) and
        // must remain between the two cells.
        let carry_armed_zero_advance = !self.has_formatter_cell();
        let armed = carry_armed_zero_advance && self.execution.zero_advance.take_armed();
        let empty_word_end_break = self.take_unrepresented_word_end_break();
        self.flush_zero_advance();
        let mut invisible_formatter_cell = self.has_invisible_formatter_cell();
        // .br and the final pre_br() phase of .sp close an occupied native
        // cell before this output owner is drained. A preceding visible row
        // can already end in LineBreak while the current empty-word cell has
        // no glyph of its own; it still needs its own completed row. A bare
        // pending \p is already transferred through empty_word_end_break,
        // so it cannot also claim a completed empty-word row.
        let requested_invisible_row = line_request
            && invisible_formatter_cell
            && !empty_word_end_break
            && !self.external_head_row_pending;
        let trailing_invisible_rows = if requested_invisible_row {
            let mut rows = 0u16;
            let mut row_has_glyph = false;
            for node in &self.nodes {
                if matches!(node, Inline::LineBreak { .. }) {
                    rows = if row_has_glyph {
                        0
                    } else {
                        rows.saturating_add(1)
                    };
                    row_has_glyph = false;
                } else {
                    row_has_glyph |= has_non_whitespace_glyph(std::slice::from_ref(node));
                }
            }
            rows
        } else {
            0
        };
        // Trailing LineBreak nodes own already closed rows; the current
        // invisible cell is a later row. A bare \p was excluded above and
        // is transferred separately through empty_word_end_break.
        let mut completed_vertical_rows = self
            .execution
            .completed_vertical_rows
            .saturating_add(u16::from(requested_invisible_row))
            .saturating_add(
                trailing_invisible_rows.saturating_sub(self.execution.completed_vertical_rows),
            );
        if completed_vertical_rows > 0 {
            // term_vspace() has already emitted these rows. Remove only their
            // trailing inline projection, including a later invisible word
            // cell; the block owner will carry the completed rows across the
            // structural split. Ordinary trailing term_newln() stays trimable.
            // term_fill() can end a row whose only buffered cells are
            // breakable blanks from a visited whitespace-only TEXT. Those
            // cells are printable to our inline model, yet still form an
            // additional physical row after term_vspace().
            let active_invisible_cell = self.execution.formatter_column
                == FormatterColumn::Advanced
                && !self
                    .nodes
                    .iter()
                    .rev()
                    .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
                    .any(|node| has_non_whitespace_glyph(std::slice::from_ref(node)));
            completed_vertical_rows = completed_vertical_rows
                .saturating_add(u16::from(active_invisible_cell && !requested_invisible_row));
            // The completed-row owner now accounts for this cell. The old
            // invisible-word fallback must not manufacture it a second time.
            invisible_formatter_cell &= !active_invisible_cell;
            let mut anchors = Vec::new();
            while self.nodes.last().is_some_and(|node| {
                matches!(node, Inline::LineBreak { .. })
                    || !has_non_whitespace_glyph(std::slice::from_ref(node))
            }) {
                if let Some(anchor @ Inline::Anchor { .. }) = self.nodes.pop() {
                    anchors.push(anchor);
                }
            }
            anchors.reverse();
            self.nodes.extend(anchors);
        }
        let completed_invisible_row =
            invisible_formatter_cell && matches!(self.nodes.last(), Some(Inline::LineBreak { .. }));
        // The item post term_newln() (mdoc_term.c:939-945) flushes a
        // run-in NOBREAK field carried across the HEAD/BODY split before
        // this owner drains; term_fill() has already decided that field's
        // rejection, so commit the wipe while the field still owns its
        // output range. Fresh `.mc` NoBreakField sessions own their own
        // earlier lifecycle and are not decided here.
        if self
            .execution
            .definition
            .as_ref()
            .is_some_and(|definition| definition.run_in_continuation)
        {
            self.discard_unprinted_definition_field_output();
        }
        let mut children = self.finish_nodes();
        if completed_invisible_row {
            children.push(Inline::line_break());
        }
        if invisible_formatter_cell && !empty_word_end_break && !has_printable_character(&children)
        {
            children.push(Inline::Text {
                value: String::new(),
            });
        }
        self.execution.reset_paragraph_segment(armed);
        (children, empty_word_end_break, completed_vertical_rows)
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
    pub(in crate::mandoc) fn finish_formatter_line(
        mut self,
        preserve_rows: bool,
    ) -> (Vec<Inline>, super::InlineExecutionState) {
        let surviving_armed = if self.has_formatter_cell() {
            false
        } else {
            self.execution.zero_advance.take_armed()
        };
        self.flush_zero_advance();
        let output = if preserve_rows {
            self.execution.word_end_break = WordEndBreak::Clear;
            self.drain_ir_nodes()
        } else {
            self.finish_nodes()
        };
        self.execution.reset_paragraph_segment(surviving_armed);
        (output, self.execution)
    }

    /// Return an inner scope without forcing a pending `\\z` glyph to become
    /// visible.  CVS mandoc carries its backtracking flags through nested
    /// `term_word()` calls, so the caller must continue the state in the
    /// surrounding inline stream before committing it at a real boundary.
    pub(in crate::mandoc) fn finish_preserving_execution(
        mut self,
    ) -> (
        Vec<Inline>,
        PreservedInlineState,
        super::InlineExecutionState,
    ) {
        let formatter_cell_occupied = self.has_formatter_cell();
        let definition_suffix_discarded = self.execution.definition_suffix_discarded();
        // term.c keeps one NOBREAK field active across the whole list item
        // (mdoc_term.c::termp_it_pre() through the item's BODY post). When
        // the HEAD session ends while that field is still configured, carry
        // it instead of retiring it with the drained output owner.
        let definition_field = match (
            self.execution
                .author_execution
                .as_ref()
                .map(|author| author.break_effect),
            self.execution.definition.take(),
        ) {
            (
                Some(AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                }),
                Some(state),
            ) => Some(super::definition::PreservedDefinitionField {
                state,
                gap_cells,
                body_width_columns,
                field_width_columns,
                flags,
            }),
            // Every other field dies with the drained owner below, exactly
            // as retire_output_owner() has always done.
            _ => None,
        };
        let state = PreservedInlineState {
            zero_advance: std::mem::take(&mut self.execution.zero_advance),
            word_end_break: self.execution.word_end_break == WordEndBreak::Pending,
            definition_suffix_discarded,
            definition_field,
            source_continuation: self.execution.final_source_continuation,
            formatter_cell_occupied,
            pending_line_indent: self.execution.pending_line_indent,
            pending_definition_indent: self.pending_definition_indent(),
            last_executed_source_line: self.execution.last_executed_source_line,
        };
        // A scope return is not term_newln(). In particular, an authored
        // br/sp at the end of an inset HEAD closed its last physical row;
        // keep that break in the detached term so the BODY cannot run in.
        self.execution.word_end_break = WordEndBreak::Clear;
        let output = self.drain_ir_nodes();
        // The output owner ended, but CVS term_word() still sees the same
        // physical row and word separator after an inset/diag HEAD. Keep its
        // registers, retiring only offsets into the drained node vector.
        self.execution.retire_output_owner();
        (output, state, self.execution)
    }

    fn finish_nodes(&mut self) -> Vec<Inline> {
        self.execution.word_end_break = WordEndBreak::Clear;
        while matches!(self.nodes.last(), Some(Inline::LineBreak { .. })) {
            self.nodes.pop();
        }
        self.drain_ir_nodes()
    }

    fn drain_ir_nodes(&mut self) -> Vec<Inline> {
        let mut nodes = std::mem::take(&mut self.nodes);
        if std::env::var_os("MANT_DBG_D1").is_some() {
            eprintln!(
                "drain nodes={:?}",
                nodes
                    .iter()
                    .map(|n| match n {
                        Inline::Text { value } => format!("T({value:?})"),
                        Inline::LineBreak { .. } => "LB".to_owned(),
                        other => format!("{other:?}"),
                    })
                    .collect::<Vec<_>>()
            );
        }
        join_authored_links(&mut nodes);
        nodes
    }

    pub(super) fn flush_zero_advance(&mut self) {
        let mut pending = Vec::new();
        self.execution.zero_advance.finish_into(&mut pending);
        if !pending.is_empty()
            && (self.execution.pending_breakable_spaces > 0
                || self.execution.pending_field_spaces > 0)
        {
            let spaces = std::mem::take(&mut self.execution.pending_breakable_spaces)
                .saturating_add(std::mem::take(&mut self.execution.pending_field_spaces));
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
        let has_glyph = has_non_whitespace_glyph(&incoming);
        self.nodes.append(&mut incoming);
        if last.is_some() {
            self.execution.last_visible_character = last;
            self.execution.trailing_output = if last.is_some_and(char::is_whitespace) {
                TrailingOutput::BoundaryBlank
            } else {
                TrailingOutput::NonBlank
            };
        }
        self.execution.has_printable_content |= printable;
        if has_glyph {
            self.execution.visible_glyph_epoch = self.execution.visible_glyph_epoch.wrapping_add(1);
            self.execution.completed_vertical_rows = 0;
        }
    }

    fn append_retained_layout(&mut self, retained: Vec<Inline>) {
        if retained.is_empty() {
            return;
        }
        if last_visible_character(&retained) == Some('\n') {
            self.execution.last_visible_character = Some('\n');
            self.execution.trailing_output = TrailingOutput::None;
        }
        self.nodes.extend(retained);
    }

    pub(super) fn retain_line_breaks(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.nodes
            .extend(std::iter::repeat_n(Inline::line_break(), count));
        self.execution.last_visible_character = Some('\n');
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.empty_word = false;
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
    }
}
fn has_non_whitespace_glyph(nodes: &[Inline]) -> bool {
    let mut found = false;
    mant_ir::visit_inline_plain_text(nodes, |text| {
        found |= text.chars().any(|character| !character.is_whitespace());
    });
    found
}

/// Count formatter-breakable ASCII blanks at the end of the current field.
/// Generated fixed cells reset this accounting at their call sites, while
/// non-breaking spaces remain distinguishable by their Unicode value.
pub(in crate::mandoc::inline) fn trailing_ascii_spaces(nodes: &[Inline]) -> usize {
    fn visit(nodes: &[Inline], count: &mut usize) -> bool {
        for node in nodes.iter().rev() {
            match node {
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => return false,
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
fn join_authored_links(nodes: &mut Vec<Inline>) {
    for node in nodes.iter_mut() {
        match node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => join_authored_links(children),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak { .. } => {}
        }
    }

    let mut index = 0;
    while index < nodes.len() {
        let marker = matches!(&nodes[index], Inline::Anchor { id, .. } if id.as_str() == INTERNAL_LINK_SPLIT);
        if !marker {
            index += 1;
            continue;
        }
        let next = index + 1;
        let prefix = matches!(nodes.get(next), Some(Inline::Text { value }) if value.chars().all(char::is_whitespace));
        let right = next + usize::from(prefix);
        let merge = match (
            index.checked_sub(1).and_then(|left| nodes.get(left)),
            nodes.get(right),
        ) {
            (
                Some(Inline::Link {
                    target: left,
                    title: left_title,
                    ..
                }),
                Some(Inline::Link {
                    target: right,
                    title: right_title,
                    ..
                }),
            ) => left == right && left_title == right_title,
            _ => false,
        };
        if merge {
            let Inline::Link { mut children, .. } = nodes.remove(right) else {
                unreachable!("checked matching link")
            };
            if prefix {
                children.insert(0, nodes.remove(next));
            }
            if let Inline::Link {
                children: accepted, ..
            } = &mut nodes[index - 1]
            {
                accepted.append(&mut children);
            }
        }
        nodes.remove(index);
    }
}

pub(in crate::mandoc) fn trim_trailing_breakable_spaces(nodes: &mut Vec<Inline>, count: usize) {
    fn trim(nodes: &mut Vec<Inline>, remaining: &mut usize) -> bool {
        let mut index = nodes.len();
        while index > 0 && *remaining > 0 {
            index -= 1;
            let remove = match &mut nodes[index] {
                Inline::Anchor { .. } => continue,
                Inline::LineBreak { .. } => return false,
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
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    if !trim(children, remaining) {
                        return false;
                    }
                    children.is_empty()
                }
                Inline::Link { children, .. } => {
                    // A discarded native field can empty the label, but the
                    // authored destination is still a typed IR fact (CVS
                    // mdoc_html.c keeps its href). Trim label blanks only.
                    if !trim(children, remaining) {
                        return false;
                    }
                    false
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
            Inline::LineBreak { .. } => 1,
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
            Inline::LineBreak { .. } => retained.push(Inline::line_break()),
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
            Inline::LineBreak { .. } => {
                retained.push(Inline::line_break());
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
        builder.append_retained_layout(vec![Inline::line_break(), Inline::anchor("mark")]);

        assert_eq!(builder.last_visible_character, Some('\n'));
        assert_eq!(builder.trailing_output, TrailingOutput::None);
        assert!(matches!(
            builder.nodes.as_slice(),
            [Inline::Text { value }, Inline::LineBreak { .. }, Inline::Anchor { .. }] if value == "prefix"
        ));
    }
}
