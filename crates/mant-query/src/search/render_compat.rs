//! Compatibility search over canonical Markdown, including TLDR-only content.

use std::ops::Range;

use grep_matcher::Matcher;
use mant_protocol::{
    MarkdownSchema, QuerySearch, SearchContextLine, SearchHit, SearchLineRange,
    SearchMarkdownRange, SearchOccurrence, SearchQuery, SearchRender, SearchRenderFormat,
    SearchRenderScope, SearchSchema,
};

use super::SearchError;
use super::mapping::{LineIndex, SearchableText};
use super::owners::{Owner, OwnerIndex};
use super::plan::{empty_match_error, matcher_error, non_utf8_pattern_error};
use crate::ResolvedContent;
use mant_codec::encode::render_addressable_markdown;
use mant_ir::DocumentBodyRef;

const MAX_OCCURRENCES_PER_MATCH: usize = 256;

pub(super) fn search_with_matcher(
    query: &ResolvedContent,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
    // The compatibility search below is defined for Flow's canonical
    // Markdown. A future Fixed arm must select its explicit visible/artifact
    // reader here; it must never fall through to an empty Flow export.
    match query.document.as_ref().map(mant_ir::Document::body) {
        Some(DocumentBodyRef::Flow(_)) | None => search_flow_with_matcher(query, request, matcher),
        Some(DocumentBodyRef::Fixed(_)) => Err(SearchError::UnsupportedFixed),
    }
}

fn search_flow_with_matcher(
    query: &ResolvedContent,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
    let artifact = render_addressable_markdown(query).map_err(SearchError::InvalidFixed)?;
    let markdown = artifact.text();
    let lines = LineIndex::with_anchors(markdown, artifact.anchor_ranges().to_vec());
    let owners = OwnerIndex::new(
        &artifact,
        query.document.as_ref().map(mant_ir::Document::content),
    );
    let searchable = SearchableText::new(markdown, request.scope);
    let line_count = u32::try_from(lines.count()).map_err(|_| SearchError::ResourceLimit)?;
    let limit = usize::try_from(request.limit).map_err(|_| SearchError::ResourceLimit)?;
    let mut collector = SearchCollector::new(markdown, &lines, request.offset, limit);
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
    let returned = u32::try_from(selected.len()).map_err(|_| SearchError::ResourceLimit)?;
    let consumed = request
        .offset
        .checked_add(returned)
        .ok_or(SearchError::ResourceLimit)?;
    let truncated = consumed < total;

    Ok(QuerySearch {
        schema: SearchSchema::V0Dot12,
        label: query.label.clone(),
        source_context: query
            .document
            .as_ref()
            .map(mant_protocol::SourceContext::from),
        meta: query
            .document
            .as_ref()
            .map(|document| document.meta.clone()),
        content_projection: None,
        query: request.clone(),
        render: SearchRender {
            schema: MarkdownSchema::V1,
            format: SearchRenderFormat::Markdown,
            scope: SearchRenderScope::Full,
            line_base: 1,
            column_base: 1,
            line_count,
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
    let mut collection_error = None;
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
            if !occurrence_has_line_ranges(markdown_start..markdown_end, markdown, lines) {
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
                && let Err(error) = collector.push(
                    found.start()..found.end(),
                    markdown_start..markdown_end,
                    owner,
                )
            {
                collection_error = Some(error);
                return false;
            }
            true
        })
        .map_err(matcher_error)?;
    if let Some(error) = collection_error {
        Err(error)
    } else if invalid_utf8_match {
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
    offset: u32,
    limit: usize,
    total: u32,
    selected: Vec<RawMatchGroup>,
    current: Option<PendingRawMatchGroup>,
}

impl<'a> SearchCollector<'a> {
    fn new(markdown: &'a str, lines: &'a LineIndex, offset: u32, limit: usize) -> Self {
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

    fn push(
        &mut self,
        searchable: Range<usize>,
        markdown: Range<usize>,
        owner: &Owner,
    ) -> Result<(), SearchError> {
        let start_line_index = self
            .lines
            .position(self.markdown, markdown.start)
            .line_index;
        let end_line_index = self
            .lines
            .line_index_at_byte(markdown.end.saturating_sub(1));
        if let Some(group) = self.current.as_mut().filter(|group| {
            group.start_line_index == start_line_index
                && group.end_line_index == end_line_index
                && group.owner.key() == owner.key
        }) {
            group.occurrence_count = group
                .occurrence_count
                .checked_add(1)
                .ok_or(SearchError::ResourceLimit)?;
            if matches!(group.owner, PendingOwner::Retained(_))
                && group.occurrences.len() < MAX_OCCURRENCES_PER_MATCH
            {
                group.occurrences.push(RawOccurrence {
                    searchable,
                    line_ranges: occurrence_line_ranges(
                        markdown.clone(),
                        self.markdown,
                        self.lines,
                    ),
                    markdown,
                });
            }
            return Ok(());
        }

        self.flush();
        let ordinal = self
            .total
            .checked_add(1)
            .ok_or(SearchError::ResourceLimit)?;
        let retained = self.total >= self.offset && self.selected.len() < self.limit;
        let occurrences = retained
            .then(|| RawOccurrence {
                searchable,
                line_ranges: occurrence_line_ranges(markdown.clone(), self.markdown, self.lines),
                markdown,
            })
            .into_iter()
            .collect();
        self.current = Some(PendingRawMatchGroup {
            ordinal,
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
        Ok(())
    }

    fn flush(&mut self) {
        let Some(group) = self.current.take() else {
            return;
        };
        self.total = group.ordinal;
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
        (self.selected, self.total)
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
                    root: None,
                    logical: None,
                    markdown_projections: Vec::new(),
                    matched_text: if searchable.direct_markdown {
                        presented_matched_text(occurrence, markdown, lines)
                    } else {
                        searchable.text[occurrence.searchable.clone()].to_owned()
                    },
                    markdown: Some(SearchMarkdownRange {
                        start_byte: u64::try_from(occurrence.markdown.start).unwrap_or(u64::MAX),
                        end_byte: u64::try_from(occurrence.markdown.end).unwrap_or(u64::MAX),
                        start_line: u32::try_from(start.line_index.saturating_add(1))
                            .unwrap_or(u32::MAX),
                        start_column: u32::try_from(start.column).unwrap_or(u32::MAX),
                        end_line: u32::try_from(end.line_index.saturating_add(1))
                            .unwrap_or(u32::MAX),
                        end_column: u32::try_from(end.column).unwrap_or(u32::MAX),
                    }),
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

// Skipped pages and count-only groups still need to reject matches wholly
// inside hidden source-map anchors, but they do not need retained line ranges.
fn occurrence_has_line_ranges(
    markdown_range: Range<usize>,
    markdown: &str,
    lines: &LineIndex,
) -> bool {
    let start = lines.position(markdown, markdown_range.start).line_index;
    let end = lines.line_index_at_byte(markdown_range.end.saturating_sub(1));
    (start..=end).any(|line_index| {
        let line_start = lines.start(line_index);
        let line = lines.line(markdown, line_index).trim_end();
        let line_end = line_start.saturating_add(line.len());
        let intersection = markdown_range.start.max(line_start)..markdown_range.end.min(line_end);
        if intersection.start >= intersection.end {
            return false;
        }
        lines.has_presented_range(
            markdown,
            line_index,
            intersection.start.saturating_sub(line_start)
                ..intersection.end.saturating_sub(line_start),
        )
    })
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

#[cfg(test)]
mod tests {
    use super::{SearchCollector, SearchError, occurrence_has_line_ranges, occurrence_line_ranges};
    use crate::search::mapping::LineIndex;
    use crate::search::owners::Owner;

    #[test]
    fn count_only_visibility_agrees_with_retained_ranges() {
        // The hidden anchor is inside line one; line two has trailing spaces
        // which presentation deliberately trims. Exercise every byte range,
        // including ranges that cross line and anchor boundaries.
        let markdown = "ab<!-- -->cd  \nef  \n";
        let lines = LineIndex::with_anchors(markdown, std::iter::once(2..10).collect());
        for start in 0..markdown.len() {
            for end in start + 1..=markdown.len() {
                let range = start..end;
                assert_eq!(
                    occurrence_has_line_ranges(range.clone(), markdown, &lines),
                    !occurrence_line_ranges(range.clone(), markdown, &lines).is_empty(),
                    "range {range:?}"
                );
            }
        }
        assert!(!occurrence_has_line_ranges(2..10, markdown, &lines));
        assert!(!occurrence_has_line_ranges(12..14, markdown, &lines));
        assert!(occurrence_has_line_ranges(0..12, markdown, &lines));
        assert!(occurrence_has_line_ranges(10..17, markdown, &lines));
    }

    #[test]
    fn collector_rejects_group_and_occurrence_counter_overflow() {
        let markdown = "a\n";
        let lines = LineIndex::with_anchors(markdown, Vec::new());
        let owner = Owner {
            key: 0,
            start: 0,
            end: markdown.len(),
            source: None,
        };
        let mut groups = SearchCollector::new(markdown, &lines, 0, 1);
        groups.total = u32::MAX;
        assert_eq!(
            groups.push(0..1, 0..1, &owner),
            Err(SearchError::ResourceLimit)
        );

        let mut occurrences = SearchCollector::new(markdown, &lines, 0, 1);
        occurrences.push(0..1, 0..1, &owner).unwrap();
        occurrences.current.as_mut().unwrap().occurrence_count = u32::MAX;
        assert_eq!(
            occurrences.push(0..1, 0..1, &owner),
            Err(SearchError::ResourceLimit)
        );
    }
}
