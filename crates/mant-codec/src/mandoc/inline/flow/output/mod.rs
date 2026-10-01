use super::{
    FilledBoundary, Font, FormatterColumn, Inline, InlineBuilder, OutputCheckpoint,
    PendingBoundary, TrailingOutput, WordEndBreak, has_printable_character,
};

// A private boundary carried only while one authored Link spans two native
// term_flushln() fields. It is removed before any IR owner is returned.
const INTERNAL_LINK_SPLIT: &str = "\0mant:field-link-split";
pub(in crate::mandoc::inline) const INTERNAL_FIELD_WORD: &str = "\0mant:field-word:";
pub(in crate::mandoc::inline) const INTERNAL_OUTPUT_SCOPE: &str = "\0mant:output-scope:";

mod boundary;
mod drain;
mod projection;
mod words;

pub(in crate::mandoc::inline::flow) use projection::ends_with_executed_line_break;
pub(in crate::mandoc) use projection::finalize_inline_output;
pub(in crate::mandoc::inline) use projection::trailing_ascii_spaces;
pub(in crate::mandoc) use projection::trim_trailing_breakable_spaces;
use projection::{has_non_whitespace_glyph, line_break_count};
pub(in crate::mandoc::inline::flow) mod native_passes;
mod record;
pub(in crate::mandoc::inline::flow) mod split;

impl InlineBuilder {
    pub(in crate::mandoc) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Record an output slice before its source executes. The checkpoint
    /// owns no formatter registers and cannot roll execution back.
    pub(in crate::mandoc) fn begin_output_checkpoint(&self) -> OutputCheckpoint {
        OutputCheckpoint {
            node_count: self.nodes.len(),
        }
    }

    /// Whether the accepted output slice contains a readable semantic glyph.
    /// Syntactic label eligibility and pending-glyph consumption remain with
    /// the macro's execution stages, independent of this output observation.
    pub(in crate::mandoc) fn output_since_has_non_whitespace_glyph(
        &self,
        checkpoint: OutputCheckpoint,
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
                | Inline::PortableDisplay { children, .. }
                | Inline::Link { children, .. } => contains_glyph(children),
                Inline::Anchor { .. } | Inline::LineBreak { .. } => false,
            })
        }
        contains_glyph(&self.nodes[checkpoint.node_count..])
    }

    /// Wrap the output emitted since `checkpoint` without replaying its
    /// formatter execution. Links and other semantic wrappers are IR
    /// annotations over an already-executed source stream.
    pub(in crate::mandoc) fn wrap_output_since(
        &mut self,
        checkpoint: OutputCheckpoint,
        wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        self.wrap_output_from(checkpoint.node_count, wrap);
    }

    /// Mark a local annotation scope without writing a formatter cell. A
    /// native flush may change its output length; the marker survives that
    /// receipt, so its owner never relies on a pre-flush vector index.
    pub(in crate::mandoc::inline) fn begin_output_scope(&mut self, marker: &str) {
        self.nodes.push(Inline::anchor(marker));
    }

    /// Find only this scope's new tail, then reuse the native committed/pending
    /// split. Missing metadata leaves the accepted text unannotated.
    pub(in crate::mandoc::inline) fn wrap_output_scope(
        &mut self,
        marker: &str,
        wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        if let Some(start) = self
            .nodes
            .iter()
            .rposition(|node| matches!(node, Inline::Anchor { id, .. } if id.as_str() == marker))
        {
            self.wrap_output_from(start, wrap);
        }
    }

    /// Preserve a formatter-requested line boundary without creating empty
    /// leading, repeated, or trailing rows around the paragraph.
    #[allow(clippy::too_many_lines)] // term_newln(): one native row boundary, one execution order
    pub(in crate::mandoc) fn hard_break(&mut self) {
        let had_native_buffer = self.definition.as_ref().is_some_and(|state| {
            state.field_buffer.resume_offset() < state.field_buffer.cells().len()
        }) || (self.execution.definition.is_none()
            && self.execution.flush_unit.resume_offset() < self.execution.flush_unit.cells().len());
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
        // term_newln() flushes the plain flush unit through the same
        // term_flushln(): a definitive rejection ends its own row here.
        let plain_flush_rejection = if self.in_definition_field() {
            // term_newln() flushes the same tcol->buf after a TAG request
            // cleared NOBREAK. A definition session still owns that input;
            // consume its receipt before the row reset retires the cells.
            self.discard_unprinted_definition_field_output()
        } else {
            self.retire_plain_flush_unit()
        };
        if !self.execution.has_printable_content {
            self.execution.leading_line_boundary = super::LeadingLineBoundary::BeforeVisibleWord;
        }
        // term_newln() flushes only an occupied terminal cell. A completed
        // `\zX` glyph and a buffered `\p` both advanced the native buffer;
        // a bare armed `\z` did not and remains ordered before the next word.
        // A definitively rejected flush unit still occupied the native
        // buffer: term_newln() flushed it (lastcol > 0), term_flushln()
        // reset the buffer (term.c:233-237), and the nbr == 0 pass ended
        // its own row (term.c:250-253). The rejection must retire here.
        let retiring_rejection = self.execution.wipe_remainder || plain_flush_rejection;
        if !self.has_formatter_cell()
            && !had_native_buffer
            && !exited_definition_row
            && !exited_discarded_buffer
            && !retiring_rejection
        {
            if self.external_head_row_pending {
                // term_newln() still flushes the detached native tag row.
                // term_flushln() clears a bare \\z before the next BODY word.
                self.execution.zero_advance.discard_at_row_end();
                self.external_head_row_pending = false;
            }
            self.reset_native_tab_origin();
            return;
        }
        // term.c:143-146 with 233-237: a definitively rejected unit dies
        // whole - including a `\z`-generated glyph still pending in the
        // zero-advance register. A bare armed `\z` wrote no cell and keeps
        // its request alive across the boundary (term.c:836-838), so it
        // still settles through the ordinary flush.
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
        if (had_native_buffer
            || self.execution.formatter_column == FormatterColumn::Advanced
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
        self.execution.wipe_remainder = false;
        self.execution.row_zero_graph = false;
        self.execution.formatter_column = FormatterColumn::Origin;
        if (exited_discarded_buffer && !exited_definition_row)
            || !matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))
            || retiring_rejection
        {
            let row_indent = self.take_definition_row_indent();
            self.nodes.push(Inline::line_break_indented(row_indent));
            self.note_definition_output_row();
            self.execution.last_visible_character = Some('\n');
        }
        self.execution.final_word_join = Some(false);
        if let Some(definition) = &mut self.execution.definition {
            // A committed hard break closes the native device row even when
            // the HEAD is still in its TAG field. Otherwise an overrun tag
            // leaves stale viscol for a later discarded buffer.
            definition.hang_row.endline();
            definition.field_buffer.clear();
            definition.field_buffer.set_tab_offset(0);
            definition.field_word_anchors.clear();
        }
        if self.execution.definition.is_none() {
            // term_flushln() resets the plain unit with the same row
            // (term.c:233-237): the next word starts a fresh buffer whose
            // output interval begins after this committed row.
            self.execution.flush_unit.clear();
            self.execution.flush_unit.set_tab_offset(0);
            self.execution.flush_unit_anchors.clear();
            self.execution.flush_unit_output_start = self.nodes.len();
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
                    if left == right && left_title == right_title)
                || matches!((accepted.last(), pending.last()), (Some(left), Some(right))
                    if super::super::links::presentation::same_link_owner(left, right));
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

    pub(super) fn retain_line_breaks(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.nodes
            .extend(std::iter::repeat_n(Inline::line_break(), count));
        self.note_definition_output_row();
        self.execution.last_visible_character = Some('\n');
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.empty_word = false;
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
    }
}
