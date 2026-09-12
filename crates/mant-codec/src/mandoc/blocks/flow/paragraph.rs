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
        let boundary = if self.builder.has_tight_boundary() || !crossed_source_line {
            FilledBoundary::SameLine
        } else if starts_indented_line {
            FilledBoundary::LineBreak
        } else {
            FilledBoundary::Word
        };
        if boundary == FilledBoundary::LineBreak {
            self.builder.hard_break();
        } else if boundary == FilledBoundary::Word && ordinary_text {
            self.builder.preserve_source_word_boundary();
        }
        let previous_count = self.builder.node_count();
        self.builder.begin_source_fragment();
        append(&mut self.builder);
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
    ) -> Option<Block> {
        let previous = std::mem::replace(self, Self::new(spacing));
        let children = previous.builder.finish();
        (!children.is_empty()).then(|| Block::Paragraph {
            children,
            layout: layout(indent),
            source: previous.source,
        })
    }
}
