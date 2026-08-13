//! Projects logical text searches back onto exact terminal cell ranges.

use std::collections::{BTreeMap, HashMap};

use ratatui::{
    style::Modifier,
    text::{Line, Span, Text},
};
use unicode_width::UnicodeWidthChar;

use super::{RenderedDocument, WrappedLine};
use crate::theme;

/// One exact visual-row range found in a width-dependent document rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSearchMatch {
    /// First visual row intersecting the match.
    pub row: usize,
    /// Inclusive terminal-cell column on the first row.
    pub start_column: usize,
    /// Exclusive terminal-cell column on the first row.
    pub end_column: usize,
    pub(super) additional_fragments: Vec<RenderedSearchFragment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RenderedSearchFragment {
    pub(super) row: usize,
    pub(super) start_column: usize,
    pub(super) end_column: usize,
}

#[derive(Debug, Clone)]
pub(super) struct RenderedSearchRecord {
    pub(super) text: String,
    pub(super) cells: Vec<RenderedSearchSourceCell>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RenderedSearchSourceCell {
    pub(super) source_start: usize,
    pub(super) source_end: usize,
    pub(super) fragment: RenderedSearchFragment,
}

impl RenderedDocument {
    /// Search visible terminal rows without rebuilding or traversing the IR.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<RenderedSearchMatch> {
        let needle = query.to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.search_records
            .iter()
            .flat_map(|record| {
                let folded = fold_for_search(&record.text);
                folded
                    .value
                    .match_indices(&needle)
                    .filter_map(|(start, value)| {
                        let source_start = folded.starts[start];
                        let source_end = folded.ends[start + value.len() - 1];
                        search_match_for_range(record, source_start, source_end)
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Clone the rendered rows and decorate ordinary and active search hits.
    #[must_use]
    pub fn highlighted_text(
        &self,
        matches: &[RenderedSearchMatch],
        active: Option<usize>,
    ) -> Text<'static> {
        self.text_range(0, self.text.lines.len(), matches, active)
    }

    /// Clone and decorate only the rows visible in one terminal viewport.
    ///
    /// A rendered GCC manual can contain tens of thousands of rows. Cloning
    /// all of them for every mouse event defeats the width cache even when no
    /// reflow occurs, so the UI asks for this bounded projection instead.
    pub(crate) fn viewport_text(
        &self,
        start: usize,
        height: usize,
        matches: &[RenderedSearchMatch],
        active: Option<usize>,
    ) -> Text<'static> {
        let end = start.saturating_add(height).min(self.text.lines.len());
        self.text_range(start, end, matches, active)
    }

    fn text_range(
        &self,
        start: usize,
        end: usize,
        matches: &[RenderedSearchMatch],
        active: Option<usize>,
    ) -> Text<'static> {
        let Some(lines) = self.text.lines.get(start..end) else {
            return Text::default();
        };
        let mut text = Text::from(lines.to_vec());
        if matches.is_empty() {
            return text;
        }

        let mut by_row: HashMap<usize, Vec<(usize, RenderedSearchFragment)>> = HashMap::new();
        for (index, search_match) in matches.iter().enumerate() {
            let first = RenderedSearchFragment {
                row: search_match.row,
                start_column: search_match.start_column,
                end_column: search_match.end_column,
            };
            for fragment in
                std::iter::once(first).chain(search_match.additional_fragments.iter().copied())
            {
                if (start..end).contains(&fragment.row) {
                    by_row
                        .entry(fragment.row - start)
                        .or_default()
                        .push((index, fragment));
                }
            }
        }
        for (row, ranges) in by_row {
            let Some(line) = text.lines.get_mut(row) else {
                continue;
            };
            *line = highlight_line(line, &ranges, active);
        }
        text
    }
}

pub(super) fn search_records_for_lines(
    lines: &[WrappedLine],
    first_row: usize,
) -> Vec<RenderedSearchRecord> {
    #[derive(Default)]
    struct RecordBuilder {
        text: String,
        cells: Vec<RenderedSearchSourceCell>,
        last_line: Option<usize>,
    }

    let mut records: BTreeMap<usize, RecordBuilder> = BTreeMap::new();
    for (line_index, line) in lines.iter().enumerate() {
        for cell in &line.search_cells {
            let record = records.entry(cell.group).or_default();
            if record.last_line != Some(line_index) {
                if record.last_line.is_some() && cell.join_before {
                    record.text.push(' ');
                }
                record.last_line = Some(line_index);
            }
            let source_start = record.text.len();
            record.text.push(cell.character);
            record.cells.push(RenderedSearchSourceCell {
                source_start,
                source_end: record.text.len(),
                fragment: RenderedSearchFragment {
                    row: first_row + line_index,
                    start_column: cell.start_column,
                    end_column: cell.end_column,
                },
            });
        }
    }
    records
        .into_values()
        .filter_map(|record| {
            (!record.text.is_empty()).then_some(RenderedSearchRecord {
                text: record.text,
                cells: record.cells,
            })
        })
        .collect()
}

fn search_match_for_range(
    record: &RenderedSearchRecord,
    source_start: usize,
    source_end: usize,
) -> Option<RenderedSearchMatch> {
    let mut fragments: Vec<RenderedSearchFragment> = Vec::new();
    for cell in record
        .cells
        .iter()
        .filter(|cell| cell.source_start < source_end && cell.source_end > source_start)
    {
        if let Some(last) = fragments.last_mut()
            && last.row == cell.fragment.row
            && last.end_column == cell.fragment.start_column
        {
            last.end_column = cell.fragment.end_column;
        } else {
            fragments.push(cell.fragment);
        }
    }
    let first = fragments.first().copied()?;
    Some(RenderedSearchMatch {
        row: first.row,
        start_column: first.start_column,
        end_column: first.end_column,
        additional_fragments: fragments.into_iter().skip(1).collect(),
    })
}

struct FoldedText {
    value: String,
    starts: Vec<usize>,
    ends: Vec<usize>,
}

fn fold_for_search(value: &str) -> FoldedText {
    let mut folded = String::new();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for (source_start, character) in value.char_indices() {
        let source_end = source_start + character.len_utf8();
        let lowered = character.to_lowercase().to_string();
        folded.push_str(&lowered);
        starts.resize(folded.len(), source_start);
        ends.resize(folded.len(), source_end);
    }
    FoldedText {
        value: folded,
        starts,
        ends,
    }
}

fn highlight_line(
    line: &Line<'static>,
    ranges: &[(usize, RenderedSearchFragment)],
    active: Option<usize>,
) -> Line<'static> {
    let mut spans = Vec::new();
    let mut column = 0;
    for span in &line.spans {
        let mut segment = String::new();
        let mut segment_style = None;
        for character in span.content.chars() {
            let width = character.width().unwrap_or(0);
            let next_column = column + width;
            let matched = ranges
                .iter()
                .find(|(_, range)| range.start_column < next_column && range.end_column > column);
            let style = match matched {
                Some((index, _)) if Some(*index) == active => span
                    .style
                    .fg(theme::BASE)
                    .bg(theme::SEARCH_ACTIVE)
                    .add_modifier(Modifier::BOLD),
                Some(_) => span.style.bg(theme::SEARCH_MATCH),
                None => span.style,
            };
            if segment_style.is_some_and(|current| current != style) {
                spans.push(Span::styled(
                    std::mem::take(&mut segment),
                    segment_style.expect("checked style"),
                ));
            }
            segment_style = Some(style);
            segment.push(character);
            column = next_column;
        }
        if !segment.is_empty() {
            spans.push(Span::styled(segment, segment_style.unwrap_or(span.style)));
        }
    }
    Line::from(spans)
}
