//! Filled content and its first/last source positions are one reset lifetime.
use super::{FilledBoundary, InlineBuilder, layout};
use mant_ir::{Block, SourceSpan};

pub(super) struct ParagraphFlow {
    builder: InlineBuilder,
    source: Option<SourceSpan>,
    last_line: Option<u32>,
}

impl ParagraphFlow {
    pub(super) const fn new(spacing: bool) -> Self {
        Self {
            builder: InlineBuilder::with_spacing(spacing),
            source: None,
            last_line: None,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.builder.is_empty()
    }
    pub(super) fn set_spacing(&mut self, setting: &str) {
        self.builder.set_spacing(setting);
    }
    pub(super) fn inherit_spacing(&mut self, spacing: bool) {
        self.builder.inherit_spacing(spacing);
    }
    pub(super) fn hard_break(&mut self) {
        self.builder.hard_break();
    }
    pub(super) fn tighten_next_boundary(&mut self) {
        self.builder.tighten_next_boundary();
    }
    pub(super) fn no_break_flush(&mut self) {
        self.builder.no_break_flush();
    }
    pub(super) fn has_formatter_cell(&self) -> bool {
        self.builder.has_formatter_cell()
    }

    pub(super) fn resolve_vertical_space(&mut self, rows: i32) -> u16 {
        self.builder.resolve_vertical_space(rows)
    }

    pub(super) fn inherit_vertical_space_debt(&mut self, debt: u16) {
        self.builder.inherit_vertical_space_debt(debt);
    }

    pub(super) const fn vertical_space_debt(&self) -> u16 {
        self.builder.vertical_space_debt()
    }

    pub(super) fn inherit_zero_advance_armed(&mut self, armed: bool) {
        self.builder.inherit_zero_advance_armed(armed);
    }

    pub(super) fn inherit_preserved_execution(
        &mut self,
        state: crate::mandoc::inline::PreservedInlineState,
    ) {
        self.last_line = state.last_executed_source_line;
        self.builder.inherit_preserved_execution(state);
    }

    pub(super) fn append_run_in_cells(&mut self, count: usize) {
        self.builder.append_run_in_cells(count);
    }

    pub(super) fn take_zero_advance_armed(&mut self) -> bool {
        self.builder.take_zero_advance_armed()
    }

    pub(super) fn append(
        &mut self,
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
            self.last_line.is_some() || self.builder.has_formatter_cell();
        let source_continues = self.builder.final_source_continuation_or(false);
        let boundary = if source_continues {
            FilledBoundary::SameLine
        } else if starts_indented_line && has_executed_predecessor {
            // In CVS print_mdoc_node(), a source line beginning with blank
            // text calls term_newln() before TERMP_NOSPACE is applied to the
            // next word. NODE_LINE is execution evidence even when macro
            // expansion gives both rows the same authored source coordinate;
            // generated inset/diagnostic cells and `.Ns` cannot erase it.
            FilledBoundary::LineBreak
        } else if self.builder.has_tight_boundary() || !crossed_source_line {
            FilledBoundary::SameLine
        } else {
            FilledBoundary::Word
        };
        if boundary == FilledBoundary::LineBreak {
            self.builder.hard_break();
        } else if boundary == FilledBoundary::Word && ordinary_text {
            self.builder.preserve_source_word_boundary();
        }
        if source_continues && !self.builder.has_tight_boundary() {
            self.builder.preserve_continued_boundary();
        }
        let previous_count = self.builder.node_count();
        let fragment = self.builder.begin_source_fragment();
        append(&mut self.builder);
        self.builder.finish_source_fragment(fragment);
        if self.builder.final_word_join_or(continues_line) {
            self.builder.tighten_next_boundary();
        }
        if self.builder.node_count() != previous_count {
            if self.source.is_none() {
                self.source = source;
            }
            if source_line.is_some() {
                self.last_line = source_line;
            }
        }
    }

    /// Detach content and reset provenance and pending source-line state together.
    pub(super) fn take(
        &mut self,
        indent: crate::mandoc::layout::SourceIndent,
        spacing: bool,
    ) -> (Option<Block>, bool) {
        self.take_with(indent, spacing, false)
    }

    pub(super) fn take_for_vertical_request(
        &mut self,
        indent: crate::mandoc::layout::SourceIndent,
        spacing: bool,
    ) -> (Option<Block>, bool) {
        self.take_with(indent, spacing, true)
    }

    fn take_with(
        &mut self,
        indent: crate::mandoc::layout::SourceIndent,
        spacing: bool,
        vertical_request: bool,
    ) -> (Option<Block>, bool) {
        let mut next = Self::new(spacing);
        let invisible_formatter_cell = self.builder.has_invisible_formatter_cell();
        if vertical_request {
            self.builder
                .transfer_vertical_request_execution(&mut next.builder);
        } else {
            self.builder.transfer_container_execution(&mut next.builder);
        }
        // The pending break makes this an active native cell. Transfer any
        // bare `\z` decision before extracting the otherwise unrepresentable
        // leading break, so the two effects remain ordered atomically.
        let empty_word_end_break = self.builder.take_unrepresented_word_end_break();
        let previous = std::mem::replace(self, next);
        let mut children = previous.builder.finish();
        if invisible_formatter_cell
            && !empty_word_end_break
            && !mant_ir::has_printable_character(&children)
        {
            children.push(mant_ir::Inline::Text {
                value: String::new(),
            });
        }
        (
            (!children.is_empty()).then(|| Block::Paragraph {
                children,
                layout: layout(indent),
                source: previous.source,
            }),
            empty_word_end_break,
        )
    }
}
