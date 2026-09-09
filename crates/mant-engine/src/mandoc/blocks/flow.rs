//! Paragraph and literal flow own pending text, provenance and flush boundaries.
use super::{FilledBoundary, InlineBuilder, layout, targets, updated_spacing};
use mant_ir::{Block, Inline};

pub(super) struct BlockState {
    pub(super) output: Vec<Block>,
    pub(super) paragraph: InlineBuilder,
    paragraph_source: Option<mant_ir::SourceSpan>,
    paragraph_last_line: Option<u32>,
    preformatted: Vec<Inline>,
    pre_source: Option<mant_ir::SourceSpan>,
    preformatted_tight_boundary: bool,
    preformatted_row_occupied: bool,
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
            paragraph: InlineBuilder::with_spacing(spacing_enabled),
            paragraph_source: None,
            paragraph_last_line: None,
            preformatted: Vec::new(),
            pre_source: None,
            preformatted_tight_boundary: false,
            preformatted_row_occupied: false,
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
        let source_line = source.map(|span| span.line);
        let crossed_source_line = self
            .paragraph_last_line
            .zip(source_line)
            .is_some_and(|(previous, current)| current > previous);
        let boundary = if self.paragraph.has_tight_boundary() || !crossed_source_line {
            FilledBoundary::SameLine
        } else if starts_indented_line {
            FilledBoundary::LineBreak
        } else {
            FilledBoundary::Word
        };
        if boundary == FilledBoundary::LineBreak {
            self.paragraph.hard_break();
        } else if boundary == FilledBoundary::Word && ordinary_text {
            self.paragraph.preserve_source_word_boundary();
        }
        let previous_count = self.paragraph.node_count();
        append(&mut self.paragraph);
        if continues_line {
            self.paragraph.tighten_next_boundary();
        }
        if self.paragraph.node_count() != previous_count {
            if self.paragraph_source.is_none() {
                self.paragraph_source = source;
            }
            if source_line.is_some() {
                self.paragraph_last_line = source_line;
            }
        }
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
        mut nodes: Vec<Inline>,
        source: Option<mant_ir::SourceSpan>,
        continues_line: bool,
        starts_line: bool,
        occupies_row: bool,
    ) {
        self.flush_paragraph();
        if nodes.is_empty() && occupies_row {
            nodes.push(Inline::Text {
                value: String::new(),
            });
        }
        if starts_line && self.preformatted_row_occupied && !self.preformatted_tight_boundary {
            if self.hanging_origin.is_some() {
                // An HP entered while already in no-fill mode still has one
                // first line at the macro origin. Materialize that line
                // before adopting the permanent hanging origin. A continued
                // source line (\c) does not reach this boundary.
                self.flush_preformatted();
            } else {
                self.preformatted.push(Inline::LineBreak);
            }
            self.preformatted_row_occupied = false;
        }
        self.preformatted.extend(nodes);
        self.preformatted_row_occupied |= occupies_row;
        self.preformatted_tight_boundary = continues_line;
        if self.pre_source.is_none() {
            self.pre_source = source;
        }
    }

    pub(super) fn flush_paragraph(&mut self) {
        let output_start = self.output.len();
        flush_paragraph(
            &mut self.output,
            &mut self.paragraph,
            &mut self.paragraph_source,
            self.indent_columns,
            self.spacing_enabled,
        );
        if self.output.len() > output_start {
            if let Some(origin) = self.hanging_origin
                && let Some(Block::Paragraph { layout, .. }) = self.output.last_mut()
            {
                layout.continuation_indent_columns = origin.offset_from(self.indent_columns);
            }
            self.consume_hanging_first_line();
        }
        self.attach_pending_to_new_output(output_start);
        self.paragraph_last_line = None;
    }

    pub(super) fn flush_preformatted(&mut self) {
        // A pure formatter word can close the preceding row without
        // occupying another one. Literal blank rows end in an explicit
        // empty text sentinel and are therefore not trimmed here.
        if !self.preformatted_row_occupied
            && matches!(self.preformatted.last(), Some(Inline::LineBreak))
        {
            self.preformatted.pop();
        }
        let output_start = self.output.len();
        flush_preformatted(
            &mut self.output,
            &mut self.preformatted,
            &mut self.pre_source,
            self.indent_columns,
        );
        if self.output.len() > output_start {
            self.consume_hanging_first_line();
        }
        self.attach_pending_to_new_output(output_start);
        self.preformatted_tight_boundary = false;
        self.preformatted_row_occupied = false;
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
        Block::Preformatted { children, .. } => mant_protocol::geometry::has_literal_rows(children),
        Block::Paragraph { children, .. } => crate::inline::has_printable_character(children),
        _ => false,
    })
}

fn flush_paragraph(
    output: &mut Vec<Block>,
    paragraph: &mut InlineBuilder,
    source: &mut Option<mant_ir::SourceSpan>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    spacing_enabled: bool,
) {
    let current =
        std::mem::replace(paragraph, InlineBuilder::with_spacing(spacing_enabled)).finish();
    if current.is_empty() {
        *source = None;
    } else {
        output.push(Block::Paragraph {
            children: current,
            layout: layout(indent_columns),
            source: source.take(),
        });
    }
}

fn flush_preformatted(
    output: &mut Vec<Block>,
    preformatted: &mut Vec<Inline>,
    source: &mut Option<mant_ir::SourceSpan>,
    indent_columns: crate::mandoc::layout::SourceIndent,
) {
    if preformatted.is_empty() {
        *source = None;
        return;
    }
    output.push(Block::Preformatted {
        children: std::mem::take(preformatted),
        language: None,
        layout: layout(indent_columns),
        source: source.take(),
    });
}
