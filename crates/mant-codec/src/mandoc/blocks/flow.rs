//! Paragraph and literal flow own pending text, provenance and flush boundaries.
use super::{FilledBoundary, InlineBuilder, layout, targets};
use mant_ir::{Block, Inline};

mod literal;
mod paragraph;
#[cfg(test)]
mod tests;
use literal::LiteralFlow;
use paragraph::ParagraphFlow;

pub(super) struct BlockState {
    pub(super) output: Vec<Block>,
    pub(super) formatter: crate::mandoc::formatter::FormatterState,
    // Filled and literal buffers are independent, not mutually exclusive modes.
    paragraph: ParagraphFlow,
    literal: LiteralFlow,
    pending_targets: targets::PendingTargets,
    indent_columns: crate::mandoc::layout::SourceIndent,
    hanging_origin: Option<crate::mandoc::layout::SourceIndent>,
}

#[derive(Clone, Copy)]
pub(super) struct LinkOutputCursor {
    output: usize,
    paragraph: usize,
    literal: usize,
}

impl BlockState {
    pub(super) fn link_output_cursor(&self) -> LinkOutputCursor {
        LinkOutputCursor {
            output: self.output.len(),
            paragraph: self.paragraph.node_count(),
            literal: self.literal.node_count(),
        }
    }

    pub(super) fn wrap_first_link_since(
        &mut self,
        cursor: LinkOutputCursor,
        target: &mant_ir::LinkTarget,
        skip_prior_glyph: bool,
    ) -> bool {
        let mut skip_visible = usize::from(skip_prior_glyph);
        let mut paragraph_offset = Some(cursor.paragraph);
        let mut literal_offset = Some(cursor.literal);
        let output_start = cursor.output.min(self.output.len());
        for block in &mut self.output[output_start..] {
            let offset = match block {
                Block::Paragraph { .. } => paragraph_offset.take().unwrap_or(0),
                Block::Preformatted { .. } => literal_offset.take().unwrap_or(0),
                _ => 0,
            };
            if super::man_links::wrap_first_visible_block(block, target, offset, &mut skip_visible)
            {
                return true;
            }
        }
        self.paragraph
            .wrap_first_link(target, paragraph_offset.unwrap_or(0), &mut skip_visible)
            || self
                .literal
                .wrap_first_link(target, literal_offset.unwrap_or(0), &mut skip_visible)
    }

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
        // Native fi/nf execute term_newln even when the requested mode is
        // already active. Keep adjacent literal blocks coalesced while ending
        // their pending row (including a continued word), not adding a gap.
        if self.hanging_origin.is_some() {
            self.flush_preformatted();
            self.flush_paragraph_for_line_request();
            self.consume_hanging_first_line();
        } else {
            self.literal.end_line();
            self.flush_paragraph_for_line_request();
        }
        // .nf/.fi have an authored source position and close the current
        // terminal row. The next no-fill word cannot still consume a pending
        // definition HEAD row (man_term.c::print_man_node, NODE_NOFILL).
        self.formatter.settle_definition_head_rows();
    }

    /// A mdoc container's `NODE_LINE` is executed before its HEAD children.
    /// Those children can lack `NODE_LINE` themselves (notably Eo operands).
    pub(super) fn begin_no_fill_source_line(&mut self) {
        if self.literal.starts_new_row(true) {
            self.literal.end_line();
        }
    }
    pub(super) fn with_output(
        indent_columns: crate::mandoc::layout::SourceIndent,
        spacing_enabled: bool,
        output: Vec<Block>,
        mut formatter: crate::mandoc::formatter::FormatterState,
    ) -> Self {
        formatter.set_spacing_enabled(spacing_enabled);
        Self {
            output,
            formatter,
            paragraph: ParagraphFlow::new(),
            literal: LiteralFlow::new(),
            pending_targets: targets::PendingTargets::new(),
            indent_columns,
            hanging_origin: None,
        }
    }

    pub(super) fn inherit_scope_posts(&mut self, posts: crate::mandoc::containers::ScopePostState) {
        self.formatter.execution.scope_posts = posts;
    }

    pub(super) fn spacing_enabled(&self) -> bool {
        self.formatter.spacing_enabled()
    }

    pub(super) fn set_spacing(&mut self, setting: &str) {
        self.paragraph
            .with_inline_builder(&mut self.formatter, |builder| {
                builder.set_spacing(setting);
            });
    }

    #[cfg(test)]
    pub(super) fn push_inline(
        &mut self,
        nodes: Vec<Inline>,
        source: Option<mant_ir::SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
    ) {
        if nodes.is_empty() {
            if continues_line {
                self.paragraph.tighten_next_boundary(&mut self.formatter);
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
            &mut self.formatter,
            source,
            starts_indented_line,
            continues_line,
            ordinary_text,
            append,
        );
        if self.formatter.definition_before_visible()
            && self.formatter.execution.take_leading_line_boundary()
        {
            self.formatter.note_definition_boundary();
            if self.formatter.definition_head_row_pending()
                && self.paragraph.consume_invisible_head_row()
            {
                self.formatter.consume_definition_head_row();
            }
            // Inline macro pre-handlers, such as An in AUTHORS, execute
            // term_newln() in the same source stream as block requests.
            self.formatter.settle_definition_head_rows();
        }
        if self.formatter.definition_before_visible()
            && self.formatter.execution.has_executed_visible_content()
        {
            self.formatter.note_definition_visible();
        }
    }

    pub(super) fn paragraph_is_empty(&self) -> bool {
        self.paragraph.is_empty()
    }

    pub(super) fn hard_break(&mut self) {
        self.formatter.note_definition_boundary();
        self.paragraph.hard_break(&mut self.formatter);
        if self.formatter.definition_head_row_pending()
            && self.paragraph.consume_invisible_head_row()
        {
            self.formatter.consume_definition_head_row();
        }
        self.formatter.settle_definition_head_rows();
    }

    pub(super) fn tighten_next_boundary(&mut self) {
        self.paragraph.tighten_next_boundary(&mut self.formatter);
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
        // The no-fill word has already executed against the same formatter.
        // Flushing an empty paragraph here would reset its live row state.
        if !self.formatter.no_fill || !self.paragraph.is_empty() {
            self.flush_paragraph();
        }
        if mant_ir::has_printable_character(&nodes) {
            self.formatter.note_definition_visible();
        }
        if self.hanging_origin.is_some() && self.literal.starts_new_row(starts_line) {
            // Materialize HP's temporary first row before adopting its permanent
            // body origin. A continued source line does not reach this boundary.
            self.flush_preformatted();
        }
        self.literal
            .append(nodes, source, continues_line, starts_line, occupies_row);
    }

    pub(super) fn adopt_trailing_preformatted(&mut self) -> bool {
        if !self.literal.is_empty() {
            return false;
        }
        if !matches!(self.output.last(), Some(Block::Preformatted { .. })) {
            return false;
        }
        let Some(Block::Preformatted {
            children,
            layout,
            source,
            ..
        }) = self.output.pop()
        else {
            unreachable!("the last block was checked above")
        };
        self.literal.adopt(children, source, layout);
        true
    }

    pub(super) fn no_break_formatter_flush(&mut self, nodes: Vec<Inline>) {
        if !self.formatter.no_fill {
            self.paragraph.no_break_flush(&mut self.formatter);
        }
        self.literal.no_break_flush(nodes);
    }

    pub(super) fn has_formatter_cell(&self) -> bool {
        self.formatter.execution.has_formatter_cell() || self.literal.has_formatter_column()
    }

    pub(super) fn resolve_vertical_space(&mut self, rows: i32) -> u16 {
        self.formatter.note_definition_boundary();
        self.paragraph
            .resolve_vertical_space(&mut self.formatter, rows)
    }

    /// A no-fill word clears CVS skipvsp; its zero-width registers are
    /// already updated by the shared text executor.
    pub(super) fn clear_formatter_word_debt(&mut self) {
        self.formatter.vertical_space_debt = 0;
    }

    pub(super) fn inherit_author_execution(
        &mut self,
        flow: crate::mandoc::formatter::AuthorFlow,
        authors_section: bool,
    ) {
        self.paragraph
            .with_inline_builder(&mut self.formatter, |builder| {
                builder.inherit_author_execution(flow, authors_section);
            });
    }

    pub(super) fn inherit_run_in_execution(
        &mut self,
        state: crate::mandoc::inline::PreservedInlineState,
        generated_cells: usize,
    ) {
        self.paragraph
            .inherit_preserved_execution(&mut self.formatter, state);
        self.paragraph
            .append_run_in_cells(&mut self.formatter, generated_cells);
    }

    pub(super) fn flush_paragraph(&mut self) {
        self.flush_paragraph_with(false);
    }

    pub(super) fn flush_paragraph_for_line_request(&mut self) {
        self.flush_paragraph_with(true);
    }

    fn flush_paragraph_with(&mut self, line_request: bool) {
        if self.formatter.no_fill && self.paragraph.is_empty() && !line_request {
            // A block output boundary has no filled content to drain. The
            // current no-fill formatter row remains live until term_newln().
            return;
        }
        let output_start = self.output.len();
        let (block, empty_word_end_break) = self
            .paragraph
            .take(&mut self.formatter, self.indent_columns);
        let mut suppressed_head_row = false;
        if let Some(block) = block {
            match block {
                Block::Paragraph {
                    children,
                    layout,
                    source,
                } if !mant_ir::has_printable_character(&children)
                    && children.iter().any(
                        |inline| matches!(inline, Inline::Text { value } if value.is_empty()),
                    ) =>
                {
                    // An explicit empty formatter cell (for example `\&`)
                    // owns one physical row. Empty paragraphs are otherwise
                    // presentation-neutral, so encode the row as spacing.
                    // Retain zero-width targets at that row during the
                    // representation change.
                    let completed_rows = children
                        .iter()
                        .filter(|inline| matches!(inline, Inline::LineBreak))
                        .count();
                    let active_row = children
                        .iter()
                        .rev()
                        .take_while(|inline| !matches!(inline, Inline::LineBreak))
                        .any(|inline| !matches!(inline, Inline::Anchor { .. }));
                    let rows = u16::try_from(completed_rows + usize::from(active_row))
                        .unwrap_or(u16::MAX)
                        .max(1);
                    let anchors = children
                        .into_iter()
                        .filter(|inline| matches!(inline, Inline::Anchor { .. }))
                        .collect::<Vec<_>>();
                    if !anchors.is_empty() {
                        self.output.push(Block::Paragraph {
                            children: anchors,
                            layout,
                            source,
                        });
                    }
                    suppressed_head_row = self.formatter.consume_definition_head_row();
                    let body_rows = rows.saturating_sub(u16::from(suppressed_head_row));
                    if body_rows > 0 {
                        self.output.push(Block::VerticalSpace {
                            lines: body_rows,
                            source,
                        });
                    }
                }
                block => self.output.push(block),
            }
        }
        if empty_word_end_break && !suppressed_head_row {
            // A bare \p can occupy the native tag row without leaving an
            // inline paragraph. Consume that row at this actual flush, just
            // as for an explicit empty formatter cell above.
            if !self.formatter.consume_definition_head_row() {
                self.output.push(Block::VerticalSpace {
                    lines: 1,
                    source: None,
                });
            }
        }
        if line_request {
            // roff_term_pre_sp() calls term_vspace() (and thus term_newln())
            // before adding its own empty row. Settle the native tag row now;
            // subsequent control-only BODY words own independent rows.
            self.formatter.settle_definition_head_rows();
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

    #[cfg(test)]
    pub(super) fn finish(mut self) -> Vec<Block> {
        self.settle();
        self.output
    }

    /// Execute all pending formatter boundaries before exporting persistent
    /// state to an enclosing structural driver.
    pub(super) fn finish_with_formatter(
        mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
    ) -> Vec<Block> {
        self.settle();
        *formatter = self.formatter;
        self.output
    }

    fn settle(&mut self) {
        self.flush_preformatted();
        self.flush_paragraph();
        let output_end = self.output.len();
        self.attach_pending_to_structural_output(output_end);
    }
}

/// Only actual buffered rows satisfy an unconditional formatter flush.
/// Zero-width target blocks remain available without masquerading as rows.
pub(super) fn has_flushed_row(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Preformatted { children, .. } => mant_ir::geometry::has_literal_rows(children),
        Block::Paragraph { children, .. } => mant_ir::has_printable_character(children),
        _ => false,
    })
}
