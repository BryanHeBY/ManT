//! Project field padding and deferred word spacing at accepted boundaries.

use super::{FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput, push_text};

impl InlineBuilder {
    pub(super) fn mark_native_field_prefix(&mut self, owner: &str) {
        let serial = owner
            .strip_prefix(super::super::INTERNAL_FIELD_WORD)
            .expect("native word owner");
        self.nodes.push(Inline::anchor(format!(
            "{}{serial}",
            super::super::INTERNAL_FIELD_PREFIX
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

    pub(super) fn append_device_field_padding(&mut self) {
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

    pub(super) fn append_boundary_spacing(
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

    pub(super) fn projects_automatic_space(
        &self,
        boundary: PendingBoundary,
        add_space: bool,
    ) -> bool {
        add_space
            && !boundary.is_tight()
            && (self.execution.spacing.enabled()
                || matches!(
                    boundary,
                    PendingBoundary::Preserved | PendingBoundary::Continued | PendingBoundary::Kept
                ))
    }

    pub(super) fn defer_current_word_spaces(&mut self, count: usize) {
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
                .push(super::super::super::DeferredWordSpace {
                    owner: self.execution.native_owner_serial,
                    start: anchor.start,
                    count,
                });
        }
    }

    /// Project the earlier word intervals before starting the next word's
    /// scalar range. Stable markers can re-enter an owner after wrapping,
    /// just like the existing pending-glyph projection.
    pub(in crate::mandoc::inline::flow::output) fn materialize_pending_word_spaces(
        &mut self,
    ) -> usize {
        let mut spaces = std::mem::take(&mut self.execution.pending_word_spaces);
        let mut count = 0usize;
        let mut last_pass = None;
        for pending in spaces.drain(..) {
            use super::super::super::field_buffer::FieldCell;
            let (buffer, _) = self.execution.word_input();
            let start = pending.start;
            let end = pending
                .start
                .saturating_add(pending.count)
                .min(buffer.cells().len());
            let width = (start..end)
                .filter(|&cell| {
                    matches!(
                        buffer.cells()[cell],
                        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
                    ) && buffer.projection_length(cell, cell + 1) > 0
                        && buffer.word_space_is_printed_or_live(cell, &mut last_pass)
                })
                .count();
            if width == 0 {
                continue;
            }
            self.nodes.push(Inline::anchor(format!(
                "{}{}",
                super::super::INTERNAL_FIELD_WORD,
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

    pub(super) fn push_field_owned_breakable_space(&mut self, spaces: String) {
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
