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
    pub(super) fn insert_link_cursor(&mut self, marker: String) {
        self.nodes.push(Inline::anchor(marker));
    }

    pub(super) fn discard_link_cursor(&mut self, marker: &str) {
        super::super::man_links::remove_link_cursor(&mut self.nodes, marker);
    }

    pub(super) fn wrap_first_link(
        &mut self,
        target: &mant_ir::LinkTarget,
        marker: &str,
        started: &mut bool,
        skip_visible: &mut usize,
    ) -> bool {
        super::super::man_links::wrap_first_visible_inline(
            &mut self.nodes,
            target,
            marker,
            started,
            skip_visible,
        )
    }

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

    /// Move the live projection destination without executing `term_newln`.
    /// Native cell and owner offsets address this same ordered node vector.
    pub(super) fn take_active_output(&mut self) -> (Vec<Inline>, Option<SourceSpan>) {
        self.last_line = None;
        (std::mem::take(&mut self.nodes), self.source.take())
    }

    /// The detached definition HEAD already represents its pending native
    /// row. Drop that row at the moment a real BODY break closes it, while
    /// retaining target anchors that identify the following visible word.
    pub(super) fn consume_invisible_head_row(&mut self) -> bool {
        let Some(break_index) = self
            .nodes
            .iter()
            .position(|node| matches!(node, Inline::LineBreak { .. }))
        else {
            return false;
        };
        if crate::mandoc::inline::has_rendered_formatter_glyph(&self.nodes[..break_index]) {
            return false;
        }
        let mut identities = self.nodes.drain(..=break_index).collect::<Vec<_>>();
        crate::mandoc::inline::retain_inline_identities(&mut identities);
        self.nodes.splice(0..0, identities);
        true
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

    pub(super) fn append_run_in_cells(
        &mut self,
        formatter: &mut FormatterState,
        count: usize,
        native_count: usize,
        generated_word: bool,
    ) {
        self.with_inline_builder(formatter, |builder| {
            builder.append_run_in_cells(count, native_count, generated_word);
        });
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
        // The definition BODY's row decision reads TERMP_NONEWLINE as it
        // stands at this word's source-line entry, before the word's own
        // term_word() clears it (term.c:588 with mdoc_term.c:314-317).
        formatter.note_definition_source_line();
        let changed = self.with_inline_builder(formatter, |builder| {
            let boundary = filled_fragment_boundary(
                builder,
                starts_indented_line && has_executed_predecessor,
                crossed_source_line,
            );
            append_filled_fragment(builder, boundary, continues_line, ordinary_text, append)
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
        line_request: bool,
    ) -> (Option<Block>, bool, u16) {
        let (children, empty_word_end_break, completed_vertical_rows) = self
            .with_inline_builder(formatter, |builder| {
                builder.take_paragraph_segment(line_request)
            });
        let source = self.source.take();
        self.last_line = None;
        (
            (!children.is_empty()).then(|| Block::Paragraph {
                children,
                layout: layout(indent),
                source,
            }),
            empty_word_end_break,
            completed_vertical_rows,
        )
    }
}

/// A filled word executes identically when its still-open physical row has
/// literal presentation. The destination does not choose another executor.
pub(super) fn append_filled_fragment(
    builder: &mut InlineBuilder,
    boundary: FilledBoundary,
    continues_line: bool,
    ordinary_text: bool,
    append: impl FnOnce(&mut InlineBuilder),
) -> bool {
    if boundary == FilledBoundary::LineBreak {
        builder.hard_break();
    } else if boundary == FilledBoundary::Word && ordinary_text {
        builder.preserve_source_word_boundary();
    }
    if builder.final_source_continuation_or(false) && !builder.has_tight_boundary() {
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
}

pub(super) fn filled_fragment_boundary(
    builder: &InlineBuilder,
    starts_indented_row: bool,
    crossed_source_line: bool,
) -> FilledBoundary {
    if builder.final_source_continuation_or(false) {
        FilledBoundary::SameLine
    } else if starts_indented_row {
        // CVS print_mdoc_node() executes NODE_LINE before a word,
        // including generated punctuation and transparent wrappers.
        FilledBoundary::LineBreak
    } else if builder.has_tight_boundary() || !crossed_source_line {
        FilledBoundary::SameLine
    } else {
        FilledBoundary::Word
    }
}
