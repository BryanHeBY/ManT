//! R02a Flow/TLDR search over the unchanged canonical Markdown visible extractor.
//!
//! The matcher and owner filter preserve the pre-R02a selection rule. Only
//! retention changes: each occurrence gets one ordinal and one bounded unit;
//! renderer-inserted visible breaks are explicit joins, never authored bytes.

use std::num::NonZeroU32;
use std::ops::Range;

use grep_matcher::Matcher;
use mant_codec::encode::render_addressable_markdown;
use mant_ir::DocumentBodyRef;
use mant_protocol::{
    MAX_SEARCH_PRESENTATION_BYTES, QuerySearch, SearchContentProjection, SearchContextLine,
    SearchDisplaySlice, SearchFlowFragmentSource, SearchFlowSourceKind, SearchFragment,
    SearchFragmentSource, SearchLocation, SearchMatch, SearchQuery, SearchRender,
    SearchRenderDerivedFragmentSource, SearchRenderDerivedSourceKind, SearchRenderFormat,
    SearchRenderSchema, SearchRenderScope, SearchSchema, SearchScope, SearchTextJoin,
    SearchTextUnit, SearchTldrFragmentSource, SearchTldrSourceKind,
};

use super::SearchError;
use super::mapping::{LineIndex, SearchableText, VisiblePart};
use super::origins::FlowOrigins;
use super::owners::{Owner, OwnerIndex};
use super::plan::{empty_match_error, matcher_error, non_utf8_pattern_error};
use super::scalar::ScalarCursor;
use crate::ResolvedContent;

pub(super) fn search_with_matcher(
    query: &ResolvedContent,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
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
    let mut collector = OccurrenceCollector::new(request.offset, request.limit);
    collect_occurrences(
        matcher,
        &searchable,
        markdown,
        &lines,
        &owners,
        request.scope,
        &mut collector,
    )?;
    let (selected, total) = collector.finish();
    let origins = if request.scope == SearchScope::Visible && !selected.is_empty() {
        FlowOrigins::new(query.document.as_ref(), &artifact)
    } else {
        FlowOrigins::default()
    };

    let mut projection = SearchContentProjection {
        fragments: Vec::new(),
        units: Vec::new(),
    };
    let mut presentation_bytes = 0usize;
    let mut scalar_cursor = ScalarCursor::default();
    let match_context = MatchContext {
        searchable: &searchable,
        markdown,
        lines: &lines,
        owners: &owners,
        origins: &origins,
        context_lines: request.context_lines,
        scope: request.scope,
    };
    let retained_matches = selected
        .iter()
        .map(|found| {
            build_match(
                found,
                &match_context,
                &mut projection,
                &mut presentation_bytes,
                &mut scalar_cursor,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let content_projection =
        if request.scope == SearchScope::Visible && !retained_matches.is_empty() {
            projection
                .validate()
                .map_err(|_| SearchError::ResourceLimit)?;
            Some(projection)
        } else {
            None
        };
    finish_flow_search(
        query,
        request,
        line_count,
        total,
        retained_matches,
        content_projection,
    )
}

fn finish_flow_search(
    query: &ResolvedContent,
    request: &SearchQuery,
    line_count: u32,
    total: u32,
    retained_matches: Vec<SearchMatch>,
    content_projection: Option<SearchContentProjection>,
) -> Result<QuerySearch, SearchError> {
    let returned = u32::try_from(retained_matches.len()).map_err(|_| SearchError::ResourceLimit)?;
    let consumed = request
        .offset
        .checked_add(returned)
        .ok_or(SearchError::ResourceLimit)?;
    let truncated = returned != 0 && consumed < total;
    let diagnostics = query
        .document
        .as_ref()
        .map_or_else(Vec::new, |document| document.diagnostics.clone());
    // Document-level diagnostics are already bounded by the normalized input
    // budget. Do not silently lose evidence of incomplete semantic coverage.
    if diagnostics.len() > mant_protocol::MAX_SEARCH_DIAGNOSTICS {
        return Err(SearchError::ResourceLimit);
    }
    let result = QuerySearch {
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
        content_projection,
        query: request.clone(),
        render: SearchRender {
            schema: SearchRenderSchema::Markdown,
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
        semantics_complete: mant_ir::semantics_complete(&diagnostics),
        coverage_details_omitted: 0,
        diagnostics,
        matches: retained_matches,
    };
    result
        .validate()
        .map_err(|_| SearchError::ContentProjection)?;
    Ok(result)
}

fn collect_occurrences(
    matcher: &grep_regex::RegexMatcher,
    searchable: &SearchableText,
    markdown: &str,
    lines: &LineIndex,
    owners: &OwnerIndex<'_, '_>,
    scope: SearchScope,
    collector: &mut OccurrenceCollector,
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
            if markdown_start >= markdown_end
                || (scope == SearchScope::Visible
                    && !occurrence_is_presented(markdown_start..markdown_end, lines))
            {
                return true;
            }
            let owner = owners.owner(markdown_start);
            let end_owner = owners.owner(markdown_end - 1);
            if let Some(owner) = owner
                && (scope == SearchScope::Markdown
                    || end_owner.is_some_and(|end_owner| owner.key == end_owner.key))
                && let Err(error) = collector.push(
                    found.start()..found.end(),
                    markdown_start..markdown_end,
                    *owner,
                    lines,
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

#[derive(Clone)]
struct RawOccurrence {
    ordinal: u32,
    searchable: Range<usize>,
    markdown: Range<usize>,
    owner: Owner,
    start_line_index: usize,
    end_line_index: usize,
}

struct OccurrenceCollector {
    offset: u32,
    limit: u32,
    total: u32,
    selected: Vec<RawOccurrence>,
}

impl OccurrenceCollector {
    fn new(offset: u32, limit: u32) -> Self {
        Self {
            offset,
            limit,
            total: 0,
            selected: Vec::new(),
        }
    }

    fn push(
        &mut self,
        searchable: Range<usize>,
        markdown: Range<usize>,
        owner: Owner,
        lines: &LineIndex,
    ) -> Result<(), SearchError> {
        self.total = self
            .total
            .checked_add(1)
            .ok_or(SearchError::ResourceLimit)?;
        if self.total <= self.offset || self.selected.len() >= self.limit as usize {
            return Ok(());
        }
        self.selected.push(RawOccurrence {
            ordinal: self.total,
            searchable,
            start_line_index: lines.line_index_at_byte(markdown.start),
            end_line_index: lines.line_index_at_byte(markdown.end - 1),
            markdown,
            owner,
        });
        Ok(())
    }

    fn finish(self) -> (Vec<RawOccurrence>, u32) {
        (self.selected, self.total)
    }
}

struct MatchContext<'a, 'b> {
    searchable: &'a SearchableText,
    markdown: &'a str,
    lines: &'a LineIndex,
    owners: &'a OwnerIndex<'a, 'b>,
    origins: &'a FlowOrigins,
    context_lines: u16,
    scope: SearchScope,
}

fn build_match(
    found: &RawOccurrence,
    context: &MatchContext<'_, '_>,
    projection: &mut SearchContentProjection,
    presentation_bytes: &mut usize,
    scalar_cursor: &mut ScalarCursor,
) -> Result<SearchMatch, SearchError> {
    let MatchContext {
        searchable,
        markdown,
        lines,
        owners,
        origins,
        context_lines,
        scope,
    } = context;
    let start = lines.position(markdown, found.markdown.start);
    let end = lines.position(markdown, found.markdown.end);
    let preview = lines.presented_line(markdown, start.line_index).text;
    let surrounding = if *context_lines == 0 {
        Vec::new()
    } else {
        let first = found
            .start_line_index
            .saturating_sub(*context_lines as usize);
        let last = found
            .end_line_index
            .saturating_add(*context_lines as usize)
            .min(lines.count().saturating_sub(1));
        (first..=last)
            .map(|line_index| SearchContextLine {
                line: u32::try_from(line_index + 1).unwrap_or(u32::MAX),
                text: lines.presented_line(markdown, line_index).text,
                matched: (found.start_line_index..=found.end_line_index).contains(&line_index),
            })
            .collect()
    };
    let (matched_text, location, display_slices) = if *scope == SearchScope::Markdown {
        let matched_text = markdown[found.markdown.clone()].to_owned();
        let location = SearchLocation::MarkdownArtifact {
            start_scalar: scalar_cursor.at(markdown, found.markdown.start)?,
            end_scalar: scalar_cursor.at(markdown, found.markdown.end)?,
            start_line: u32::try_from(start.line_index + 1)
                .map_err(|_| SearchError::ResourceLimit)?,
            start_column: u32::try_from(start.column).map_err(|_| SearchError::ResourceLimit)?,
            end_line: u32::try_from(end.line_index + 1).map_err(|_| SearchError::ResourceLimit)?,
            end_column: u32::try_from(end.column).map_err(|_| SearchError::ResourceLimit)?,
        };
        (matched_text, location, Vec::new())
    } else {
        let matched_text = searchable.text[found.searchable.clone()].to_owned();
        let tldr_base = owners
            .tldr_start(found.owner.key)
            .and_then(|start| searchable.visible_start_for_markdown(start));
        let visible_projection = project_visible(
            searchable,
            found.searchable.clone(),
            projection,
            origins,
            tldr_base,
        )?;
        let location = SearchLocation::VisibleFlow {
            unit: visible_projection.unit,
            start_scalar: visible_projection.start_scalar,
            end_scalar: visible_projection.end_scalar,
        };
        (matched_text, location, visible_projection.display_slices)
    };
    *presentation_bytes = presentation_bytes
        .checked_add(matched_text.len())
        .and_then(|value| value.checked_add(preview.len()))
        .ok_or(SearchError::ResourceLimit)?;
    for line in &surrounding {
        *presentation_bytes = presentation_bytes
            .checked_add(line.text.len())
            .ok_or(SearchError::ResourceLimit)?;
    }
    if *presentation_bytes > MAX_SEARCH_PRESENTATION_BYTES {
        return Err(SearchError::ResourceLimit);
    }
    Ok(SearchMatch {
        ordinal: found.ordinal,
        outline: owners.trail(found.owner.key),
        matched_text,
        location,
        display_slices,
        node_source: found.owner.source,
        preview,
        context: surrounding,
    })
}

struct VisibleProjection {
    unit: NonZeroU32,
    start_scalar: u64,
    end_scalar: u64,
    display_slices: Vec<SearchDisplaySlice>,
}

fn project_visible(
    searchable: &SearchableText,
    matched_range: Range<usize>,
    projection: &mut SearchContentProjection,
    origins: &FlowOrigins,
    tldr_base: Option<usize>,
) -> Result<VisibleProjection, SearchError> {
    // A leading render break needs a real predecessor to be a join, rather
    // than invented display bytes. It is context, not part of the match.
    let prefix_start = if searchable.is_separator_at(matched_range.start) {
        searchable.text[..matched_range.start]
            .char_indices()
            .next_back()
            .map_or(matched_range.start, |(start, _)| start)
    } else {
        matched_range.start
    };
    let unit_range = prefix_start..matched_range.end;
    let unit_key = dense_key(projection.units.len())?;
    let mut keys = Vec::new();
    let mut joins = Vec::new();
    let mut slices = Vec::new();
    let mut pending_separator = String::new();
    for part in searchable.parts(unit_range.clone()) {
        if part.separator {
            pending_separator.push_str(part.text);
            continue;
        }
        if !keys.is_empty() {
            joins.push(if pending_separator.is_empty() {
                SearchTextJoin::DirectContact
            } else {
                SearchTextJoin::RenderSeparator {
                    text: std::mem::take(&mut pending_separator),
                }
            });
        }
        let key = dense_key(projection.fragments.len())?;
        let source = source_for_visible_part(&part, matched_range.start, origins, tldr_base)?;
        let display_start = matched_range.start.max(part.visible.start);
        let display_end = matched_range.end.min(part.visible.end);
        if display_start < display_end {
            slices.push(SearchDisplaySlice {
                fragment: key,
                start_scalar: part.text[..display_start - part.visible.start]
                    .chars()
                    .count() as u64,
                end_scalar: part.text[..display_end - part.visible.start]
                    .chars()
                    .count() as u64,
            });
        }
        projection.fragments.push(SearchFragment {
            key,
            text: part.text.to_owned(),
            source,
        });
        keys.push(key);
    }
    if keys.is_empty() {
        return Err(SearchError::ContentProjection);
    }
    if !pending_separator.is_empty() {
        // No following glyph exists in this unit. A zero-width derived
        // endpoint closes the exact separator join without claiming display.
        let sentinel = dense_key(projection.fragments.len())?;
        joins.push(SearchTextJoin::RenderSeparator {
            text: pending_separator,
        });
        projection.fragments.push(SearchFragment {
            key: sentinel,
            text: String::new(),
            source: render_derived_source(),
        });
        keys.push(sentinel);
    }
    projection.units.push(SearchTextUnit {
        key: unit_key,
        fragments: keys,
        joins,
    });
    Ok(VisibleProjection {
        unit: unit_key,
        start_scalar: searchable.text[unit_range.start..matched_range.start]
            .chars()
            .count() as u64,
        end_scalar: searchable.text[unit_range.start..matched_range.end]
            .chars()
            .count() as u64,
        display_slices: slices,
    })
}

fn source_for_visible_part(
    part: &VisiblePart<'_>,
    matched_start: usize,
    origins: &FlowOrigins,
    tldr_base: Option<usize>,
) -> Result<SearchFragmentSource, SearchError> {
    if part.visible.end <= matched_start {
        // One real glyph may precede a matched leading render break. It
        // closes the response-local join but asserts no match provenance.
        return Ok(render_derived_source());
    }
    if let Some(base) = tldr_base.filter(|base| part.visible.start >= *base) {
        let start = part.visible.start - base;
        let end = part.visible.end - base;
        return Ok(SearchFragmentSource::Tldr(SearchTldrFragmentSource {
            kind: SearchTldrSourceKind::Tldr,
            path: "0".to_owned(),
            start_byte: u64::try_from(start).map_err(|_| SearchError::ResourceLimit)?,
            end_byte: u64::try_from(end).map_err(|_| SearchError::ResourceLimit)?,
        }));
    }
    if let Some((location, start_byte, end_byte)) = part
        .markdown
        .clone()
        .and_then(|markdown| origins.locate(markdown, part.text))
    {
        return Ok(SearchFragmentSource::Flow(SearchFlowFragmentSource {
            kind: SearchFlowSourceKind::Flow,
            location,
            start_byte,
            end_byte,
        }));
    }
    Ok(render_derived_source())
}

fn render_derived_source() -> SearchFragmentSource {
    SearchFragmentSource::RenderDerived(SearchRenderDerivedFragmentSource {
        kind: SearchRenderDerivedSourceKind::RenderDerived,
    })
}

fn dense_key(index: usize) -> Result<NonZeroU32, SearchError> {
    let one = index.checked_add(1).ok_or(SearchError::ResourceLimit)?;
    NonZeroU32::new(u32::try_from(one).map_err(|_| SearchError::ResourceLimit)?)
        .ok_or(SearchError::ResourceLimit)
}

// Count-only scans still reject hidden source-map anchors without constructing
// per-hit presentation strings or copies of the complete searched document.
fn occurrence_is_presented(range: Range<usize>, lines: &LineIndex) -> bool {
    let start = lines.line_index_at_byte(range.start);
    let end = lines.line_index_at_byte(range.end.saturating_sub(1));
    (start..=end).any(|line_index| {
        let line_start = lines.start(line_index);
        let line_end = lines.presented_end(line_index);
        let intersection = range.start.max(line_start)..range.end.min(line_end);
        intersection.start < intersection.end
            && lines.has_presented_range(
                line_index,
                intersection.start - line_start..intersection.end - line_start,
            )
    })
}
