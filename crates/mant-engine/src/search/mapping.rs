//! Map canonical Markdown bytes to visible text and presentation coordinates.
use crate::markdown_mapping::{InlineMappingKind, map_inline_characters};
use mant_protocol::SearchScope;
use pulldown_cmark::{Event, Parser, TagEnd};
use std::ops::Range;

#[cfg(test)]
pub(super) fn display_markdown_line(line: &str) -> String {
    LineIndex::new(line).presented_line(line, 0).text
}

pub(super) struct AnchorStrippedLine {
    pub(super) text: String,
    segments: Vec<OffsetSegment>,
}

impl AnchorStrippedLine {
    pub(super) fn new(line: &str, hidden: impl Iterator<Item = Range<usize>>) -> Self {
        let mut text = String::with_capacity(line.len());
        let mut segments = Vec::new();
        let mut cursor = 0;
        for range in hidden {
            push_retained_line_segment(
                line,
                cursor..range.start.min(line.len()),
                &mut text,
                &mut segments,
            );
            cursor = range.end.min(line.len());
        }
        push_retained_line_segment(line, cursor..line.len(), &mut text, &mut segments);
        Self { text, segments }
    }

    pub(super) fn map_range(&self, source: Range<usize>) -> Vec<Range<usize>> {
        self.segments
            .iter()
            .filter_map(|segment| {
                let start = source.start.max(segment.markdown.start);
                let end = source.end.min(segment.markdown.end);
                (start < end).then(|| {
                    segment.visible.start + start.saturating_sub(segment.markdown.start)
                        ..segment.visible.start + end.saturating_sub(segment.markdown.start)
                })
            })
            .collect()
    }
}

fn push_retained_line_segment(
    line: &str,
    source: Range<usize>,
    text: &mut String,
    segments: &mut Vec<OffsetSegment>,
) {
    if source.is_empty() {
        return;
    }
    let visible_start = text.len();
    text.push_str(&line[source.clone()]);
    segments.push(OffsetSegment {
        visible: visible_start..text.len(),
        markdown: source,
        linear: true,
    });
}

pub(super) struct TextPosition {
    pub(super) line_index: usize,
    pub(super) column: usize,
}

pub(super) struct LineIndex {
    starts: Vec<usize>,
    anchors: Vec<Range<usize>>,
}

impl LineIndex {
    pub(super) fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self {
            starts,
            anchors: crate::output::anchor_markers(text)
                .into_iter()
                .map(|marker| marker.range)
                .collect(),
        }
    }

    pub(super) fn presented_line(&self, text: &str, index: usize) -> AnchorStrippedLine {
        let start = self.start(index);
        let line = self.line(text, index).trim_end();
        let first = self.anchors.partition_point(|range| range.end <= start);
        let hidden = self.anchors[first..]
            .iter()
            .take_while(|range| range.start < start + line.len())
            .map(|range| range.start.saturating_sub(start)..range.end.saturating_sub(start));
        AnchorStrippedLine::new(line, hidden)
    }

    pub(super) fn count(&self) -> usize {
        self.starts.len()
    }

    pub(super) fn position(&self, text: &str, offset: usize) -> TextPosition {
        let offset = offset.min(text.len());
        let line_index = self.starts.partition_point(|start| *start <= offset) - 1;
        let line_start = self.starts[line_index];
        TextPosition {
            line_index,
            column: text[line_start..offset].chars().count().saturating_add(1),
        }
    }

    pub(super) fn line_index_at_byte(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset) - 1
    }

    pub(super) fn line<'a>(&self, text: &'a str, line_index: usize) -> &'a str {
        let start = self.starts[line_index];
        let end = self
            .starts
            .get(line_index + 1)
            .copied()
            .unwrap_or(text.len());
        text[start..end]
            .strip_suffix('\n')
            .unwrap_or(&text[start..end])
    }

    pub(super) fn start(&self, line_index: usize) -> usize {
        self.starts[line_index]
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
}

impl SearchableText {
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
