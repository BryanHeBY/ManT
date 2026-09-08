//! Bounded windows over the same safe original projection used by collection.
use mant_ir::Block;
use mant_protocol::ExplanationPreview;
use std::ops::Range;

pub(super) struct LiteralHit<'a> {
    pub block: &'a Block,
    pub path: String,
    pub range: Range<usize>,
}
impl LiteralHit<'_> {
    pub(super) fn preview(&self) -> ExplanationPreview {
        let text = super::literal::block_text(self.block).expect("matched text block");
        let match_start = text[..self.range.start].chars().count();
        let match_len = text[self.range.clone()].chars().count();
        let total = text.chars().count();
        let start = match_start.saturating_sub((1024 - match_len) / 2);
        let end = total.min(start + 1024);
        ExplanationPreview {
            block_path: self.path.clone(),
            source: crate::block::block_source(self.block),
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
