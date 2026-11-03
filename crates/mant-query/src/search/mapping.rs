//! Map canonical Markdown bytes to visible text and presentation coordinates.
use mant_codec::markdown_mapping::{InlineMappingKind, map_inline_characters};
use mant_protocol::SearchScope;
use pulldown_cmark::{Event, TagEnd};
use std::{borrow::Cow, cell::OnceCell, ops::Range};

#[cfg(test)]
pub(super) fn display_markdown_line(line: &str) -> String {
    LineIndex::new(line).presented_line(0).text.to_string()
}

pub(super) struct AnchorStrippedLine<'a> {
    pub(super) text: Cow<'a, str>,
    segments: Vec<OffsetSegment>,
}

impl<'a> AnchorStrippedLine<'a> {
    pub(super) fn new(line: &'a str, mut hidden: impl Iterator<Item = Range<usize>>) -> Self {
        let Some(first) = hidden.next() else {
            return Self {
                text: Cow::Borrowed(line),
                segments: vec![OffsetSegment {
                    visible: 0..line.len(),
                    markdown: 0..line.len(),
                    linear: true,
                }],
            };
        };
        let mut text = String::with_capacity(line.len());
        let mut segments = Vec::new();
        let mut cursor = 0;
        for range in std::iter::once(first).chain(hidden) {
            push_retained_line_segment(
                line,
                cursor..range.start.min(line.len()),
                &mut text,
                &mut segments,
            );
            cursor = range.end.min(line.len());
        }
        push_retained_line_segment(line, cursor..line.len(), &mut text, &mut segments);
        #[cfg(test)]
        work::record(|work| work.presentation_bytes_copied += text.len());
        Self {
            text: Cow::Owned(text),
            segments,
        }
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

type ScalarColumnIndex = OnceCell<Box<[(usize, usize)]>>;

pub(super) struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
    trimmed_ends: Vec<usize>,
    anchors: Vec<Range<usize>>,
    presented: Vec<OnceCell<Box<AnchorStrippedLine<'a>>>>,
    scalar_columns: Vec<ScalarColumnIndex>,
}

impl<'a> LineIndex<'a> {
    #[cfg(test)]
    pub(super) fn new(text: &'a str) -> Self {
        Self::with_anchors(text, Vec::new())
    }

    pub(super) fn with_anchors(text: &'a str, anchors: Vec<Range<usize>>) -> Self {
        #[cfg(test)]
        work::record(|work| work.line_index_bytes += text.len());
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        let trimmed_ends = starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let end = starts.get(index + 1).copied().unwrap_or(text.len());
                start + text[*start..end].trim_end().len()
            })
            .collect();
        Self {
            text,
            presented: (0..starts.len()).map(|_| OnceCell::new()).collect(),
            scalar_columns: (0..starts.len()).map(|_| OnceCell::new()).collect(),
            starts,
            trimmed_ends,
            anchors,
        }
    }

    pub(super) fn presented_line(&self, index: usize) -> &AnchorStrippedLine<'a> {
        self.presented[index].get_or_init(|| {
            #[cfg(test)]
            work::record(|work| work.presentation_builds += 1);
            let start = self.start(index);
            let end = self.trimmed_ends[index];
            let first = self.anchors.partition_point(|range| range.end <= start);
            let hidden = self.anchors[first..]
                .iter()
                .take_while(|range| range.start < end)
                .map(|range| range.start.saturating_sub(start)..range.end.saturating_sub(start));
            Box::new(AnchorStrippedLine::new(&self.text[start..end], hidden))
        })
    }

    /// Test whether a match has any presentation without copying or mapping a line.
    pub(super) fn has_presented_bytes(
        &self,
        source: Range<usize>,
        start_line: usize,
        end_line: usize,
    ) -> bool {
        #[cfg(test)]
        work::record(|work| work.validity_checks += 1);
        (start_line..=end_line).any(|index| {
            #[cfg(test)]
            work::record(|work| work.validity_line_visits += 1);
            let start = source.start.max(self.starts[index]);
            let end = source.end.min(self.trimmed_ends[index]);
            if start >= end {
                return false;
            }
            let first = self.anchors.partition_point(|range| range.end <= start);
            let mut cursor = start;
            for anchor in self.anchors[first..]
                .iter()
                .take_while(|anchor| anchor.start < end)
            {
                if cursor < anchor.start {
                    return true;
                }
                cursor = cursor.max(anchor.end);
            }
            cursor < end
        })
    }

    pub(super) fn count(&self) -> usize {
        self.starts.len()
    }

    pub(super) fn position(&self, offset: usize) -> TextPosition {
        let offset = offset.min(self.text.len());
        let line_index = self.starts.partition_point(|start| *start <= offset) - 1;
        let line_start = self.starts[line_index];
        let columns = self.scalar_columns[line_index].get_or_init(|| {
            let line = self.line(line_index);
            #[cfg(test)]
            work::record(|work| work.scalar_index_bytes += line.len());
            let mut extra_bytes = 0;
            line.char_indices()
                .filter_map(|(byte, character)| {
                    let len = character.len_utf8();
                    extra_bytes += len - 1;
                    (len > 1).then_some((byte + len, extra_bytes))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice()
        });
        let relative = offset - line_start;
        let index = columns.partition_point(|(end, _)| *end <= relative);
        let extra_bytes = index.checked_sub(1).map_or(0, |index| columns[index].1);
        TextPosition {
            line_index,
            column: relative.saturating_sub(extra_bytes).saturating_add(1),
        }
    }

    pub(super) fn line_index_at_byte(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset) - 1
    }

    pub(super) fn line(&self, line_index: usize) -> &'a str {
        let start = self.starts[line_index];
        let end = self
            .starts
            .get(line_index + 1)
            .copied()
            .unwrap_or(self.text.len());
        self.text[start..end]
            .strip_suffix('\n')
            .unwrap_or(&self.text[start..end])
    }

    pub(super) fn trimmed_line(&self, line_index: usize) -> &'a str {
        &self.text[self.starts[line_index]..self.trimmed_ends[line_index]]
    }

    pub(super) fn start(&self, line_index: usize) -> usize {
        self.starts[line_index]
    }
}

/// Per-thread counters keep workload assertions independent of elapsed time and
/// concurrent tests. Only an explicitly measured search contributes counters.
#[cfg(test)]
pub(super) mod work {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default)]
    pub(in crate::search) struct Work {
        pub(in crate::search) line_index_bytes: usize,
        pub(in crate::search) validity_checks: usize,
        pub(in crate::search) validity_line_visits: usize,
        pub(in crate::search) range_mappings: usize,
        pub(in crate::search) presentation_builds: usize,
        pub(in crate::search) presentation_bytes_copied: usize,
        pub(in crate::search) scalar_index_bytes: usize,
    }

    thread_local! {
        static CURRENT: Cell<Option<Work>> = const { Cell::new(None) };
    }

    pub(in crate::search) fn record(update: impl FnOnce(&mut Work)) {
        CURRENT.with(|current| {
            if let Some(mut work) = current.get() {
                update(&mut work);
                current.set(Some(work));
            }
        });
    }

    pub(in crate::search) fn measure<T>(run: impl FnOnce() -> T) -> (T, Work) {
        struct Restore(Option<Work>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CURRENT.set(self.0);
            }
        }
        let restore = Restore(CURRENT.replace(Some(Work::default())));
        let result = run();
        let work = CURRENT.get().expect("measuring search work");
        drop(restore);
        (result, work)
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
        for (event, source) in mant_codec::markdown_mapping::markdown_source_events(markdown) {
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
