use super::{
    FilledBoundary, Font, FormatterColumn, Inline, InlineBuilder, OutputCheckpoint,
    PendingBoundary, TrailingOutput, WordEndBreak, has_printable_character,
};

// A private boundary carried only while one authored Link spans two native
// term_flushln() fields. It is removed before any IR owner is returned.
pub(in crate::mandoc::inline) const INTERNAL_LINK_SPLIT: &str = "\0mant:field-link-split";
const INTERNAL_COMPLETED_ROW: &str = "\0mant:output-scope:completed-row";
const INTERNAL_LITERAL_ROW: &str = "\0mant:output-scope:literal-row";
const INTERNAL_ROW_ORIGIN: &str = "\0mant:output-scope:row-origin:";
pub(in crate::mandoc::inline::flow) const INTERNAL_FIELD_PREFIX: &str =
    "\0mant:output-scope:field-prefix:";

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::mandoc) enum CompletedRowOrigin {
    Layout,
    LiteralText,
}
pub(super) const INTERNAL_TERM_ALTERNATIVE: &str = "\0mant:output-scope:term-alternative";
pub(super) fn is_term_alternative(node: &Inline) -> bool {
    matches!(node, Inline::Anchor { id, .. } if id.as_str() == INTERNAL_TERM_ALTERNATIVE)
}

const INTERNAL_DEVICE_ROW_END: &str = "\0mant:output-scope:device-row-end";
pub(in crate::mandoc::inline) const INTERNAL_FIELD_WORD: &str = "\0mant:field-word:";
pub(in crate::mandoc::inline) const INTERNAL_OUTPUT_SCOPE: &str = "\0mant:output-scope:";

/// The public drain and local row-index consumers share this classification.
/// Metadata can sit after a real row end without becoming that row's payload.
pub(in crate::mandoc::inline::flow) fn is_private_output_marker(node: &Inline) -> bool {
    row_origins::is_layout_carrier(node)
        || matches!(node, Inline::Anchor { id, .. }
        if id.as_str().starts_with(INTERNAL_FIELD_WORD)
            || id.as_str().starts_with(INTERNAL_OUTPUT_SCOPE)
            || id.as_str() == INTERNAL_LINK_SPLIT)
}

mod boundary;
mod drain;
mod projection;
mod words;

pub(in crate::mandoc) use projection::consume_one_row_ending;
pub(in crate::mandoc) use projection::ends_with_executed_line_break;
pub(in crate::mandoc) use projection::has_rendered_formatter_glyph;
use projection::line_break_count;
pub(in crate::mandoc) use projection::retain_inline_identities;
pub(in crate::mandoc::inline) use projection::trailing_ascii_spaces;
pub(in crate::mandoc) use projection::trailing_completed_row_origins;
pub(in crate::mandoc) use projection::trim_trailing_breakable_spaces;
pub(in crate::mandoc) use projection::{
    native_row_origin, prepare_inline_output, strip_native_projection_markers,
};
pub(in crate::mandoc::inline::flow) mod native_passes;
mod record;
pub(in crate::mandoc::inline::flow) mod row_origins;
pub(in crate::mandoc) use row_origins::{
    push_row_break, split_row_origin, take_definition_term_breaks, take_inline_layout,
};
pub(in crate::mandoc::inline::flow) mod split;

impl InlineBuilder {
    /// Row-layout carriers can be inserted into an already accepted prefix.
    /// Keep every live root-vector address on its original content boundary.
    /// `NativeWordAnchor` and field owner ranges are `FieldBuffer` cell/scalar
    /// positions; stable word/annotation markers and committed IR need no map.
    pub(in crate::mandoc::inline::flow) fn remap_output_positions(
        &mut self,
        edit: row_origins::OutputNodeEdit,
    ) {
        if let Some(registry) = &self.output_positions {
            for position in registry.borrow().iter().filter_map(std::rc::Weak::upgrade) {
                let mut index = position.get();
                edit.remap(&mut index);
                position.set(index);
            }
        }
        edit.remap(&mut self.execution.flush_unit_output_start);
        if let Some(author) = &mut self.execution.author_execution {
            edit.remap(&mut author.field_output_start);
        }
        if let Some(definition) = &mut self.execution.definition {
            if let Some(field) = &mut definition.no_break {
                field.remap_output_positions(edit);
            }
            if let Some(index) = &mut definition.hang_row.provisional_trailing_break {
                edit.remap(index);
            }
        }
    }

    /// Already executed empty endlines in the accepted output tail.
    pub(in crate::mandoc) fn completed_empty_rows(&self) -> u16 {
        u16::try_from(projection::trailing_completed_row_origins(&self.nodes).len())
            .unwrap_or(u16::MAX)
    }
    /// Retain one actual native graph-row end in its current output owner.
    /// This is a boundary receipt, not an additional completed empty row.
    pub(in crate::mandoc) fn record_device_row_end(&mut self) {
        self.nodes.push(Inline::anchor(INTERNAL_DEVICE_ROW_END));
    }

    /// Record an executed empty row in its active output owner. A later
    /// provisional glyph cannot revoke it when native acceptance rejects
    /// that glyph; actual accepted content determines its final placement.
    pub(in crate::mandoc) fn record_completed_vertical_rows(&mut self, rows: u16) {
        self.record_completed_rows(rows, CompletedRowOrigin::Layout);
    }

    pub(in crate::mandoc) fn record_completed_rows(
        &mut self,
        rows: u16,
        origin: CompletedRowOrigin,
    ) {
        Self::record_completed_rows_at(&mut self.execution, &mut self.nodes, rows, origin);
    }

    pub(in crate::mandoc::inline::flow) fn record_completed_rows_at(
        execution: &mut super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
        rows: u16,
        origin: CompletedRowOrigin,
    ) {
        execution.completed_vertical_rows = execution.completed_vertical_rows.saturating_add(rows);
        let marker = match origin {
            CompletedRowOrigin::Layout => INTERNAL_COMPLETED_ROW,
            CompletedRowOrigin::LiteralText => INTERNAL_LITERAL_ROW,
        };
        nodes.extend(std::iter::repeat_n(
            Inline::anchor(marker),
            usize::from(rows),
        ));
    }

    pub(in crate::mandoc) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Record an output slice before its source executes. The checkpoint
    /// owns no formatter registers and cannot roll execution back.
    pub(in crate::mandoc) fn begin_output_checkpoint(&mut self) -> OutputCheckpoint {
        let position = std::rc::Rc::new(std::cell::Cell::new(self.nodes.len()));
        let registry = self
            .output_positions
            .get_or_insert_with(std::rc::Rc::default);
        registry
            .borrow_mut()
            .push(std::rc::Rc::downgrade(&position));
        OutputCheckpoint {
            position,
            registry: std::rc::Rc::downgrade(registry),
        }
    }

    fn output_checkpoint_index(&self, checkpoint: &OutputCheckpoint) -> usize {
        checkpoint.position.get().min(self.nodes.len())
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
                | Inline::Link { children, .. } => contains_glyph(children),
                Inline::Anchor { .. } | Inline::LineBreak { .. } => false,
            })
        }
        let present = contains_glyph(&self.nodes[self.output_checkpoint_index(&checkpoint)..]);
        drop(checkpoint);
        present
    }

    /// Wrap the output emitted since `checkpoint` without replaying its
    /// formatter execution. Links and other semantic wrappers are IR
    /// annotations over an already-executed source stream.
    pub(in crate::mandoc) fn wrap_output_since(
        &mut self,
        checkpoint: OutputCheckpoint,
        wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        self.wrap_output_from(self.output_checkpoint_index(&checkpoint), wrap);
        drop(checkpoint);
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
    pub(in crate::mandoc) fn hard_break(&mut self) {
        // term_newln() selects NOSPACE before even testing lastcol/viscol
        // (term.c:475-481). An empty word may have cleared it without
        // buffering a cell, so a no-output return still owns this update.
        self.execution.boundary = PendingBoundary::Tight;
        self.hard_break_using_field(None, false);
        self.execution.boundary = PendingBoundary::Tight;
    }

    /// The field's real flush already applied acceptance. Project its tail
    /// without re-querying the old buffer or rejecting it a second time.
    pub(in crate::mandoc::inline::flow) fn hard_break_after_field_projection(
        &mut self,
        native: Option<&super::definition::NativeFieldDevice>,
    ) {
        self.hard_break_using_field(native, true);
    }

    fn hard_break_using_field(
        &mut self,
        captured: Option<&super::definition::NativeFieldDevice>,
        projected: bool,
    ) {
        let had_native_buffer = self.definition.as_ref().is_some_and(|state| {
            state.field_buffer.resume_offset() < state.field_buffer.cells().len()
        }) || (self.execution.definition.is_none()
            && self.execution.flush_unit.resume_offset() < self.execution.flush_unit.cells().len());
        let unprinted_field_tail = had_native_buffer
            && if projected {
                captured.is_some_and(super::definition::NativeFieldDevice::closes_unprinted_row)
            } else {
                self.native_field_closes_unprinted_row()
            };
        let exited_discarded_buffer = self.discarded_exited_definition_buffer();
        let exited_definition_row = self
            .execution
            .detached_device_row
            .is_some_and(|row| row.page_origin_printed || row.viscol > 0)
            || self
                .execution
                .definition
                .as_ref()
                .is_some_and(|definition| {
                    definition.hang_row.native_row_occupied()
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
        let plain_flush_rejection = if projected {
            self.execution
                .definition
                .as_ref()
                .is_some_and(|state| state.hang_row.field_discarded)
        } else if self.in_definition_field() {
            // term_newln() flushes the same tcol->buf after a TAG request
            // cleared NOBREAK. A definition session still owns that input;
            // consume its receipt before the row reset retires the cells.
            self.discard_unprinted_definition_field_output()
        } else {
            self.retire_plain_flush_unit()
        };
        if !self.execution.has_printable_content
            && self.execution.leading_line_boundary == super::LeadingLineBoundary::None
        {
            self.execution.leading_line_boundary =
                super::LeadingLineBoundary::AtVisibleCheckpoint(self.execution.visible_glyph_epoch);
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
        if plain_flush_rejection {
            // This real term_newln() finished a rejected nbr=0 pass. Its
            // row is complete now, even if another native unit starts before
            // the paragraph owner drains (term.c:143-146,250-253).
            self.record_completed_vertical_rows(1);
        } else if unprinted_field_tail
            || !self.in_definition_field()
                && self.execution.completed_vertical_rows > 0
                && self.execution.formatter_column == FormatterColumn::Advanced
                && !self
                    .nodes
                    .iter()
                    .rev()
                    .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
                    .any(|node| has_rendered_formatter_glyph(std::slice::from_ref(node)))
        {
            // term_newln() commits a whitespace-only formatter row even
            // though term_fill() prints no glyphs. When a later empty TEXT
            // requests another term_vspace(), that committed row must remain
            // in the completed-row count after formatter_column resets.
            self.record_completed_vertical_rows(1);
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
        self.finish_hard_break_projection(
            exited_definition_row,
            exited_discarded_buffer,
            retiring_rejection,
        );
        // term_newln() does not clear TERMP_NONEWLINE; the next word does.
    }

    fn finish_hard_break_projection(
        &mut self,
        exited_definition_row: bool,
        exited_discarded_buffer: bool,
        retiring_rejection: bool,
    ) {
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
            push_row_break(&mut self.nodes, row_indent);
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
            self.execution.detached_device_row = None;
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
            let actual_link = accepted
                .last()
                .is_some_and(|node| !super::super::links::presentation::is_private_owner(node));
            let split_link = actual_link
                && matches!((accepted.last(), pending.last()),
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
        let checkpoint = self.begin_output_checkpoint();
        append(self);
        self.wrap_output_since(checkpoint, style);
    }

    /// Annotate operands without capturing their executed automatic prefix.
    /// `term_word()` writes that prefix before encoding the source spelling
    /// (term.c:573-589); inspecting leading whitespace cannot distinguish it
    /// from the author's own blank glyphs. These bounded local markers do.
    /// Return whether this occurrence produced a typed annotation prefix;
    /// a cached tail can rejoin only that prefix, never a preceding occurrence.
    pub(in crate::mandoc) fn append_semantic_scope(
        &mut self,
        owner: u32,
        append: impl FnOnce(&mut Self),
        mut wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) -> bool {
        let scope = format!("{INTERNAL_OUTPUT_SCOPE}semantic:{owner}");
        let content = format!("{scope}:content");
        self.begin_output_scope(&scope);
        self.pending_output_scope_prefixes.push(content.clone());
        append(self);
        self.pending_output_scope_prefixes
            .retain(|pending| pending != &content);
        let mut started = false;
        let mut annotated = false;
        self.wrap_output_scope(&scope, |children| {
            if started {
                let output = wrap(children);
                annotated |= output
                    .iter()
                    .any(|node| matches!(node, Inline::Link { .. }));
                return output;
            }
            let (mut prefix, children, found) =
                projection::split_output_scope_prefix(children, &content);
            started = found;
            if found {
                let output = wrap(children);
                annotated |= output
                    .iter()
                    .any(|node| matches!(node, Inline::Link { .. }));
                prefix.extend(output);
            }
            prefix
        });
        annotated
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
