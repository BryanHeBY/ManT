//! Match and project native-proven Fixed visible selections.
#![allow(clippy::similar_names)] // Matcher, match and matches are distinct search-domain terms.

use super::units::append_bounded;
use super::{
    FixedBody, FixedSectionReader, FixedUnitPart, FixedVisibleUnit, FixedVisibleUnits,
    MAX_FIXED_SCAN_BYTES, MAX_FIXED_SEARCH_BYTES, MAX_SEARCH_PRESENTATION_BYTES, Matcher,
    NonZeroU32, QuerySearch, Range, ResolvedContent, SearchContentProjection, SearchContextLine,
    SearchDisplaySlice, SearchError, SearchFixedFragmentSource, SearchFragment,
    SearchFragmentSource, SearchLocation, SearchMatch, SearchQuery, SearchRenderFormat,
    SearchRenderSchema, SearchTextJoin, SearchTextUnit, TextJoin, empty_match_error, finish_result,
    matcher_error, non_utf8_pattern_error,
};

pub(super) fn search_visible(
    query: &ResolvedContent,
    fixed: &FixedBody,
    reader: &FixedSectionReader<'_>,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
    let units = FixedVisibleUnits::new(fixed)?;
    let mut projection = SearchContentProjection {
        fragments: Vec::new(),
        units: Vec::new(),
    };
    let mut matches = Vec::new();
    let mut total = 0_u32;
    let mut presentation_bytes = 0usize;
    let mut scan_bytes = 0usize;
    for unit in units.iter() {
        search_unit(
            fixed,
            reader,
            unit,
            request,
            matcher,
            &mut total,
            &mut matches,
            &mut projection,
            &mut presentation_bytes,
            &mut scan_bytes,
        )?;
    }
    let content_projection = if matches.is_empty() {
        None
    } else {
        projection
            .validate()
            .map_err(|_| SearchError::ResourceLimit)?;
        Some(projection)
    };
    finish_result(
        query,
        request,
        SearchRenderSchema::Fixed,
        SearchRenderFormat::FixedVisible,
        u32::try_from(fixed.surface.rows.len()).map_err(|_| SearchError::ResourceLimit)?,
        total,
        matches,
        content_projection,
    )
}

#[allow(clippy::too_many_arguments)] // One bounded unit scan passes borrowed state and two budgets.
fn search_unit(
    fixed: &FixedBody,
    reader: &FixedSectionReader<'_>,
    unit: FixedVisibleUnit<'_>,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
    total: &mut u32,
    matches: &mut Vec<SearchMatch>,
    projection: &mut SearchContentProjection,
    presentation_bytes: &mut usize,
    scan_bytes: &mut usize,
) -> Result<(), SearchError> {
    let logical = unit.materialize(fixed)?;
    let text = &logical.text;
    let parts = &logical.parts;
    *scan_bytes = scan_bytes
        .checked_add(text.len())
        .ok_or(SearchError::ResourceLimit)?;
    if *scan_bytes > MAX_FIXED_SCAN_BYTES {
        return Err(SearchError::ResourceLimit);
    }
    let mut callback_error = None;
    let mut invalid_utf8 = false;
    let mut zero_width = false;
    matcher
        .find_iter(text.as_bytes(), |found| {
            if found.start() == found.end() {
                zero_width = true;
                return false;
            }
            if !text.is_char_boundary(found.start()) || !text.is_char_boundary(found.end()) {
                invalid_utf8 = true;
                return false;
            }
            let Some(next) = total.checked_add(1) else {
                callback_error = Some(SearchError::ResourceLimit);
                return false;
            };
            *total = next;
            if next <= request.offset || matches.len() >= request.limit as usize {
                return true;
            }
            match make_visible_match(
                fixed,
                reader,
                text,
                parts,
                found.start()..found.end(),
                next,
                request.context_lines,
                projection,
                presentation_bytes,
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
        Err(error)
    } else if invalid_utf8 {
        Err(non_utf8_pattern_error())
    } else if zero_width {
        Err(empty_match_error())
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // A single occurrence closes typed coordinates, slices and preview.
fn make_visible_match(
    fixed: &FixedBody,
    reader: &FixedSectionReader<'_>,
    text: &str,
    parts: &[FixedUnitPart<'_>],
    found: Range<usize>,
    ordinal: u32,
    context_lines: u16,
    projection: &mut SearchContentProjection,
    presentation_bytes: &mut usize,
) -> Result<SearchMatch, SearchError> {
    // Retain only the parts necessary to close this occurrence. A consumed
    // native separator still needs both neighboring real run fragments.
    let first = parts
        .partition_point(|part| part.text_range.start <= found.start)
        .saturating_sub(1);
    let last = parts
        .partition_point(|part| part.text_range.end < found.end)
        .min(parts.len() - 1);
    let selected = &parts[first..=last];
    let unit_key = dense_key(projection.units.len())?;
    let mut keys = Vec::with_capacity(selected.len());
    let mut joins = Vec::with_capacity(selected.len().saturating_sub(1));
    let mut display_slices = Vec::new();
    let mut unit_start = selected[0].text_range.start;
    for (index, part) in selected.iter().enumerate() {
        let piece = part.piece;
        let mut clip = part.text_range.clone();
        if index == 0 && found.start < clip.end {
            clip.start = clip.start.max(found.start);
        }
        if index + 1 == selected.len() && found.end > clip.start {
            clip.end = clip.end.min(found.end);
        }
        // A separator-only occurrence needs a tiny real endpoint on either
        // side. The endpoint is context, not a displayed part of the hit.
        if clip.start >= clip.end {
            if index == 0 {
                clip.start = text[..clip.end]
                    .char_indices()
                    .next_back()
                    .map_or(clip.start, |(start, _)| start);
            } else {
                clip.end = text[clip.start..]
                    .chars()
                    .next()
                    .map_or(clip.end, |scalar| clip.start + scalar.len_utf8());
            }
        }
        let fragment_text = text
            .get(clip.clone())
            .ok_or(SearchError::ContentProjection)?;
        if fragment_text.is_empty() {
            return Err(SearchError::ContentProjection);
        }
        if index == 0 {
            unit_start = clip.start;
        } else {
            joins.push(
                match piece
                    .join_before
                    .as_ref()
                    .ok_or(SearchError::ContentProjection)?
                {
                    TextJoin::DirectContact => SearchTextJoin::DirectContact,
                    TextJoin::AuthoredSeparator(separator) => SearchTextJoin::AuthoredSeparator {
                        text: separator.clone(),
                    },
                    TextJoin::GeneratedSeparator(separator) => SearchTextJoin::GeneratedSeparator {
                        text: separator.clone(),
                    },
                    TextJoin::HardBoundary | TextJoin::Unknown => {
                        return Err(SearchError::ContentProjection);
                    }
                },
            );
        }
        let key = dense_key(projection.fragments.len())?;
        let fragment_offset = clip.start - part.text_range.start;
        let start_byte = piece
            .slice
            .start_byte
            .checked_add(u64::try_from(fragment_offset).map_err(|_| SearchError::ResourceLimit)?)
            .ok_or(SearchError::ResourceLimit)?;
        let end_byte = start_byte
            .checked_add(
                u64::try_from(fragment_text.len()).map_err(|_| SearchError::ResourceLimit)?,
            )
            .ok_or(SearchError::ResourceLimit)?;
        let run = &fixed.surface.runs[(piece.slice.run.get() - 1) as usize];
        projection.fragments.push(SearchFragment {
            key,
            text: fragment_text.to_owned(),
            source: SearchFragmentSource::Fixed(SearchFixedFragmentSource {
                row: run.row,
                run: piece.slice.run,
                start_byte,
                end_byte,
            }),
        });
        let overlap = found.start.max(clip.start)..found.end.min(clip.end);
        if overlap.start < overlap.end {
            display_slices.push(SearchDisplaySlice {
                fragment: key,
                start_scalar: fragment_text[..overlap.start - clip.start].chars().count() as u64,
                end_scalar: fragment_text[..overlap.end - clip.start].chars().count() as u64,
            });
        }
        keys.push(key);
    }
    projection.units.push(SearchTextUnit {
        key: unit_key,
        fragments: keys,
        joins,
    });
    let start_scalar = text[unit_start..found.start].chars().count() as u64;
    let end_scalar = text[unit_start..found.end].chars().count() as u64;
    let first_piece = selected[0].piece;
    let last_piece = selected[selected.len() - 1].piece;
    let first_row = fixed.surface.runs[(first_piece.slice.run.get() - 1) as usize].row;
    let last_row = fixed.surface.runs[(last_piece.slice.run.get() - 1) as usize].row;
    let preview = row_text(fixed, first_row)?;
    let context = row_context(fixed, first_row, last_row, context_lines)?;
    let matched_text = text
        .get(found)
        .ok_or(SearchError::ContentProjection)?
        .to_owned();
    charge_presentation(presentation_bytes, &matched_text, &preview, &context)?;
    Ok(SearchMatch {
        ordinal,
        outline: crate::fixed_navigation::section_trail(reader, first_piece.section)
            .ok_or(SearchError::ContentProjection)?,
        matched_text,
        location: SearchLocation::VisibleFixed {
            unit: unit_key,
            start_scalar,
            end_scalar,
        },
        display_slices,
        node_source: first_piece.source,
        preview,
        context,
    })
}

fn dense_key(index: usize) -> Result<NonZeroU32, SearchError> {
    let next = index.checked_add(1).ok_or(SearchError::ResourceLimit)?;
    NonZeroU32::new(u32::try_from(next).map_err(|_| SearchError::ResourceLimit)?)
        .ok_or(SearchError::ResourceLimit)
}

fn row_text(fixed: &FixedBody, key: NonZeroU32) -> Result<String, SearchError> {
    let row = fixed
        .surface
        .rows
        .get((key.get() - 1) as usize)
        .ok_or(SearchError::ContentProjection)?;
    let first = (row.first_run.get() - 1) as usize;
    let end = first
        .checked_add(row.run_count as usize)
        .ok_or(SearchError::ResourceLimit)?;
    let mut text = String::new();
    let mut column = 0_u32;
    for run in fixed
        .surface
        .runs
        .get(first..end)
        .ok_or(SearchError::ContentProjection)?
    {
        let gap = run
            .column
            .checked_sub(column)
            .ok_or(SearchError::ContentProjection)?;
        append_spaces(&mut text, gap)?;
        append_bounded(
            &mut text,
            fixed
                .surface
                .run_text(run.key)
                .ok_or(SearchError::ContentProjection)?,
        )?;
        column = run
            .column
            .checked_add(run.width)
            .ok_or(SearchError::ResourceLimit)?;
    }
    append_spaces(
        &mut text,
        row.column_count
            .checked_sub(column)
            .ok_or(SearchError::ContentProjection)?,
    )?;
    Ok(text)
}

fn append_spaces(text: &mut String, count: u32) -> Result<(), SearchError> {
    let length = usize::try_from(count).map_err(|_| SearchError::ResourceLimit)?;
    if text
        .len()
        .checked_add(length)
        .ok_or(SearchError::ResourceLimit)?
        > MAX_FIXED_SEARCH_BYTES
    {
        return Err(SearchError::ResourceLimit);
    }
    text.extend(std::iter::repeat_n(' ', length));
    Ok(())
}

fn row_context(
    fixed: &FixedBody,
    start: NonZeroU32,
    end: NonZeroU32,
    radius: u16,
) -> Result<Vec<SearchContextLine>, SearchError> {
    if radius == 0 {
        return Ok(Vec::new());
    }
    let first = start.get().saturating_sub(u32::from(radius)).max(1);
    let last = end
        .get()
        .saturating_add(u32::from(radius))
        .min(u32::try_from(fixed.surface.rows.len()).map_err(|_| SearchError::ResourceLimit)?);
    (first..=last)
        .map(|line| {
            let key = NonZeroU32::new(line).ok_or(SearchError::ContentProjection)?;
            Ok(SearchContextLine {
                line,
                text: row_text(fixed, key)?,
                matched: start.get() <= line && line <= end.get(),
            })
        })
        .collect()
}

pub(super) fn charge_presentation(
    bytes: &mut usize,
    matched_text: &str,
    preview: &str,
    context: &[SearchContextLine],
) -> Result<(), SearchError> {
    *bytes = bytes
        .checked_add(matched_text.len())
        .and_then(|value| value.checked_add(preview.len()))
        .ok_or(SearchError::ResourceLimit)?;
    for line in context {
        *bytes = bytes
            .checked_add(line.text.len())
            .ok_or(SearchError::ResourceLimit)?;
    }
    if *bytes > MAX_SEARCH_PRESENTATION_BYTES {
        return Err(SearchError::ResourceLimit);
    }
    Ok(())
}
