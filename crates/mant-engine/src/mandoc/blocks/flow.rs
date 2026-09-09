//! Paragraph and literal flow own pending text, provenance and flush boundaries.
use super::{FilledBoundary, InlineBuilder, layout, targets, updated_spacing};
use mant_ir::{Block, Inline};

mod literal;
mod paragraph;
#[cfg(test)]
mod tests;
use literal::LiteralFlow;
use paragraph::ParagraphFlow;

pub(super) struct BlockState {
    pub(super) output: Vec<Block>,
    // Filled and literal buffers are independent, not mutually exclusive modes.
    paragraph: ParagraphFlow,
    literal: LiteralFlow,
    pending_targets: targets::PendingTargets,
    indent_columns: crate::mandoc::layout::SourceIndent,
    hanging_origin: Option<crate::mandoc::layout::SourceIndent>,
    spacing_enabled: bool,
}

impl BlockState {
    pub(super) const fn source_indent(&self) -> crate::mandoc::layout::SourceIndent {
        self.indent_columns
    }

    pub(super) fn set_source_indent(&mut self, indent: crate::mandoc::layout::SourceIndent) {
        self.flush_preformatted();
        self.flush_paragraph();
        self.indent_columns = indent;
        self.hanging_origin = None;
    }

    pub(super) fn start_hanging(&mut self, origin: crate::mandoc::layout::SourceIndent) {
        self.hanging_origin = Some(origin);
    }

    pub(super) fn consume_hanging_first_line(&mut self) {
        if let Some(origin) = self.hanging_origin.take() {
            self.indent_columns = origin;
        }
    }

    pub(super) fn literal_mode_boundary(&mut self) {
        // The mode switch has a geometric effect only while the temporary
        // HP first line is pending. Otherwise retain the existing coalescing
        // of adjacent, geometrically equivalent no-fill regions.
        if self.hanging_origin.is_some() {
            self.flush_preformatted();
            self.flush_paragraph();
            self.consume_hanging_first_line();
        }
    }
    pub(super) const fn with_output(
        indent_columns: crate::mandoc::layout::SourceIndent,
        spacing_enabled: bool,
        output: Vec<Block>,
    ) -> Self {
        Self {
            output,
            paragraph: ParagraphFlow::new(spacing_enabled),
            literal: LiteralFlow::new(),
            pending_targets: targets::PendingTargets::new(),
            indent_columns,
            hanging_origin: None,
            spacing_enabled,
        }
    }

    pub(super) const fn spacing_enabled(&self) -> bool {
        self.spacing_enabled
    }

    pub(super) fn set_spacing(&mut self, setting: &str) {
        self.paragraph.set_spacing(setting);
        self.spacing_enabled = updated_spacing(self.spacing_enabled, setting);
    }

    /// Carry formatter state out of a structural subtree.
    ///
    /// Nested mdoc enclosures can contain `.Sm` transitions that affect later
    /// sibling nodes even though the enclosure itself is lowered by a nested
    /// block builder. The nested builder has already applied the transition at
    /// its source position, so the parent inherits only the final state.
    pub(super) fn inherit_spacing(&mut self, spacing_enabled: bool) {
        if spacing_enabled == self.spacing_enabled {
            return;
        }
        self.paragraph.inherit_spacing(spacing_enabled);
        self.spacing_enabled = spacing_enabled;
    }

    pub(super) fn push_inline(
        &mut self,
        nodes: Vec<Inline>,
        source: Option<mant_ir::SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
    ) {
        if nodes.is_empty() {
            if continues_line {
                self.paragraph.tighten_next_boundary();
            }
            return;
        }
        self.push_inline_with(source, starts_indented_line, continues_line, |paragraph| {
            paragraph.append(nodes);
        });
    }

    /// Lower siblings into the same flow so zero-width controls and two-sided
    /// punctuation can act on both the preceding and following visible runs.
    pub(super) fn push_inline_with(
        &mut self,
        source: Option<mant_ir::SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        self.push_source_inline_with(source, starts_indented_line, continues_line, false, append);
    }

    pub(super) fn push_source_inline_with(
        &mut self,
        source: Option<mant_ir::SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
        ordinary_text: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        self.paragraph.append(
            source,
            starts_indented_line,
            continues_line,
            ordinary_text,
            append,
        );
    }

    pub(super) fn paragraph_is_empty(&self) -> bool {
        self.paragraph.is_empty()
    }

    pub(super) fn hard_break(&mut self) {
        self.paragraph.hard_break();
    }

    pub(super) fn tighten_next_boundary(&mut self) {
        self.paragraph.tighten_next_boundary();
    }

    pub(super) fn queue_targets(
        &mut self,
        targets: impl IntoIterator<Item = String>,
        owner_source: Option<mant_ir::SourceSpan>,
    ) {
        self.pending_targets.queue(targets, owner_source);
    }

    pub(super) fn attach_pending_to_new_output(&mut self, output_start: usize) {
        if self.pending_targets.is_empty() || self.output.len() == output_start {
            return;
        }
        self.attach_pending_to_structural_output(output_start);
    }

    pub(super) fn attach_pending_to_structural_output(&mut self, output_start: usize) {
        if self.pending_targets.is_empty() {
            return;
        }
        let mut lowered = self.output.split_off(output_start.min(self.output.len()));
        self.pending_targets
            .attach_leading(&mut lowered, layout(self.indent_columns));
        self.output.append(&mut lowered);
    }

    pub(super) fn push_preformatted(
        &mut self,
        nodes: Vec<Inline>,
        source: Option<mant_ir::SourceSpan>,
        continues_line: bool,
        starts_line: bool,
        occupies_row: bool,
    ) {
        self.flush_paragraph();
        if self.hanging_origin.is_some() && self.literal.starts_new_row(starts_line) {
            // Materialize HP's temporary first row before adopting its permanent
            // body origin. A continued source line does not reach this boundary.
            self.flush_preformatted();
        }
        self.literal
            .append(nodes, source, continues_line, starts_line, occupies_row);
    }

    pub(super) fn flush_paragraph(&mut self) {
        let output_start = self.output.len();
        if let Some(block) = self
            .paragraph
            .take(self.indent_columns, self.spacing_enabled)
        {
            self.output.push(block);
        }
        if self.output.len() > output_start {
            if let Some(origin) = self.hanging_origin
                && let Some(Block::Paragraph { layout, .. }) = self.output.last_mut()
            {
                layout.continuation_indent_columns = origin.offset_from(self.indent_columns);
            }
            self.consume_hanging_first_line();
        }
        self.attach_pending_to_new_output(output_start);
    }

    pub(super) fn flush_preformatted(&mut self) {
        let output_start = self.output.len();
        if let Some(block) = self.literal.take(self.indent_columns) {
            self.output.push(block);
        }
        if self.output.len() > output_start {
            self.consume_hanging_first_line();
        }
        self.attach_pending_to_new_output(output_start);
    }

    /// Unlike an ordinary break, native `term_flushln` emits a row even
    /// after an embedded spacing/font request emptied its pending content.
    pub(super) fn flush_requested_line(&mut self, source: Option<mant_ir::SourceSpan>) {
        let start = self.output.len();
        self.flush_preformatted();
        self.flush_paragraph();
        if !has_flushed_row(&self.output[start..]) {
            let start = self.output.len();
            self.output.push(Block::Preformatted {
                children: vec![Inline::Text {
                    value: String::new(),
                }],
                language: None,
                layout: layout(self.indent_columns),
                source,
            });
            self.attach_pending_to_new_output(start);
        }
        self.consume_hanging_first_line();
    }

    pub(super) fn finish(mut self) -> Vec<Block> {
        self.flush_preformatted();
        self.flush_paragraph();
        let output_end = self.output.len();
        self.attach_pending_to_structural_output(output_end);
        self.output
    }
}

/// Only actual buffered rows satisfy an unconditional formatter flush.
/// Zero-width target blocks remain available without masquerading as rows.
pub(super) fn has_flushed_row(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Preformatted { children, .. } => mant_ir::geometry::has_literal_rows(children),
        Block::Paragraph { children, .. } => crate::inline::has_printable_character(children),
        _ => false,
    })
}
