//! Final cell-slot alignment consumes prepared rows and their coordinate maps.
use super::{Span, WrappedLine};
use mant_ir::TextAlignment;

#[derive(Clone, Copy, Debug)]
pub(super) struct PreparedAlignment {
    pub(super) placement: TextAlignment,
    pub(super) prefix: usize,
    pub(super) content: usize,
}

impl WrappedLine {
    pub(super) fn alignment_padding(&self, width: usize) -> Option<usize> {
        self.copy_map.alignment.map(|receipt| {
            receipt
                .placement
                .padding(width.saturating_sub(receipt.prefix), receipt.content)
        })
    }

    /// Returns whether an owner-local preference owns this row's alignment.
    /// Source cells were measured and placed before these generated cells.
    pub(super) fn position_alignment(&mut self, width: usize) -> bool {
        let Some(receipt) = self.copy_map.alignment.take() else {
            return false;
        };
        let padding = receipt
            .placement
            .padding(width.saturating_sub(receipt.prefix), receipt.content);
        if padding == 0 {
            return true;
        }
        insert_padding(&mut self.line.spans, receipt.prefix, padding);
        for link in &mut self.links {
            if link.start_column >= receipt.prefix {
                link.start_column += padding;
                link.end_column += padding;
            }
        }
        for cell in &mut self.search_cells {
            if cell.start_column >= receipt.prefix {
                cell.start_column += padding;
                cell.end_column += padding;
            }
        }
        for range in &mut self.copy_map.omitted {
            if range.start >= receipt.prefix {
                range.start += padding;
                range.end += padding;
            }
        }
        let index = self
            .copy_map
            .omitted
            .partition_point(|range| range.start < receipt.prefix);
        self.copy_map
            .omitted
            .insert(index, receipt.prefix..receipt.prefix + padding);
        if let Some(end) = &mut self.copy_map.end {
            *end += padding;
        }
        true
    }
}

fn insert_padding(spans: &mut Vec<Span<'static>>, prefix: usize, padding: usize) {
    let mut column = 0;
    let mut index = 0;
    while index < spans.len() {
        if column == prefix {
            break;
        }
        let width = super::super::inline::spans_width(std::slice::from_ref(&spans[index]));
        if column.saturating_add(width) <= prefix {
            column += width;
            index += 1;
            continue;
        }
        let mut at = 0;
        for glyph in mant_render::cells::graphemes(&spans[index].content) {
            if column >= prefix {
                break;
            }
            column += glyph.columns();
            at = glyph.bytes().end;
        }
        let span = spans.remove(index);
        let mut content = span.content.into_owned();
        let tail = content.split_off(at);
        spans.insert(index, Span::styled(content, span.style));
        spans.insert(index + 1, Span::styled(tail, span.style));
        index += 1;
        break;
    }
    spans.insert(index, Span::raw(" ".repeat(padding)));
}
