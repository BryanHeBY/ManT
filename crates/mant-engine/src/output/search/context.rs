//! Group supplied DTO context and format exact source/visible coordinates.
use super::line::render_search_line;
use mant_protocol::{SearchHit, SearchScope};
use std::{collections::BTreeMap, ops::Range};

pub(super) fn occurrence_line_ranges(found: &SearchHit, line: u32) -> Vec<Range<usize>> {
    found
        .occurrences
        .iter()
        .flat_map(|occurrence| occurrence.line_ranges.iter())
        .filter(|range| range.line == line)
        .filter_map(|range| {
            Some(usize::try_from(range.start_byte).ok()?..usize::try_from(range.end_byte).ok()?)
        })
        .collect()
}

pub(super) fn group_coordinates(matches: &[SearchHit]) -> String {
    let mut lines: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for occurrence in matches.iter().flat_map(|found| found.occurrences.iter()) {
        lines
            .entry(occurrence.markdown.start_line)
            .or_default()
            .push(occurrence.markdown.start_column);
    }
    format_coordinate_lines(lines)
}

pub(super) fn text_group_coordinates(matches: &[SearchHit], scope: SearchScope) -> String {
    if scope == SearchScope::Markdown {
        return group_coordinates(matches);
    }

    let mut lines: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for found in matches {
        for occurrence in &found.occurrences {
            let line = occurrence.markdown.start_line;
            let visible_column = search_line_text(found, line)
                .and_then(|text| {
                    let ranges = occurrence
                        .line_ranges
                        .iter()
                        .filter(|range| range.line == line)
                        .filter_map(|range| {
                            Some(
                                usize::try_from(range.start_byte).ok()?
                                    ..usize::try_from(range.end_byte).ok()?,
                            )
                        })
                        .collect::<Vec<_>>();
                    let (rendered, highlights) = render_search_line(text, &ranges);
                    highlights
                        .iter()
                        .map(|range| range.start)
                        .min()
                        .map(|start| {
                            u32::try_from(rendered[..start].chars().count().saturating_add(1))
                                .unwrap_or(u32::MAX)
                        })
                })
                .unwrap_or(occurrence.markdown.start_column);
            lines.entry(line).or_default().push(visible_column);
        }
    }
    format_coordinate_lines(lines)
}

fn search_line_text(found: &SearchHit, line: u32) -> Option<&str> {
    found
        .context
        .iter()
        .find(|context| context.line == line)
        .map(|context| context.text.as_str())
        .or_else(|| {
            found
                .occurrences
                .iter()
                .any(|occurrence| occurrence.markdown.start_line == line)
                .then_some(found.preview.as_str())
        })
}

fn format_coordinate_lines(lines: BTreeMap<u32, Vec<u32>>) -> String {
    lines
        .into_iter()
        .map(|(line, mut columns)| {
            columns.sort_unstable();
            columns.dedup();
            format!(
                "{line}:{}",
                columns
                    .into_iter()
                    .map(|column| column.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub(super) fn truncated_occurrence_summary(matches: &[SearchHit]) -> Option<String> {
    matches
        .iter()
        .any(|found| found.occurrences_truncated)
        .then(|| {
            let total = matches
                .iter()
                .map(|found| u64::from(found.occurrence_count))
                .sum::<u64>();
            let shown = matches
                .iter()
                .map(|found| found.occurrences.len() as u64)
                .sum::<u64>();
            format!("{total} occurrences; {shown} exact coordinates shown")
        })
}

pub(super) fn context_group_end(matches: &[SearchHit], start: usize) -> usize {
    let Some((_, mut last_line)) = context_bounds(&matches[start]) else {
        return start + 1;
    };
    let outline = &matches[start].outline;
    let mut end = start + 1;
    while let Some(found) = matches.get(end) {
        let Some((first_line, found_last_line)) = context_bounds(found) else {
            break;
        };
        if &found.outline != outline || first_line > last_line.saturating_add(1) {
            break;
        }
        last_line = last_line.max(found_last_line);
        end += 1;
    }
    end
}

fn context_bounds(found: &SearchHit) -> Option<(u32, u32)> {
    Some((found.context.first()?.line, found.context.last()?.line))
}

type MergedContext<'a> = BTreeMap<u32, (&'a str, bool, Vec<Range<usize>>)>;

pub(super) fn merged_context(matches: &[SearchHit]) -> MergedContext<'_> {
    let mut merged: MergedContext<'_> = BTreeMap::new();
    for found in matches {
        for line in &found.context {
            let entry = merged
                .entry(line.line)
                .or_insert_with(|| (line.text.as_str(), false, Vec::new()));
            entry.1 |= line.matched;
            if line.matched {
                entry.2.extend(occurrence_line_ranges(found, line.line));
            }
        }
    }
    merged
}
