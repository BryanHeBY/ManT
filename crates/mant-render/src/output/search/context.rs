//! Group supplied DTO context and format exact source/visible coordinates.
use mant_ir::ContentContext;
use mant_protocol::{SearchHit, SearchOccurrence};
use std::{collections::BTreeMap, ops::Range};

pub(super) fn occurrence_line_ranges(
    found: &SearchHit,
    logical: &str,
    line: u32,
) -> Vec<Range<usize>> {
    found
        .occurrences
        .iter()
        .filter_map(|occurrence| logical_line_range(logical, occurrence, line))
        .collect()
}

pub(super) fn occurrence_start_line(logical: &str, occurrence: &SearchOccurrence) -> Option<u32> {
    let start = usize::try_from(occurrence.logical?.start_byte).ok()?;
    let prefix = logical.get(..start)?;
    u32::try_from(
        prefix
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            .saturating_add(1),
    )
    .ok()
}

pub(super) fn group_coordinates(matches: &[SearchHit]) -> String {
    let mut lines: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for range in matches
        .iter()
        .flat_map(|found| &found.occurrences)
        .flat_map(|occurrence| {
            occurrence
                .markdown
                .iter()
                .chain(occurrence.markdown_projections.iter())
        })
    {
        lines
            .entry(range.start_line)
            .or_default()
            .push(range.start_column);
    }
    if !lines.is_empty() {
        return format_coordinate_lines(lines);
    }
    matches
        .iter()
        .flat_map(|found| &found.occurrences)
        .map(|occurrence| {
            let root = occurrence.root.expect("validated logical hit has a root");
            let logical = occurrence
                .logical
                .expect("validated logical hit has a range");
            format!(
                "logical-root-{}:{}",
                root.get(),
                logical.start_scalar.saturating_add(1)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn logical_line_range(
    text: &str,
    occurrence: &SearchOccurrence,
    line: u32,
) -> Option<Range<usize>> {
    let target = usize::try_from(line.checked_sub(1)?).ok()?;
    let line_start = std::iter::once(0)
        .chain(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        )
        .nth(target)?;
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |offset| line_start + offset);
    let match_start = usize::try_from(occurrence.logical?.start_byte).ok()?;
    let match_end = usize::try_from(occurrence.logical?.end_byte).ok()?;
    let start = match_start.max(line_start);
    let end = match_end.min(line_end);
    (start < end).then_some(start - line_start..end - line_start)
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
    let Some(root) = matches[start]
        .occurrences
        .first()
        .map(|occurrence| occurrence.root)
    else {
        return start + 1;
    };
    let mut end = start + 1;
    while let Some(found) = matches.get(end) {
        let Some((first_line, found_last_line)) = context_bounds(found) else {
            break;
        };
        if &found.outline != outline
            || found.occurrences.first().map(|occurrence| occurrence.root) != Some(root)
            || first_line > last_line.saturating_add(1)
        {
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

pub(super) fn merged_context<'a>(
    content: ContentContext<'_>,
    matches: &'a [SearchHit],
) -> MergedContext<'a> {
    let mut merged: MergedContext<'_> = BTreeMap::new();
    for found in matches {
        let logical = found.occurrences.first().and_then(|occurrence| {
            occurrence
                .root
                .and_then(|root| content.root_logical_text(root))
        });
        for line in &found.context {
            let entry = merged
                .entry(line.line)
                .or_insert_with(|| (line.text.as_str(), false, Vec::new()));
            entry.1 |= line.matched;
            if line.matched
                && let Some(logical) = &logical
            {
                entry
                    .2
                    .extend(occurrence_line_ranges(found, logical, line.line));
            }
        }
    }
    merged
}
