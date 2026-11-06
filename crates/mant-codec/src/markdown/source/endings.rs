//! Physical source line endings, before entity decoding or semantic lowering.
use super::MarkdownSource;
use mant_ir::{
    Block, Document, EntryFacts, Inline, SourceSpan, ValueDomain,
    visit::{self, VisitMut},
};
use std::{borrow::Cow, ops::Range};

/// LF is the parser input spelling; sparse removed-byte checkpoints recover
/// original UTF-8 byte boundaries. A CRLF is one physical line, not two spaces
/// in a code span (`CommonMark` 0.31.2, characters-and-lines / code-spans).
pub(in crate::markdown) struct LineEndings<'a> {
    pub(in crate::markdown) text: Cow<'a, str>,
    removed: Vec<usize>,
}

impl<'a> LineEndings<'a> {
    pub(in crate::markdown) fn new(source: &'a str) -> Self {
        if !source.contains('\r') {
            return Self {
                text: Cow::Borrowed(source),
                removed: Vec::new(),
            };
        }
        let mut text = String::with_capacity(source.len());
        let mut removed = Vec::new();
        let mut rest = source;
        while let Some(position) = rest.find('\r') {
            text.push_str(&rest[..position]);
            text.push('\n');
            rest = &rest[position + 1..];
            if let Some(next) = rest.strip_prefix('\n') {
                removed.push(text.len());
                rest = next;
            }
        }
        text.push_str(rest);
        Self {
            text: Cow::Owned(text),
            removed,
        }
    }

    pub(in crate::markdown) fn range(&self, range: Range<usize>) -> Range<usize> {
        let offset =
            |at: usize| at.saturating_add(self.removed.partition_point(|removed| *removed <= at));
        offset(range.start)..offset(range.end)
    }

    pub(in crate::markdown) fn restore_sources(&self, document: &mut Document, original: &str) {
        if self.removed.is_empty() {
            return;
        }
        let mut restore = RestoreSources {
            endings: self,
            source: MarkdownSource::new(original),
        };
        restore.visit_document_mut(document);
        for diagnostic in &mut document.diagnostics {
            restore.span(&mut diagnostic.source);
        }
    }
}

struct RestoreSources<'a, 's> {
    endings: &'a LineEndings<'s>,
    source: MarkdownSource<'s>,
}
impl RestoreSources<'_, '_> {
    fn span(&self, span: &mut Option<SourceSpan>) {
        if let Some(value) = span
            && let Some(range) = value.byte_range
        {
            *value = self.source.span(
                &self
                    .endings
                    .range(range.start.get() as usize..range.end.get() as usize),
            );
        }
    }
    fn entry(&self, entry: &mut Option<EntryFacts>) {
        if let Some(entry) = entry
            && let Some(ValueDomain::EntrySet { source, .. }) = &mut entry.value_domain
        {
            self.span(source);
        }
    }
}
impl VisitMut for RestoreSources<'_, '_> {
    fn visit_heading_mut(&mut self, heading: &mut mant_ir::Heading) {
        self.span(&mut heading.source);
        visit::walk_heading_mut(self, heading);
    }
    fn visit_section_mut(&mut self, section: &mut mant_ir::Section) {
        self.span(&mut section.source);
        visit::walk_section_mut(self, section);
    }
    fn visit_block_mut(&mut self, block: &mut Block) {
        let (Block::Paragraph { source, .. }
        | Block::Preformatted { source, .. }
        | Block::List { source, .. }
        | Block::DefinitionList { source, .. }
        | Block::Table { source, .. }
        | Block::Equation { source, .. }
        | Block::VerticalSpace { source, .. }
        | Block::ThematicBreak { source }
        | Block::Unsupported { source, .. }) = block;
        self.span(source);
        visit::walk_block_mut(self, block);
    }
    fn visit_list_item_mut(&mut self, item: &mut mant_ir::ListItem) {
        self.span(&mut item.source);
        self.entry(&mut item.entry);
        visit::walk_list_item_mut(self, item);
    }
    fn visit_definition_item_mut(&mut self, item: &mut mant_ir::DefinitionItem) {
        self.span(&mut item.source);
        self.entry(&mut item.entry);
        visit::walk_definition_item_mut(self, item);
    }
    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        if let Inline::Anchor { owner_source, .. } = inline {
            self.span(owner_source);
        }
        visit::walk_inline_mut(self, inline);
    }
}
