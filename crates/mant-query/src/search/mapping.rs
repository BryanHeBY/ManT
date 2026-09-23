//! Map canonical Markdown bytes to visible text and presentation coordinates.
use mant_codec::markdown_mapping::{InlineMappingKind, map_inline_characters};
use mant_protocol::SearchScope;
use pulldown_cmark::{Event, Parser, TagEnd};
use std::cell::Cell;
use std::ops::Range;

pub(super) struct AnchorStrippedLine {
    pub(super) text: String,
}

impl AnchorStrippedLine {
    pub(super) fn new(line: &str, hidden: impl Iterator<Item = Range<usize>>) -> Self {
        let mut text = String::with_capacity(line.len());
        let mut cursor = 0;
        for range in hidden {
            let start = range.start.min(line.len());
            if cursor < start {
                text.push_str(&line[cursor..start]);
            }
            cursor = cursor.max(range.end.min(line.len()));
        }
        text.push_str(&line[cursor..]);
        Self { text }
    }
}

pub(super) struct TextPosition {
    pub(super) line_index: usize,
    pub(super) column: usize,
}

pub(super) struct LineIndex {
    starts: Vec<usize>,
    // Final presented end per line. Trimming a long whitespace suffix for
    // every hit on the same line would make count-only scans quadratic.
    presented_ends: Vec<usize>,
    anchors: Vec<Range<usize>>,
    // Search hits are visited in source order. The cursor makes repeated
    // Unicode columns on one long line amortized linear, without a per-scalar
    // side index for the entire document.
    column_cursor: Cell<(usize, usize, usize)>,
}

impl LineIndex {
    pub(super) fn with_anchors(text: &str, anchors: Vec<Range<usize>>) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        let presented_ends = starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let end = starts.get(index + 1).copied().unwrap_or(text.len());
                let line = text[*start..end]
                    .strip_suffix('\n')
                    .unwrap_or(&text[*start..end]);
                *start + line.trim_end().len()
            })
            .collect();
        Self {
            starts,
            presented_ends,
            anchors,
            column_cursor: Cell::new((0, 0, 0)),
        }
    }

    pub(super) fn presented_line(&self, text: &str, index: usize) -> AnchorStrippedLine {
        let start = self.start(index);
        let line = &text[start..self.presented_end(index)];
        let first = self.anchors.partition_point(|range| range.end <= start);
        let hidden = self.anchors[first..]
            .iter()
            .take_while(|range| range.start < start + line.len())
            .map(|range| range.start.saturating_sub(start)..range.end.saturating_sub(start));
        AnchorStrippedLine::new(line, hidden)
    }

    /// Determine whether a source range retains any presented bytes without
    /// constructing a full line or its exact highlight fragments.
    pub(super) fn has_presented_range(&self, line_index: usize, source: Range<usize>) -> bool {
        let line_start = self.start(line_index);
        let line_end = self.presented_end(line_index);
        let start = (line_start + source.start).min(line_end);
        let end = (line_start + source.end).min(line_end);
        if start >= end {
            return false;
        }
        let mut cursor = start;
        let first = self.anchors.partition_point(|range| range.end <= start);
        for hidden in self.anchors[first..]
            .iter()
            .take_while(|range| range.start < end)
        {
            if cursor < hidden.start {
                return true;
            }
            cursor = cursor.max(hidden.end);
        }
        cursor < end
    }

    pub(super) fn count(&self) -> usize {
        self.starts.len()
    }

    pub(super) fn position(&self, text: &str, offset: usize) -> TextPosition {
        let offset = offset.min(text.len());
        let line_index = self.starts.partition_point(|start| *start <= offset) - 1;
        let line_start = self.starts[line_index];
        let (old_line, old_byte, old_scalars) = self.column_cursor.get();
        let (begin, prior_scalars) = if old_line == line_index && old_byte <= offset {
            (old_byte, old_scalars)
        } else {
            (line_start, 0)
        };
        let scalars = prior_scalars.saturating_add(text[begin..offset].chars().count());
        self.column_cursor.set((line_index, offset, scalars));
        TextPosition {
            line_index,
            column: scalars.saturating_add(1),
        }
    }

    pub(super) fn line_index_at_byte(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset) - 1
    }

    pub(super) fn start(&self, line_index: usize) -> usize {
        self.starts[line_index]
    }

    pub(super) fn presented_end(&self, line_index: usize) -> usize {
        self.presented_ends[line_index]
    }
}

pub(super) struct SearchableText {
    pub(super) text: String,
    segments: Vec<OffsetSegment>,
    pub(super) direct_markdown: bool,
}

#[derive(Debug)]
struct OffsetSegment {
    visible: Range<usize>,
    markdown: Range<usize>,
    linear: bool,
    separator: bool,
}

pub(super) struct VisiblePart<'a> {
    pub(super) text: &'a str,
    pub(super) separator: bool,
    pub(super) markdown: Option<Range<usize>>,
    pub(super) visible: Range<usize>,
}

impl SearchableText {
    pub(super) fn is_separator_at(&self, offset: usize) -> bool {
        self.segment_at(offset)
            .is_some_and(|segment| segment.separator)
    }

    pub(super) fn visible_start_for_markdown(&self, markdown_start: usize) -> Option<usize> {
        self.segments
            .iter()
            .find(|segment| {
                segment.markdown.end > markdown_start
                    || (segment.markdown.is_empty() && segment.markdown.start == markdown_start)
            })
            .map(|segment| segment.visible.start)
    }

    /// Retained canonical-visible parts, with synthetic breaks kept separate.
    pub(super) fn parts(&self, range: Range<usize>) -> Vec<VisiblePart<'_>> {
        let first = self
            .segments
            .partition_point(|segment| segment.visible.end <= range.start);
        self.segments[first..]
            .iter()
            .take_while(|segment| segment.visible.start < range.end)
            .filter_map(|segment| {
                let start = range.start.max(segment.visible.start);
                let end = range.end.min(segment.visible.end);
                (start < end).then(|| VisiblePart {
                    text: &self.text[start..end],
                    separator: segment.separator,
                    markdown: segment.linear.then(|| {
                        let source_start = segment.markdown.start + start - segment.visible.start;
                        source_start..source_start + end - start
                    }),
                    visible: start..end,
                })
            })
            .collect()
    }
    pub(super) fn new(markdown: &str, scope: SearchScope) -> Self {
        if scope == SearchScope::Markdown {
            return Self {
                text: markdown.to_owned(),
                segments: Vec::new(),
                direct_markdown: true,
            };
        }

        let mut visible = VisibleBuilder::new(markdown);
        for (event, source) in Parser::new(markdown).into_offset_iter() {
            match event {
                Event::Text(value) | Event::InlineMath(value) | Event::DisplayMath(value) => {
                    visible.push_mapped(&value, source, InlineMappingKind::Text);
                }
                Event::Code(value) => {
                    visible.push_mapped(&value, source, InlineMappingKind::Code);
                }
                Event::SoftBreak | Event::HardBreak | Event::Rule => visible.push_break(source),
                Event::End(
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::Item
                    | TagEnd::CodeBlock
                    | TagEnd::TableRow,
                ) => visible.push_break(source.end..source.end),
                Event::Start(_)
                | Event::End(_)
                | Event::Html(_)
                | Event::InlineHtml(_)
                | Event::FootnoteReference(_)
                | Event::TaskListMarker(_) => {}
            }
        }
        visible.finish()
    }

    pub(super) fn markdown_start(&self, offset: usize) -> usize {
        if self.direct_markdown {
            return offset;
        }
        self.segment_at(offset).map_or(0, |segment| {
            if segment.linear {
                segment.markdown.start + offset.saturating_sub(segment.visible.start)
            } else {
                segment.markdown.start
            }
        })
    }

    pub(super) fn markdown_end(&self, offset: usize) -> usize {
        if self.direct_markdown {
            return offset;
        }
        if offset == 0 {
            return 0;
        }
        self.segment_at(offset - 1).map_or(0, |segment| {
            if segment.linear {
                segment.markdown.start + offset.saturating_sub(segment.visible.start)
            } else {
                segment.markdown.end
            }
        })
    }

    fn segment_at(&self, offset: usize) -> Option<&OffsetSegment> {
        let index = self
            .segments
            .partition_point(|segment| segment.visible.end <= offset);
        self.segments
            .get(index)
            .filter(|segment| segment.visible.contains(&offset))
    }
}

struct VisibleBuilder<'a> {
    markdown: &'a str,
    text: String,
    segments: Vec<OffsetSegment>,
}

impl<'a> VisibleBuilder<'a> {
    pub(super) fn new(markdown: &'a str) -> Self {
        Self {
            markdown,
            text: String::new(),
            segments: Vec::new(),
        }
    }

    pub(super) fn push_mapped(
        &mut self,
        value: &str,
        source: Range<usize>,
        kind: InlineMappingKind,
    ) {
        for mapped in map_inline_characters(self.markdown, value, source, kind) {
            let visible_start = self.text.len();
            self.text.push(mapped.value);
            let visible_end = self.text.len();
            self.push_segment(OffsetSegment {
                visible: visible_start..visible_end,
                markdown: mapped.source,
                linear: mapped.linear,
                separator: false,
            });
        }
    }

    pub(super) fn push_break(&mut self, markdown: Range<usize>) {
        if self.text.ends_with('\n') || self.text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push('\n');
        self.push_segment(OffsetSegment {
            visible: start..self.text.len(),
            markdown,
            linear: false,
            separator: true,
        });
    }

    pub(super) fn push_segment(&mut self, segment: OffsetSegment) {
        if let Some(previous) = self.segments.last_mut() {
            let contiguous = previous.visible.end == segment.visible.start
                && previous.markdown.end == segment.markdown.start
                && previous.linear
                && segment.linear;
            if contiguous {
                previous.visible.end = segment.visible.end;
                previous.markdown.end = segment.markdown.end;
                return;
            }
        }
        self.segments.push(segment);
    }

    pub(super) fn finish(self) -> SearchableText {
        SearchableText {
            text: self.text,
            segments: self.segments,
            direct_markdown: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LineIndex;

    #[test]
    fn unicode_columns_remain_exact_across_forward_and_backward_offsets() {
        let text = "a界e\u{301}z\nβ🙂x";
        let lines = LineIndex::with_anchors(text, Vec::new());
        let offsets = [0, 1, 4, 5, 7, 8, 9, 11, 15, 16, 8, 7, 0, text.len()];
        for offset in offsets {
            let found = lines.position(text, offset);
            let line_start = lines.start(found.line_index);
            assert_eq!(
                found.column,
                text[line_start..offset].chars().count() + 1,
                "byte offset {offset}"
            );
        }
    }
}
