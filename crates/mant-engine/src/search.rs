//! Searches deterministic Markdown while retaining addressable manual nodes.
//!
//! Section and semantic-entry anchors emitted by the Markdown renderer form
//! an internal source map. pulldown-cmark supplies a visible-text projection
//! whose byte ranges map back into that exact Markdown document.

use std::{error::Error, fmt, ops::Range};

use grep_matcher::Matcher;
use mant_protocol::{
    MAX_SEARCH_PATTERN_CHARS, MarkdownSchema, QuerySearch, SearchContextLine, SearchHit,
    SearchLineRange, SearchMarkdownRange, SearchOccurrence, SearchQuery, SearchRender,
    SearchRenderFormat, SearchRenderScope, SearchSchema,
};

use crate::{ResolvedContent, output::render_addressable_markdown};

mod mapping;
mod owners;
mod plan;
#[cfg(test)]
use mapping::display_markdown_line;
use mapping::{LineIndex, SearchableText};
pub(crate) use plan::SearchPlan;
pub use plan::validate_search_query;
use plan::{
    MAX_CONTEXT_LINES, MAX_SEARCH_LIMIT, empty_match_error, matcher_error, non_utf8_pattern_error,
};

use owners::{Owner, OwnerIndex};

const MAX_OCCURRENCES_PER_MATCH: usize = 256;
/// Invalid search input or matcher construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// Search pattern contained no bytes.
    EmptyPattern,
    /// Search pattern exceeded the request bound.
    PatternTooLong,
    /// Result limit was zero or exceeded the protocol maximum.
    InvalidLimit,
    /// Requested context exceeded the protocol maximum.
    ContextTooLarge,
    /// Regular-expression compilation or execution failed.
    InvalidPattern(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => formatter.write_str("search pattern must not be empty"),
            Self::PatternTooLong => write!(
                formatter,
                "search pattern exceeds the {MAX_SEARCH_PATTERN_CHARS}-character limit"
            ),
            Self::InvalidLimit => write!(
                formatter,
                "search limit must be between 1 and {MAX_SEARCH_LIMIT}"
            ),
            Self::ContextTooLarge => write!(
                formatter,
                "search context must not exceed {MAX_CONTEXT_LINES} lines"
            ),
            Self::InvalidPattern(message) => write!(formatter, "invalid search pattern: {message}"),
        }
    }
}

impl Error for SearchError {}

/// Search one complete query and report coordinates in its canonical Markdown.
///
/// # Errors
///
/// Returns [`SearchError`] for empty or excessive inputs and invalid regular
/// expressions. A valid search with no matches is a successful empty result.
pub fn search_query(
    query: &ResolvedContent,
    request: &SearchQuery,
) -> Result<QuerySearch, SearchError> {
    SearchPlan::new(request)?.execute(query, request.offset, request.limit)
}

fn search_with_matcher(
    query: &ResolvedContent,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
    let artifact = render_addressable_markdown(query);
    let markdown = artifact.text();
    let lines = LineIndex::with_anchors(markdown, artifact.anchor_ranges().to_vec());
    let owners = OwnerIndex::new(&artifact);
    let searchable = SearchableText::new(markdown, request.scope);
    let offset = usize::try_from(request.offset).unwrap_or(usize::MAX);
    let limit = usize::try_from(request.limit).unwrap_or(usize::MAX);
    let mut collector = SearchCollector::new(markdown, &lines, offset, limit);
    collect_occurrences(
        matcher,
        &searchable,
        markdown,
        &lines,
        &owners,
        &mut collector,
    )?;

    let (raw_groups, total) = collector.finish();
    let selected = raw_groups
        .iter()
        .map(|found| {
            build_match(
                found,
                &searchable,
                markdown,
                &lines,
                &owners,
                request.context_lines,
            )
        })
        .collect::<Vec<_>>();
    let returned = u32::try_from(selected.len()).unwrap_or(u32::MAX);
    let consumed = request.offset.saturating_add(returned);
    let truncated = consumed < total;

    Ok(QuerySearch {
        schema: SearchSchema::V0Dot11,
        label: query.label.clone(),
        source: query
            .document
            .as_ref()
            .map(|document| document.source.clone()),
        meta: query
            .document
            .as_ref()
            .map(|document| document.meta.clone()),
        query: request.clone(),
        render: SearchRender {
            schema: MarkdownSchema::V1,
            format: SearchRenderFormat::Markdown,
            scope: SearchRenderScope::Full,
            line_base: 1,
            column_base: 1,
            line_count: u32::try_from(lines.count()).unwrap_or(u32::MAX),
        },
        total,
        returned,
        offset: request.offset,
        truncated,
        next_offset: truncated.then_some(consumed),
        matches: selected,
    })
}

fn collect_occurrences(
    matcher: &grep_regex::RegexMatcher,
    searchable: &SearchableText,
    markdown: &str,
    lines: &LineIndex,
    owners: &OwnerIndex<'_, '_>,
    collector: &mut SearchCollector<'_>,
) -> Result<(), SearchError> {
    let mut invalid_utf8_match = false;
    let mut invalid_zero_width_match = false;
    matcher
        .find_iter(searchable.text.as_bytes(), |found| {
            if found.start() == found.end() {
                invalid_zero_width_match = true;
                return false;
            }
            if !searchable.text.is_char_boundary(found.start())
                || !searchable.text.is_char_boundary(found.end())
            {
                invalid_utf8_match = true;
                return false;
            }
            let markdown_start = searchable.markdown_start(found.start());
            let markdown_end = searchable.markdown_end(found.end());
            if !markdown.is_char_boundary(markdown_start)
                || !markdown.is_char_boundary(markdown_end)
            {
                invalid_utf8_match = true;
                return false;
            }
            if markdown_start >= markdown_end {
                // Visible text contains synthetic separators between block
                // events. They make whitespace searchable but have no
                // canonical Markdown bytes of their own, so this occurrence
                // is intentionally absent rather than invalidating the query.
                return true;
            }
            let line_ranges = occurrence_line_ranges(markdown_start..markdown_end, markdown, lines);
            if line_ranges.is_empty() {
                // A Markdown-scope matcher can land wholly inside one of the
                // zero-width source-map anchors. Such internal matches have
                // no anchor-free presentation and must not become phantom
                // result rows.
                return true;
            }
            let owner = owners.owner(markdown_start);
            let end_owner = owners.owner(markdown_end - 1);
            if let (Some(owner), Some(end_owner)) = (owner, end_owner)
                && owner.key == end_owner.key
            {
                collector.push(
                    RawOccurrence {
                        searchable: found.start()..found.end(),
                        markdown: markdown_start..markdown_end,
                        line_ranges,
                    },
                    owner,
                );
            }
            true
        })
        .map_err(matcher_error)?;
    if invalid_utf8_match {
        Err(non_utf8_pattern_error())
    } else if invalid_zero_width_match {
        Err(empty_match_error())
    } else {
        Ok(())
    }
}

struct RawOccurrence {
    searchable: Range<usize>,
    markdown: Range<usize>,
    line_ranges: Vec<SearchLineRange>,
}

struct RawMatchGroup {
    ordinal: u32,
    occurrences: Vec<RawOccurrence>,
    occurrence_count: u32,
    owner: Owner,
    start_line_index: usize,
    end_line_index: usize,
}

struct PendingRawMatchGroup {
    ordinal: u32,
    occurrences: Vec<RawOccurrence>,
    occurrence_count: u32,
    owner: PendingOwner,
    start_line_index: usize,
    end_line_index: usize,
}

enum PendingOwner {
    Retained(Owner),
    CountOnly(usize),
}

impl PendingOwner {
    const fn key(&self) -> usize {
        match self {
            Self::Retained(owner) => owner.key,
            Self::CountOnly(key) => *key,
        }
    }
}

struct SearchCollector<'a> {
    markdown: &'a str,
    lines: &'a LineIndex,
    offset: usize,
    limit: usize,
    total: usize,
    selected: Vec<RawMatchGroup>,
    current: Option<PendingRawMatchGroup>,
}

impl<'a> SearchCollector<'a> {
    fn new(markdown: &'a str, lines: &'a LineIndex, offset: usize, limit: usize) -> Self {
        Self {
            markdown,
            lines,
            offset,
            limit,
            total: 0,
            selected: Vec::with_capacity(limit.min(256)),
            current: None,
        }
    }

    fn push(&mut self, occurrence: RawOccurrence, owner: &Owner) {
        let start_line_index = self
            .lines
            .position(self.markdown, occurrence.markdown.start)
            .line_index;
        let end_line_index = self
            .lines
            .line_index_at_byte(occurrence.markdown.end.saturating_sub(1));
        if let Some(group) = self.current.as_mut().filter(|group| {
            group.start_line_index == start_line_index
                && group.end_line_index == end_line_index
                && group.owner.key() == owner.key
        }) {
            group.occurrence_count = group.occurrence_count.saturating_add(1);
            if matches!(group.owner, PendingOwner::Retained(_))
                && group.occurrences.len() < MAX_OCCURRENCES_PER_MATCH
            {
                group.occurrences.push(occurrence);
            }
            return;
        }

        self.flush();
        let retained = self.total >= self.offset && self.selected.len() < self.limit;
        let occurrences = retained.then_some(occurrence).into_iter().collect();
        self.current = Some(PendingRawMatchGroup {
            ordinal: u32::try_from(self.total.saturating_add(1)).unwrap_or(u32::MAX),
            occurrences,
            occurrence_count: 1,
            owner: if retained {
                PendingOwner::Retained(*owner)
            } else {
                PendingOwner::CountOnly(owner.key)
            },
            start_line_index,
            end_line_index,
        });
    }

    fn flush(&mut self) {
        let Some(group) = self.current.take() else {
            return;
        };
        self.total = self.total.saturating_add(1);
        if let PendingOwner::Retained(owner) = group.owner {
            self.selected.push(RawMatchGroup {
                ordinal: group.ordinal,
                occurrences: group.occurrences,
                occurrence_count: group.occurrence_count,
                owner,
                start_line_index: group.start_line_index,
                end_line_index: group.end_line_index,
            });
        }
    }

    fn finish(mut self) -> (Vec<RawMatchGroup>, u32) {
        self.flush();
        (self.selected, u32::try_from(self.total).unwrap_or(u32::MAX))
    }
}

impl RawMatchGroup {
    fn occurrences_truncated(&self) -> bool {
        usize::try_from(self.occurrence_count).map_or(true, |count| count > self.occurrences.len())
    }
}

fn build_match(
    found: &RawMatchGroup,
    searchable: &SearchableText,
    markdown: &str,
    lines: &LineIndex,
    owners: &OwnerIndex<'_, '_>,
    context_lines: u16,
) -> SearchHit {
    let first = &found.occurrences[0];
    let start = lines.position(markdown, first.markdown.start);
    let preview = lines.presented_line(markdown, start.line_index).text;
    let context_start = found
        .start_line_index
        .saturating_sub(usize::from(context_lines));
    let context_end = found
        .end_line_index
        .saturating_add(usize::from(context_lines))
        .min(lines.count().saturating_sub(1));
    let context = if context_lines == 0 {
        Vec::new()
    } else {
        (context_start..=context_end)
            .map(|line_index| SearchContextLine {
                line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
                text: lines.presented_line(markdown, line_index).text,
                matched: (found.start_line_index..=found.end_line_index).contains(&line_index),
            })
            .collect()
    };

    SearchHit {
        ordinal: found.ordinal,
        outline: owners.trail(found.owner.key),
        occurrences: found
            .occurrences
            .iter()
            .map(|occurrence| {
                let start = lines.position(markdown, occurrence.markdown.start);
                let end = lines.position(markdown, occurrence.markdown.end);
                SearchOccurrence {
                    matched_text: if searchable.direct_markdown {
                        presented_matched_text(occurrence, markdown, lines)
                    } else {
                        searchable.text[occurrence.searchable.clone()].to_owned()
                    },
                    markdown: SearchMarkdownRange {
                        start_byte: u64::try_from(occurrence.markdown.start).unwrap_or(u64::MAX),
                        end_byte: u64::try_from(occurrence.markdown.end).unwrap_or(u64::MAX),
                        start_line: u32::try_from(start.line_index.saturating_add(1))
                            .unwrap_or(u32::MAX),
                        start_column: u32::try_from(start.column).unwrap_or(u32::MAX),
                        end_line: u32::try_from(end.line_index.saturating_add(1))
                            .unwrap_or(u32::MAX),
                        end_column: u32::try_from(end.column).unwrap_or(u32::MAX),
                    },
                    line_ranges: occurrence.line_ranges.clone(),
                }
            })
            .collect(),
        occurrence_count: found.occurrence_count,
        occurrences_truncated: found.occurrences_truncated(),
        node_source: found.owner.source,
        preview,
        context,
    }
}

fn occurrence_line_ranges(
    markdown_range: Range<usize>,
    markdown: &str,
    lines: &LineIndex,
) -> Vec<SearchLineRange> {
    let start = lines.position(markdown, markdown_range.start).line_index;
    let end = lines.line_index_at_byte(markdown_range.end.saturating_sub(1));
    (start..=end)
        .flat_map(|line_index| {
            let line_start = lines.start(line_index);
            let line = lines.line(markdown, line_index).trim_end();
            let line_end = line_start.saturating_add(line.len());
            let intersection =
                markdown_range.start.max(line_start)..markdown_range.end.min(line_end);
            (intersection.start < intersection.end)
                .then(|| lines.presented_line(markdown, line_index))
                .into_iter()
                .flat_map(move |visible| {
                    visible.map_range(
                        intersection.start.saturating_sub(line_start)
                            ..intersection.end.saturating_sub(line_start),
                    )
                })
                .map(move |range| SearchLineRange {
                    line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
                    start_byte: u32::try_from(range.start).unwrap_or(u32::MAX),
                    end_byte: u32::try_from(range.end).unwrap_or(u32::MAX),
                })
        })
        .collect()
}

fn presented_matched_text(occurrence: &RawOccurrence, markdown: &str, lines: &LineIndex) -> String {
    let mut text = String::new();
    let mut previous_line = None;
    for range in &occurrence.line_ranges {
        let line_index = usize::try_from(range.line.saturating_sub(1)).unwrap_or(usize::MAX);
        if previous_line.is_some_and(|previous| previous != line_index) {
            text.push('\n');
        }
        let line = lines.presented_line(markdown, line_index).text;
        let start = usize::try_from(range.start_byte).unwrap_or(usize::MAX);
        let end = usize::try_from(range.end_byte).unwrap_or(usize::MAX);
        if let Some(fragment) = line.get(start..end) {
            text.push_str(fragment);
        }
        previous_line = Some(line_index);
    }
    text
}

/// Test convenience; production uses the full-document marker index.
#[cfg(test)]
mod tests;
