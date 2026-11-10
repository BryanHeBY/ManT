//! Filled IR content and its source positions; text registers live in the formatter.
use super::{InlineBuilder, layout};
use crate::mandoc::formatter::FormatterState;
use crate::mandoc::layout::SourceIndent;
use mant_ir::{Block, Inline, SourceSpan};

pub(super) struct ParagraphFlow {
    nodes: Vec<Inline>,
    source: Option<SourceSpan>,
    hanging_first_line: Option<SourceIndent>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ParagraphDrain {
    Ordinary,
    LineRequest,
    BeforeNativeFlush,
    AfterNativeFlush,
}

impl ParagraphFlow {
    pub(super) fn output_nodes_mut(&mut self) -> &mut Vec<Inline> {
        &mut self.nodes
    }
    pub(super) fn note_source(&mut self, source: Option<SourceSpan>) {
        if self.source.is_none() {
            self.source = source;
        }
    }

    pub(super) fn insert_link_cursor(&mut self, marker: String) {
        self.nodes.push(Inline::anchor(marker));
    }

    pub(super) fn discard_link_cursor(
        &mut self,
        marker: &str,
        entry: Option<&str>,
        execution: Option<&mut crate::mandoc::inline::InlineExecutionState>,
    ) {
        super::super::man_links::remove_link_cursor(&mut self.nodes, marker, entry, execution);
    }

    pub(super) fn finish_output_membership(
        &mut self,
        marker: &str,
        started: &mut bool,
        membership: crate::mandoc::inline::ownership::OutputMembership,
        execution: Option<&mut crate::mandoc::inline::InlineExecutionState>,
    ) {
        super::super::man_links::finish_inline_membership(
            &mut self.nodes,
            marker,
            started,
            membership,
            execution,
        );
    }

    pub(super) fn wrap_first_link(
        &mut self,
        target: &mant_ir::LinkTarget,
        marker: &str,
        started: &mut super::super::man_links::LabelRange,
        membership: crate::mandoc::inline::ownership::OutputMembership,
        execution: Option<&mut crate::mandoc::inline::InlineExecutionState>,
    ) -> bool {
        super::super::man_links::wrap_first_visible_inline(
            &mut self.nodes,
            target,
            marker,
            started,
            membership,
            execution,
        )
    }

    pub(super) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            source: None,
            hanging_first_line: None,
        }
    }

    pub(super) fn remember_hanging_first_line(&mut self, origin: SourceIndent) {
        if crate::mandoc::inline::ends_with_executed_line_break(&self.nodes) {
            self.hanging_first_line = Some(origin);
        }
    }

    pub(super) fn retains_hanging_first_line(&self) -> bool {
        self.hanging_first_line.is_some()
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
        self.hanging_first_line = None;
        (std::mem::take(&mut self.nodes), self.source.take())
    }

    pub(super) fn adopt_active_output(&mut self, nodes: Vec<Inline>, source: Option<SourceSpan>) {
        assert_eq!(self.nodes.len(), 0);
        self.nodes = nodes;
        self.source = source;
    }

    /// The detached definition HEAD already represents its pending native
    /// row. Drop that row at the moment a real BODY break closes it, while
    /// retaining target anchors that identify the following visible word.
    pub(super) fn consume_invisible_head_row(&mut self, formatter: &mut FormatterState) -> bool {
        self.with_inline_builder(formatter, InlineBuilder::consume_invisible_head_row)
    }

    pub(super) fn hard_break(&mut self, formatter: &mut FormatterState) {
        self.with_inline_builder(formatter, InlineBuilder::hard_break);
    }

    #[cfg(test)]
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
    ) -> Option<crate::mandoc::inline::RunInBoundaryReceipt> {
        self.with_inline_builder(formatter, |builder| {
            builder.append_run_in_cells(count, native_count, generated_word);
            (count > 0 && generated_word)
                .then(|| builder.observe_run_in_boundary())
                .flatten()
        })
    }

    pub(super) fn append(
        &mut self,
        formatter: &mut FormatterState,
        source: Option<SourceSpan>,
        continues_line: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        // The definition BODY's row decision reads TERMP_NONEWLINE as it
        // stands at this word's source-line entry, before the word's own
        // term_word() clears it (term.c:588 with mdoc_term.c:314-317).
        formatter.note_definition_source_line();
        let changed = self.with_inline_builder(formatter, |builder| {
            append_filled_fragment(builder, continues_line, append)
        });
        if changed && self.source.is_none() {
            self.source = source;
        }
    }

    /// Drain one IR segment while the formatter retains the execution state.
    pub(super) fn take(
        &mut self,
        formatter: &mut FormatterState,
        indent: SourceIndent,
        continuation_indent: i32,
        hanging_origin: Option<SourceIndent>,
        drain: ParagraphDrain,
    ) -> (
        Option<Block>,
        bool,
        Vec<crate::mandoc::inline::CompletedRowOrigin>,
        crate::mandoc::inline::OutputRowEnd,
    ) {
        let (mut children, empty_word_end_break, completed_vertical_rows, closed_graph_tail) = self
            .with_inline_builder(formatter, |builder| {
                if drain == ParagraphDrain::AfterNativeFlush {
                    return builder.take_closed_paragraph_projection(true);
                }
                if drain == ParagraphDrain::BeforeNativeFlush
                    && let Some(retained) = builder.take_paragraph_before_native_flush()
                {
                    return retained;
                }
                builder.take_paragraph_segment(drain == ParagraphDrain::LineRequest)
            });
        let mut inline_layout = crate::mandoc::inline::take_inline_layout(&mut children);
        let first_line = self.hanging_first_line.take();
        let owner_origin = first_line.unwrap_or(indent);
        let mut layout = layout(owner_origin);
        if first_line.is_some() && drain != ParagraphDrain::AfterNativeFlush {
            // Only now does a new output owner use the hanging origin.
            // This paragraph's accepted hints keep the original base.
            formatter
                .execution
                .set_man_hanging_reading_origin(indent.physical_basic_units(), false);
        }
        if let Some(origin) = first_line.map(|_| indent).or(hanging_origin) {
            // BRIND's accepted hard rows can carry explicit native offsets.
            // Resolve against the continuation once, for the whole owner;
            // an unhinted continuation row inherits the hanging origin.
            let continuation = origin.offset_from(owner_origin);
            for hint in &mut inline_layout.row_hints {
                if hint.row > 0 {
                    hint.indent_columns = hint.indent_columns.saturating_sub(continuation);
                }
            }
            inline_layout.row_hints.retain(|hint| !hint.is_empty());
            layout.continuation_indent_columns = continuation;
        } else if continuation_indent != 0 {
            // Nm BODY and HEAD execute in one owner. Its resolved BODY
            // origin governs visual wraps, while accepted hard-row origins
            // already use the old owner base. Rebase those facts once;
            // changing the paragraph continuation cannot indent them twice.
            inline_layout.row_hints = (0..mant_ir::logical_row_count(&children))
                .map(|row| {
                    let mut hint = inline_layout.row_layout(row);
                    hint.row = u32::try_from(row).unwrap_or(u32::MAX);
                    if row > 0 {
                        hint.indent_columns =
                            hint.indent_columns.saturating_sub(continuation_indent);
                    }
                    hint
                })
                .filter(|hint| !hint.is_empty())
                .collect();
            layout.continuation_indent_columns = continuation_indent;
        }
        let source = self.source.take();
        (
            (!children.is_empty()).then_some(Block::Paragraph {
                inline_layout,
                children,
                layout,
                source,
            }),
            empty_word_end_break,
            completed_vertical_rows,
            closed_graph_tail,
        )
    }
}

/// A filled word executes identically when its still-open physical row has
/// literal presentation. The destination does not choose another executor.
pub(super) fn append_filled_fragment(
    builder: &mut InlineBuilder,
    continues_line: bool,
    append: impl FnOnce(&mut InlineBuilder),
) -> bool {
    // Filled NODE_LINE observes tab/source state but does not rewrite the
    // next word's NOSPACE/NONOSPACE decision (mdoc_term.c:318-322,
    // term.c:573-589). Ordinary source words use the same word entry as
    // macro operands, including the one released boundary before Sm off.
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
