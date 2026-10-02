//! Generated words and pending glyphs execute in the live formatter.

use super::super::field_buffer::FieldWrite;
use super::super::{
    FormatterColumn, Inline, InlineBuilder, KeepPhase, PendingBoundary, TrailingOutput,
    WordEndBreak, has_printable_character, last_visible_character,
};
use super::projection::has_rendered_formatter_glyph;
use crate::mandoc::inline::is_formatter_word_blank;

/// Ownership receipt for one semantic wrapper macro (`Lk`, `Mt`, `Sx`,
/// `In`, `Bx`, `Xr`, `MR`).
#[derive(Clone)]
pub(in crate::mandoc) struct SemanticOwnerCheckpoint {
    nodes_start: usize,
    pending: usize,
    original: Option<Inline>,
}

impl InlineBuilder {
    /// Mint projection identity before decoding, so a cached BACKBEFORE
    /// glyph remains attached to its source word after a later word emits it.
    pub(in crate::mandoc) fn begin_native_word_owner(&mut self) {
        self.execution.native_owner_serial = self.execution.native_owner_serial.wrapping_add(1);
        let marker = format!(
            "{}{}",
            super::INTERNAL_FIELD_WORD,
            self.execution.native_owner_serial
        );
        self.execution
            .zero_advance
            .set_native_word_owner(self.execution.native_owner_serial);
        self.execution.native_word_owner = Some(marker);
        // `termp_fl_pre()` emits a real '-' word before its children. Its
        // scoped role must be registered here, before either generated or
        // authored text can be retired from the accepted output buffer.
        self.record_current_native_operand(false);
    }

    /// Keep the formatter's write order when this word armed BACKBEFORE.
    ///
    /// `term_word()` buffers the word boundary before decoding `\zX`.  The
    /// following word then overwrites its own boundary with X; postponing the
    /// first boundary until that point incorrectly moves it after X.  Commit
    /// only the already executed padding here and leave the glyph pending.
    pub(super) fn materialize_boundary_before_pending_glyph(&mut self) {
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
        self.begin_native_word_owner();
        self.execution.native_word_writes = Some(Vec::new());
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
        self.begin_native_word_owner();
        self.execution
            .native_word_boundary
            .get_or_insert(self.execution.boundary);
        self.execution.native_word_writes = Some(FieldWrite::literal(value));
        // term_word() wrote these cells even when bare BACKAFTER buffers its
        // sole glyph. The next physical row boundary must still flush it.
        self.note_produced_formatter_cell(!value.is_empty());
        let kept_zero_boundary = self.execution.boundary == PendingBoundary::Kept
            && value.chars().next().is_some_and(is_formatter_word_blank)
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
        self.execution.native_word_writes = Some(FieldWrite::literal(value));
        self.note_produced_formatter_cell(!value.is_empty());
        self.begin_word_projection(!value.is_empty());
        self.begin_native_word_owner();
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

    /// Projection-only ownership checkpoint at a semantic wrapper's entry.
    ///
    /// Native wrapper entry executes no `term_word()`: `termp_lk_pre()`
    /// only font-pushes before its first real operand (`mdoc_term.c:1881`),
    /// so no word register, boundary decision, or pending-break settlement
    /// may run here — those belong to the wrapper's first real operand or
    /// generated word, which enters through its own word methods above.
    /// The checkpoint's single duty is ownership: a `\z` glyph delayed
    /// from preceding source stays pending until that real word settles
    /// it, and must then keep its own owner and style instead of being
    /// captured by the wrapper's Link or Code
    /// annotation. The zero-advance owner ledger carries that receipt
    /// across the whole handler execution.
    pub(in crate::mandoc) fn begin_semantic_owner_checkpoint(&mut self) -> SemanticOwnerCheckpoint {
        let pending = self.execution.zero_advance.pending_visible_characters();
        let original = self.execution.zero_advance.pending_projection();
        self.execution.zero_advance.begin_output_owner();
        SemanticOwnerCheckpoint {
            nodes_start: self.nodes.len(),
            pending,
            original,
        }
    }

    /// Close a semantic wrapper checkpoint after its handler ran: glyphs
    /// that were already pending at entry and were emitted while the
    /// wrapper owned the output stream are split back out of the wrapper's
    /// annotation, keeping their own source, style, and link identity.
    pub(in crate::mandoc) fn finish_semantic_owner_checkpoint(
        &mut self,
        checkpoint: SemanticOwnerCheckpoint,
    ) {
        let mut owned = if self.execution.zero_advance.end_output_owner() {
            checkpoint.pending
        } else {
            0
        };
        let start = checkpoint.nodes_start.min(self.nodes.len());
        if owned > 0
            && start < self.nodes.len()
            && let Some(original) = checkpoint.original
        {
            let projected = self.nodes.split_off(start);
            let (owned, rest) =
                super::split::split_owned_glyph_prefix(projected, &mut owned, &original);
            self.nodes.extend(owned);
            self.nodes.extend(rest);
        }
    }

    /// Split delayed glyphs out of one wrapper's collected output just
    /// before that wrapper compacts the rest into a single annotation
    /// node (`.In`'s code span, which flattens nested identity).
    /// `entry_pending` is the glyph count the zero-advance ledger held
    /// when the wrapper's output began; `emitted` is the ledger's receipt
    /// of whether those glyphs settled while it ran.
    pub(in crate::mandoc) fn split_delayed_glyph_prefix(
        nodes: Vec<Inline>,
        entry_pending: usize,
        emitted: bool,
        original: Option<&Inline>,
    ) -> (Vec<Inline>, Vec<Inline>) {
        let mut remaining = usize::from(emitted) * entry_pending;
        let Some(original) = original.filter(|_| remaining > 0) else {
            return (Vec::new(), nodes);
        };
        super::split::split_owned_glyph_prefix(nodes, &mut remaining, original)
    }

    /// Start an ordinary formatter word after a source text node. If a prior
    /// `\z` glyph is pending, model term.c's implicit blank before decoding
    /// the next glyph: preserve the pending glyph but suppress this reader's
    /// own inter-word space. Tight joins (for example alternating `.BR`
    /// operands) deliberately bypass this transition and overstrike instead.
    /// Under a definitive rejection (term.c:143-146 with 233-237) a glyph
    /// still buffered in the zero-advance register is unprinted input: it
    /// must not settle at a word boundary. It dies at the next retirement.
    fn resolve_zero_advance_at_word_boundary(&mut self) -> Option<Vec<Inline>> {
        if self.execution.wipe_remainder {
            return None;
        }
        self.execution.zero_advance.resolve_at_word_boundary()
    }

    pub(in crate::mandoc) fn begin_word_projection(&mut self, next_is_visible: bool) {
        self.begin_word_projection_with_break(next_is_visible, next_is_visible, false);
    }

    /// An empty or control-only formatter word still updates registers, but
    pub(in crate::mandoc) fn begin_word_projection_with_break(
        &mut self,
        next_is_visible: bool,
        next_has_glyph: bool,
        marker_blank_before_graph: bool,
    ) {
        let continued_word = self.begin_native_word_registers(next_is_visible);
        self.settle_pending_word_break(next_is_visible, next_has_glyph);
        if continued_word && !self.execution.boundary.is_tight() {
            // TERMP_NONEWLINE suppresses the physical source break while an
            // `.Ec`-style release permits the normal formatter blank.  An
            // authored leading blank is retained separately by the incoming
            // word, producing the two spaces emitted by CVS in both filled
            // and literal flows.
            self.execution.boundary = PendingBoundary::Continued;
        }
        self.promote_word_keep(next_is_visible);
        if marker_blank_before_graph
            && self.execution.zero_advance.has_buffered_glyph()
            && !self.execution.boundary.is_nonbreaking()
            && (self.execution.spacing.enabled()
                || matches!(
                    self.execution.boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued
                ))
        {
            // The incoming word starts with a `\p` marker whose following
            // blank the pending glyph's BACKBEFORE retreat consumes
            // (term.c:901-908): resolving the glyph at this virtual boundary
            // would instead eat the separator and arm the marker's break on
            // the wrong cell. Keep the glyph pending; the word's own blank
            // settles it. The surviving separator is the blank term_word()
            // actually wrote before the marker, so it exists only where the
            // entry state allowed one: spacing enabled (`.Sm on`) or the
            // first fragment after the transition (`Preserved`/`Continued`),
            // never across a tight join — TERMP_NOSPACE wrote nothing
            // (term.c:573-580).
            // A completed BACKBEFORE glyph still occupies its cell when a
            // second \\z has armed BACKAFTER. Both flags coexist upstream:
            // the marker's author blank, rather than this automatic blank,
            // is what the next encode1() retreats over (term.c:901-927).
            self.execution.zero_advance.note_marker_blank_separator();
            return;
        }
        if !next_is_visible
            || self.execution.boundary.is_nonbreaking()
            || !(self.execution.has_printable_content
                || self.execution.zero_advance.has_buffered_glyph())
            || !(self.execution.spacing.enabled()
                || matches!(self.execution.boundary, PendingBoundary::Preserved))
        {
            return;
        }
        let Some(glyph) = self.resolve_zero_advance_at_word_boundary() else {
            return;
        };
        self.append_projected(glyph);
        self.execution.boundary = PendingBoundary::Tight;
    }

    fn promote_word_keep(&mut self, next_is_visible: bool) {
        if next_is_visible && self.execution.keep.phase == KeepPhase::PreKeep {
            // term_word() inserts the leading boundary using the *previous*
            // KEEP value, then promotes PREKEEP. Execute that ordering before
            // decoding the word: a bare BACKAFTER survives the ordinary blank,
            // while BACKBEFORE consumes it and retains its buffered glyph.
            if !self.execution.boundary.is_nonbreaking()
                && (self.execution.formatter_column == FormatterColumn::Advanced
                    || self.execution.zero_advance.has_buffered_glyph())
                && (self.execution.spacing.enabled()
                    || matches!(self.execution.boundary, PendingBoundary::Preserved))
            {
                if let Some(glyph) = self.resolve_zero_advance_at_word_boundary() {
                    self.append_projected(glyph);
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
    }

    fn settle_pending_word_break(&mut self, next_is_visible: bool, next_has_glyph: bool) {
        if next_is_visible
            && self.execution.keep.keeping()
            && !self.execution.boundary.is_tight()
            && (self.execution.spacing.enabled()
                || matches!(
                    self.execution.boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued
                ))
        {
            self.execution.boundary = PendingBoundary::Kept;
            if let Some(glyph) = self.resolve_zero_advance_at_word_boundary() {
                // CVS writes TERMP_KEEP's implicit NBRSP before the next
                // glyph. It settles BACKBEFORE without becoming visible, so
                // retain the glyph and join the incoming formatter word.
                self.append_projected(glyph);
                self.execution.boundary = PendingBoundary::Tight;
            }
        }
        if next_is_visible
            && next_has_glyph
            && !self.execution.boundary.is_nonbreaking()
            && self.execution.word_end_break == WordEndBreak::Pending
            && (self.execution.spacing.enabled()
                || matches!(
                    self.execution.boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued
                ))
            && !self.in_definition_field()
            && self.execution.flush_unit.has_pending_break_markers()
            && !self
                .execution
                .flush_unit
                .has_non_ignorable_after(self.execution.flush_unit.resume_offset(), false)
            && self.current_row_has_graph()
        {
            // A `.mc` no-break flush resets the native buffer (term.c:233-
            // 237), so the marker's unit holds no printable cell and its
            // pass can never accept a row. CVS loses the following word at
            // this edge; ManT keeps it on the row this blank ends
            // (term.c:294-295) as the content-preserving deviation.
            self.hard_break();
        }
        if next_is_visible
            && next_has_glyph
            && !self.pending_flush_break_has_no_graph()
            && !self.execution.boundary.is_nonbreaking()
            && self.execution.word_end_break == WordEndBreak::Pending
            && (self.execution.spacing.enabled()
                || matches!(
                    self.execution.boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued
                ))
        {
            if let Some(glyph) = self.resolve_zero_advance_at_word_boundary() {
                // The formatter's automatic word blank settles BACKBEFORE
                // before a buffered `\\p` can become a line boundary.  Keep
                // the glyph and join the incoming word at that position.
                self.append_projected(glyph);
                self.execution.boundary = PendingBoundary::Tight;
            } else if !self.in_definition_field()
                && (!self.execution.has_printable_content
                    || (self.execution.flush_unit.has_pending_break_markers()
                        && !self.execution.flush_unit.has_non_ignorable_after(
                            self.execution.flush_unit.resume_offset(),
                            false,
                        )))
                && self.current_row_has_graph()
            {
                // The marker sits in a retired owner (a drained TAG field
                // or scope), or its unit holds no printable cell at all
                // (a `.mc` no-break flush reset the buffer): the shared
                // pass loop can never accept a row there, but the blank
                // still ends the occupied row (term.c:294-295) and ManT
                // keeps the following word's content.
                self.hard_break();
            }
        }
    }

    fn begin_native_word_registers(&mut self, next_is_visible: bool) -> bool {
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
        // term_word() writes its automatic separator using the incoming
        // NOSPACE/KEEP registers, then promotes PREKEEP (term.c:573-586).
        // Zero-advance projection may subsequently tighten the IR boundary;
        // that cannot change which native cell was already written.
        if next_is_visible {
            self.execution.native_word_boundary = Some(
                if self.execution.keep.phase == KeepPhase::Keep
                    && !self.execution.boundary.is_tight()
                    && (self.execution.spacing.enabled()
                        || matches!(
                            self.execution.boundary,
                            PendingBoundary::Preserved | PendingBoundary::Continued
                        ))
                {
                    PendingBoundary::Kept
                } else {
                    self.execution.boundary
                },
            );
        }
        continued_word
    }

    pub(in crate::mandoc::inline::flow) fn flush_zero_advance(&mut self) {
        // term.c:143-146 with 233-237: a definitively rejected unit dies
        // whole - including a `\z`-generated glyph still pending in the
        // zero-advance register, at every retirement site (hard break,
        // paragraph drain, formatter-line finish). Cached glyph and newly
        // armed BACKAFTER can coexist; rejection retires both. A bare \z
        // in an independently empty, non-rejected buffer remains untouched.
        if self.execution.wipe_remainder {
            self.execution.zero_advance.discard_at_row_end();
            return;
        }
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

    pub(in crate::mandoc::inline::flow) fn append_projected(&mut self, mut incoming: Vec<Inline>) {
        if incoming.is_empty() {
            return;
        }
        let last = last_visible_character(&incoming);
        let printable = has_printable_character(&incoming);
        let has_glyph = has_rendered_formatter_glyph(&incoming);
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
}
