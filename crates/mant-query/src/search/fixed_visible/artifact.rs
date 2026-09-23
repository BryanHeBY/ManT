//! Search the one canonical Fixed Markdown export as artifact bytes.
#![allow(clippy::similar_names)] // Matcher, match and matches are distinct search-domain terms.

use super::{
    LineIndex, Matcher, OutlineNodeReference, OutlineTrail, QuerySearch, Range, ResolvedContent,
    SearchContextLine, SearchError, SearchLocation, SearchMatch, SearchQuery, SearchRenderFormat,
    SearchRenderSchema, charge_presentation, empty_match_error, finish_result, matcher_error,
    non_utf8_pattern_error, root_trail,
};
use mant_codec::encode::{MarkdownNode, render_addressable_markdown};

pub(super) fn search_markdown(
    query: &ResolvedContent,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
    let artifact = render_addressable_markdown(query).map_err(SearchError::InvalidFixed)?;
    let markdown = artifact.text();
    let tldr_range = artifact
        .nodes()
        .iter()
        .find(|node| matches!(node.node(), MarkdownNode::Tldr))
        .map(mant_codec::encode::MarkdownNodeRange::range);
    let lines = LineIndex::with_anchors(markdown, Vec::new());
    let line_count = u32::try_from(lines.count()).map_err(|_| SearchError::ResourceLimit)?;
    let mut total = 0_u32;
    let mut matches = Vec::new();
    let mut presentation_bytes = 0usize;
    let mut callback_error = None;
    let mut invalid_utf8 = false;
    let mut zero_width = false;
    matcher
        .find_iter(markdown.as_bytes(), |found| {
            if found.start() == found.end() {
                zero_width = true;
                return false;
            }
            if !markdown.is_char_boundary(found.start()) || !markdown.is_char_boundary(found.end())
            {
                invalid_utf8 = true;
                return false;
            }
            let Some(next) = total.checked_add(1) else {
                callback_error = Some(SearchError::ResourceLimit);
                return false;
            };
            total = next;
            if next <= request.offset || matches.len() >= request.limit as usize {
                return true;
            }
            match markdown_match(
                markdown,
                &lines,
                found.start()..found.end(),
                tldr_range.as_ref(),
                next,
                request.context_lines,
                &mut presentation_bytes,
            ) {
                Ok(matched) => matches.push(matched),
                Err(error) => {
                    callback_error = Some(error);
                    return false;
                }
            }
            true
        })
        .map_err(matcher_error)?;
    if let Some(error) = callback_error {
        return Err(error);
    }
    if invalid_utf8 {
        return Err(non_utf8_pattern_error());
    }
    if zero_width {
        return Err(empty_match_error());
    }
    finish_result(
        query,
        request,
        SearchRenderSchema::Markdown,
        SearchRenderFormat::Markdown,
        line_count,
        total,
        matches,
        None,
    )
}

fn markdown_match(
    markdown: &str,
    lines: &LineIndex,
    found: Range<usize>,
    tldr_range: Option<&Range<usize>>,
    ordinal: u32,
    context_lines: u16,
    presentation_bytes: &mut usize,
) -> Result<SearchMatch, SearchError> {
    let start = lines.position(markdown, found.start);
    let end = lines.position(markdown, found.end);
    let end_line = lines.line_index_at_byte(found.end - 1);
    let preview = lines.presented_line(markdown, start.line_index).text;
    let context = if context_lines == 0 {
        Vec::new()
    } else {
        let first = start.line_index.saturating_sub(context_lines as usize);
        let last = end_line
            .saturating_add(context_lines as usize)
            .min(lines.count().saturating_sub(1));
        (first..=last)
            .map(|index| SearchContextLine {
                line: u32::try_from(index + 1).unwrap_or(u32::MAX),
                text: lines.presented_line(markdown, index).text,
                matched: start.line_index <= index && index <= end_line,
            })
            .collect()
    };
    let matched_text = markdown
        .get(found.clone())
        .ok_or(SearchError::ContentProjection)?
        .to_owned();
    charge_presentation(presentation_bytes, &matched_text, &preview, &context)?;
    Ok(SearchMatch {
        ordinal,
        outline: if tldr_range
            .is_some_and(|range| range.start <= found.start && found.end <= range.end)
        {
            OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::Tldr {
                    path: "0".into(),
                    id: "tldr".into(),
                    title: "TLDR QUICK REFERENCE".into(),
                },
            }
        } else {
            root_trail()
        },
        matched_text,
        location: SearchLocation::MarkdownArtifact {
            start_byte: u64::try_from(found.start).map_err(|_| SearchError::ResourceLimit)?,
            end_byte: u64::try_from(found.end).map_err(|_| SearchError::ResourceLimit)?,
            start_line: u32::try_from(start.line_index + 1)
                .map_err(|_| SearchError::ResourceLimit)?,
            start_column: u32::try_from(start.column).map_err(|_| SearchError::ResourceLimit)?,
            end_line: u32::try_from(end.line_index + 1).map_err(|_| SearchError::ResourceLimit)?,
            end_column: u32::try_from(end.column).map_err(|_| SearchError::ResourceLimit)?,
        },
        display_slices: Vec::new(),
        node_source: None,
        preview,
        context,
    })
}
