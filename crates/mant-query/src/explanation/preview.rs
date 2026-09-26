//! Bounded windows over the same safe original projection used by collection.
use mant_ir::Block;
use mant_protocol::ExplanationPreview;
use std::ops::Range;

pub(super) struct LiteralHit<'a> {
    pub block: &'a Block,
    /// Original definition-list item/term when the literal lives in a
    /// non-semantic HEAD rather than in a descendant text block.
    pub term: Option<(usize, usize)>,
    pub path: String,
    pub range: Range<usize>,
}
impl LiteralHit<'_> {
    fn source(&self) -> Option<mant_ir::SourceSpan> {
        match self.term {
            None => mant_ir::geometry::block_source(self.block),
            Some((item, _)) => {
                let Block::DefinitionList { items, .. } = self.block else {
                    unreachable!("term hit belongs to a definition list")
                };
                items[item].source
            }
        }
    }

    pub(super) fn text(&self, content: mant_ir::ContentContext<'_>) -> String {
        match self.term {
            None => super::literal::block_text(content, self.block).expect("matched text block"),
            Some((item, term)) => {
                let Block::DefinitionList { items, .. } = self.block else {
                    unreachable!("term hit belongs to a definition list")
                };
                mant_protocol::ExplanationTextRoot::Inline(&items[item].terms[term])
                    .safe_text(content)
                    .expect("document term resolves in its content store")
            }
        }
    }

    pub(super) fn preview(&self, content: mant_ir::ContentContext<'_>) -> ExplanationPreview {
        let text = self.text(content);
        let match_start = text[..self.range.start].chars().count();
        let match_len = text[self.range.clone()].chars().count();
        let total = text.chars().count();
        let start = match_start.saturating_sub((1024 - match_len) / 2);
        let end = total.min(start + 1024);
        ExplanationPreview {
            block_path: self.path.clone(),
            source: self.source(),
            text: text.chars().skip(start).take(end - start).collect(),
            match_start_char: u32::try_from(match_start - start).expect("bounded preview"),
            match_end_char: u32::try_from(match_start - start + match_len)
                .expect("bounded preview"),
            content_ranges: Vec::new(),
            clipped_before: start > 0,
            clipped_after: end < total,
        }
    }
}
