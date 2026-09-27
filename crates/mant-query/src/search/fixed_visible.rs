//! Search final Fixed bytes and native-proven text joins, never a Markdown round trip.
//!
//! One final run can be mentioned by a section, owner, and region. The most
//! specific owner claims it once; a copied ancestor selection cannot create a
//! second occurrence. Native `TextJoin` evidence is retained only across two
//! adjacent, fully surviving parts of the same direct selection.

use std::{num::NonZeroU32, ops::Range};

use grep_matcher::Matcher;
use mant_codec::encode::EncodeError;
use mant_ir::{
    DisplayRole, DocumentBodyRef, FixedBody, FixedSectionReader, OutputSlice, SourceSpan, TextJoin,
    TextSelection,
};
use mant_protocol::{
    MAX_SEARCH_DIAGNOSTICS, MAX_SEARCH_PRESENTATION_BYTES, OutlineNodeReference, OutlineTrail,
    QuerySearch, SearchContentProjection, SearchContextLine, SearchDisplaySlice,
    SearchFixedFragmentSource, SearchFragment, SearchFragmentSource, SearchLocation, SearchMatch,
    SearchQuery, SearchRender, SearchRenderFormat, SearchRenderSchema, SearchRenderScope,
    SearchSchema, SearchScope, SearchTextJoin, SearchTextUnit,
};

use super::{
    SearchError,
    mapping::LineIndex,
    plan::{empty_match_error, matcher_error, non_utf8_pattern_error},
};
use crate::ResolvedContent;

mod artifact;
mod candidates;
#[cfg(test)]
mod tests;
pub(crate) mod units;
mod visible;

use crate::fixed_navigation::root_trail;
use artifact::search_markdown;
pub(crate) use candidates::SelectionKind;
pub(crate) use units::{FixedUnitPart, FixedVisibleUnit, FixedVisibleUnits};
use visible::{charge_presentation, search_visible};

const MAX_FIXED_SEARCH_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_FIXED_SCAN_BYTES: usize = 128 * 1024 * 1024;

pub(super) fn search_with_matcher(
    query: &ResolvedContent,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
    let document = query.document.as_ref().ok_or(SearchError::MissingContent)?;
    let DocumentBodyRef::Fixed(fixed) = document.body() else {
        return Err(SearchError::MissingContent);
    };
    fixed
        .validate()
        .map_err(|error| SearchError::InvalidFixed(EncodeError::InvalidFixed(error)))?;
    mant_ir::validate_document_sources(document)
        .map_err(|error| SearchError::InvalidFixed(EncodeError::InvalidFixedSource(error)))?;
    match request.scope {
        SearchScope::Visible if query.tldr.is_some() => {
            search_mixed_visible(query, fixed, request, matcher)
        }
        SearchScope::Visible => search_visible(query, fixed, request, matcher),
        SearchScope::Markdown => search_markdown(query, request, matcher),
    }
}

fn search_mixed_visible(
    query: &ResolvedContent,
    fixed: &FixedBody,
    request: &SearchQuery,
    matcher: &grep_regex::RegexMatcher,
) -> Result<QuerySearch, SearchError> {
    // TLDR precedes the primary Fixed manual in the canonical query snapshot.
    // Reuse R02a's Flow-visible extractor only for that independent arm; no
    // Markdown export of the Fixed body participates in visible matching.
    let tldr_only = ResolvedContent {
        label: query.label.clone(),
        address: None,
        document: None,
        tldr: query.tldr.clone(),
    };
    let mut quick = super::render_compat::search_with_matcher(&tldr_only, request, matcher)?;
    let remaining = request
        .limit
        .checked_sub(quick.returned)
        .ok_or(SearchError::ResourceLimit)?;
    let fixed_request = SearchQuery {
        offset: if remaining == 0 {
            u32::MAX
        } else {
            request.offset.saturating_sub(quick.total)
        },
        limit: remaining.max(1),
        ..request.clone()
    };
    let mut manual = search_visible(query, fixed, &fixed_request, matcher)?;
    let total = quick
        .total
        .checked_add(manual.total)
        .ok_or(SearchError::ResourceLimit)?;
    for matched in &mut manual.matches {
        matched.ordinal = matched
            .ordinal
            .checked_add(quick.total)
            .ok_or(SearchError::ResourceLimit)?;
    }
    let mut projection = quick
        .content_projection
        .take()
        .unwrap_or(SearchContentProjection {
            fragments: Vec::new(),
            units: Vec::new(),
        });
    if let Some(manual_projection) = manual.content_projection.take() {
        append_projection(&mut projection, manual_projection, &mut manual.matches)?;
    }
    quick.matches.extend(manual.matches);
    let mut presentation_bytes = 0usize;
    for matched in &quick.matches {
        charge_presentation(
            &mut presentation_bytes,
            &matched.matched_text,
            &matched.preview,
            &matched.context,
        )?;
    }
    let content_projection = if quick.matches.is_empty() {
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
        quick.matches,
        content_projection,
    )
}

fn append_projection(
    destination: &mut SearchContentProjection,
    source: SearchContentProjection,
    matches: &mut [SearchMatch],
) -> Result<(), SearchError> {
    let fragment_base =
        u32::try_from(destination.fragments.len()).map_err(|_| SearchError::ResourceLimit)?;
    let unit_base =
        u32::try_from(destination.units.len()).map_err(|_| SearchError::ResourceLimit)?;
    let shifted = |key: NonZeroU32, base: u32| {
        NonZeroU32::new(
            key.get()
                .checked_add(base)
                .ok_or(SearchError::ResourceLimit)?,
        )
        .ok_or(SearchError::ResourceLimit)
    };
    for mut fragment in source.fragments {
        fragment.key = shifted(fragment.key, fragment_base)?;
        destination.fragments.push(fragment);
    }
    for mut unit in source.units {
        unit.key = shifted(unit.key, unit_base)?;
        for fragment in &mut unit.fragments {
            *fragment = shifted(*fragment, fragment_base)?;
        }
        destination.units.push(unit);
    }
    for matched in matches {
        let SearchLocation::VisibleFixed { unit, .. } = &mut matched.location else {
            return Err(SearchError::ContentProjection);
        };
        *unit = shifted(*unit, unit_base)?;
        for slice in &mut matched.display_slices {
            slice.fragment = shifted(slice.fragment, fragment_base)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finish_result(
    query: &ResolvedContent,
    request: &SearchQuery,
    render_schema: SearchRenderSchema,
    render_format: SearchRenderFormat,
    line_count: u32,
    total: u32,
    matches: Vec<SearchMatch>,
    content_projection: Option<SearchContentProjection>,
) -> Result<QuerySearch, SearchError> {
    let returned = u32::try_from(matches.len()).map_err(|_| SearchError::ResourceLimit)?;
    let consumed = request
        .offset
        .checked_add(returned)
        .ok_or(SearchError::ResourceLimit)?;
    let truncated = returned != 0 && consumed < total;
    let document = query.document.as_ref().ok_or(SearchError::MissingContent)?;
    let diagnostics = document.projection_diagnostics();
    if diagnostics.len() > MAX_SEARCH_DIAGNOSTICS {
        return Err(SearchError::ResourceLimit);
    }
    let result = QuerySearch {
        schema: SearchSchema::V0Dot12,
        label: query.label.clone(),
        source_context: Some(mant_protocol::SourceContext::from(document)),
        meta: Some(document.meta.clone()),
        content_projection,
        query: request.clone(),
        render: SearchRender {
            schema: render_schema,
            format: render_format,
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
        matches,
    };
    result
        .validate()
        .map_err(|_| SearchError::ContentProjection)?;
    Ok(result)
}
