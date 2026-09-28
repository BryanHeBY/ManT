//! Filled IR content and its source positions; text registers live in the formatter.
use super::{FilledBoundary, InlineBuilder, layout};
use crate::mandoc::formatter::FormatterState;
use mant_ir::{Block, Inline, SourceSpan};

pub(super) struct ParagraphFlow {
    nodes: Vec<Inline>,
    source: Option<SourceSpan>,
    last_line: Option<u32>,
}

impl ParagraphFlow {
    pub(super) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            source: None,
            last_line: None,
        }
    }

    /// Give one output segment to the active formatter state for the duration
    /// of an operation. The same registers return before the next node enters;
    /// neither a paragraph flush nor a different IR destination clones them.
    pub(super) fn with_inline_builder<R>(
        &mut self,
        formatter: &mut FormatterState,
        operation: impl FnOnce(&mut InlineBuilder) -> R,
    ) -> R {
        formatter.with_output_builder(&mut self.nodes, operation)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub(super) fn hard_break(&mut self, formatter: &mut FormatterState) {
        self.with_inline_builder(formatter, InlineBuilder::hard_break);
    }

    pub(super) fn tighten_next_boundary(&mut self, formatter: &mut FormatterState) {
        self.with_inline_builder(formatter, InlineBuilder::tighten_next_boundary);
    }

    pub(super) fn no_break_flush(&mut self, formatter: &mut FormatterState) {
        self.with_inline_builder(formatter, InlineBuilder::no_break_flush);
    }

    pub(super) fn resolve_vertical_space(
        &mut self,
        formatter: &mut FormatterState,
        rows: i32,
    ) -> u16 {
        self.with_inline_builder(formatter, |builder| builder.resolve_vertical_space(rows))
    }

    pub(super) fn inherit_preserved_execution(
        &mut self,
        formatter: &mut FormatterState,
        state: crate::mandoc::inline::PreservedInlineState,
    ) {
        self.last_line = state.last_executed_source_line;
        self.with_inline_builder(formatter, |builder| {
            builder.inherit_preserved_execution(state);
        });
    }

    pub(super) fn append_run_in_cells(&mut self, formatter: &mut FormatterState, count: usize) {
        self.with_inline_builder(formatter, |builder| builder.append_run_in_cells(count));
    }

    pub(super) fn append(
        &mut self,
        formatter: &mut FormatterState,
        source: Option<SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
        ordinary_text: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        let source_line = source.map(|span| span.line);
        let crossed_source_line = self
            .last_line
            .zip(source_line)
            .is_some_and(|(previous, current)| current > previous);
        let has_executed_predecessor =
            self.last_line.is_some() || formatter.execution.has_formatter_cell();
        let changed = self.with_inline_builder(formatter, |builder| {
            let source_continues = builder.final_source_continuation_or(false);
            let boundary = if source_continues {
                FilledBoundary::SameLine
            } else if starts_indented_line && has_executed_predecessor {
                // CVS print_mdoc_node() executes NODE_LINE before a word,
                // including generated punctuation and transparent wrappers.
                FilledBoundary::LineBreak
            } else if builder.has_tight_boundary() || !crossed_source_line {
                FilledBoundary::SameLine
            } else {
                FilledBoundary::Word
            };
            if boundary == FilledBoundary::LineBreak {
                builder.hard_break();
            } else if boundary == FilledBoundary::Word && ordinary_text {
                builder.preserve_source_word_boundary();
            }
            if source_continues && !builder.has_tight_boundary() {
                builder.preserve_continued_boundary();
            }
            let previous_count = builder.node_count();
            let fragment = builder.begin_source_fragment();
            append(builder);
            builder.finish_source_fragment(fragment);
            if builder.final_word_join_or(continues_line) {
                builder.tighten_next_boundary();
            }
            builder.node_count() != previous_count
        });
        if changed {
            if self.source.is_none() {
                self.source = source;
            }
            if source_line.is_some() {
                self.last_line = source_line;
            }
        }
    }

    /// Drain one IR segment while the formatter retains the execution state.
    pub(super) fn take(
        &mut self,
        formatter: &mut FormatterState,
        indent: crate::mandoc::layout::SourceIndent,
    ) -> (Option<Block>, bool) {
        let (children, empty_word_end_break) =
            self.with_inline_builder(formatter, InlineBuilder::take_paragraph_segment);
        let source = self.source.take();
        self.last_line = None;
        (
            (!children.is_empty()).then(|| Block::Paragraph {
                children,
                layout: layout(indent),
                source,
            }),
            empty_word_end_break,
        )
    }
}
