//! Global search pagination over validated, already-loaded snapshots.
use super::{QueryScopeView, ScopeExecutionError};
use crate::search::SearchError;
use mant_protocol::{
    MAX_SEARCH_PRESENTATION_BYTES, ScopeSearch, ScopeSearchSchema, ScopedSearchCoverage,
    ScopedSearchDocument, SearchContentProjection, SearchMatch, SearchQuery, SearchTextJoin,
};

#[cfg(test)]
mod tests;

/// Search existing snapshots with one global result cursor.
/// No loading or parsing occurs; coverage remains in the supplied graph.
///
/// # Errors
/// Returns invalid search bounds or matcher errors.
pub fn search_scope(
    input: QueryScopeView<'_>,
    query: &SearchQuery,
) -> Result<ScopeSearch, ScopeExecutionError> {
    let plan = crate::search::SearchPlan::new(query).map_err(ScopeExecutionError::Search)?;
    let mut total = 0_u32;
    let mut remaining_skip = query.offset;
    let mut remaining_take = query.limit;
    let mut groups = Vec::new();
    let mut coverage_by_document = Vec::new();
    let mut presentation_bytes = 0usize;
    let mut projection_bytes = 0usize;
    let mut metadata_bytes = 0usize;
    for (scoped, bundle) in input.iter() {
        let document_ordinal_base = total;
        let coverage = coverage_for(scoped, bundle)?;
        accumulate_metadata_bytes(&coverage, &mut metadata_bytes)?;
        coverage_by_document.push(coverage);
        if remaining_take == 0 {
            let count = plan.count(bundle).map_err(ScopeExecutionError::Search)?;
            total = total
                .checked_add(count)
                .ok_or(ScopeExecutionError::Search(SearchError::ResourceLimit))?;
            continue;
        }
        let local = plan
            .execute(bundle, remaining_skip, remaining_take)
            .map_err(ScopeExecutionError::Search)?;
        total = total
            .checked_add(local.total)
            .ok_or(ScopeExecutionError::Search(SearchError::ResourceLimit))?;
        // Skipping a document with fewer hits than the remaining cursor is a
        // deliberate clamp, not a result-count overflow.
        remaining_skip = remaining_skip.saturating_sub(local.total);
        if local.matches.is_empty() {
            continue;
        }
        let mut local = local;
        let mut hits = std::mem::take(&mut local.matches);
        let hit_count = u32::try_from(hits.len())
            .map_err(|_| ScopeExecutionError::Search(SearchError::ResourceLimit))?;
        debug_assert!(hit_count <= remaining_take);
        for hit in &mut hits {
            hit.ordinal = document_ordinal_base
                .checked_add(hit.ordinal)
                .ok_or(ScopeExecutionError::Search(SearchError::ResourceLimit))?;
        }
        accumulate_response_bytes(
            &hits,
            local.content_projection.as_ref(),
            &mut presentation_bytes,
            &mut projection_bytes,
        )?;
        if presentation_bytes > MAX_SEARCH_PRESENTATION_BYTES
            || projection_bytes > MAX_SEARCH_PRESENTATION_BYTES
        {
            return Err(ScopeExecutionError::Search(SearchError::ResourceLimit));
        }
        if let Some(context) = &local.source_context {
            // The hit group's source table is serialized a second time,
            // independently of the scanned-document coverage entry.
            accumulate_metadata_bytes(context, &mut metadata_bytes)?;
        }
        remaining_take = remaining_take
            .checked_sub(hit_count)
            .ok_or(ScopeExecutionError::Search(SearchError::ResourceLimit))?;
        groups.push(ScopedSearchDocument {
            address: scoped.address.clone(),
            depth: scoped.depth,
            source_context: local.source_context,
            content_projection: local.content_projection,
            render: local.render,
            matches: hits,
        });
    }
    let returned = query
        .limit
        .checked_sub(remaining_take)
        .ok_or(ScopeExecutionError::Search(SearchError::ResourceLimit))?;
    let end = query
        .offset
        .checked_add(returned)
        .ok_or(ScopeExecutionError::Search(SearchError::ResourceLimit))?;
    let result = ScopeSearch {
        schema: ScopeSearchSchema::V0Dot12,
        query: query.clone(),
        total,
        returned,
        offset: query.offset,
        truncated: returned != 0 && end < total,
        next_offset: (returned != 0 && end < total).then_some(end),
        semantics_complete: coverage_by_document
            .iter()
            .all(|entry| entry.semantics_complete),
        coverage_by_document,
        documents: groups,
    };
    result
        .validate()
        .map_err(|_| ScopeExecutionError::Search(SearchError::ContentProjection))?;
    Ok(result)
}

fn accumulate_metadata_bytes<T: serde::Serialize>(
    value: &T,
    total: &mut usize,
) -> Result<(), ScopeExecutionError> {
    serde_json::to_writer(ResponseMetadataWriter(total), value)
        .map_err(|_| ScopeExecutionError::Search(SearchError::ResourceLimit))
}

struct ResponseMetadataWriter<'a>(&'a mut usize);

impl std::io::Write for ResponseMetadataWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        *self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|total| *total <= MAX_SEARCH_PRESENTATION_BYTES)
            .ok_or_else(|| std::io::Error::other("scope search metadata byte budget"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn coverage_for(
    scoped: &mant_protocol::ScopedDocument,
    bundle: &mant_ir::ResolvedContent,
) -> Result<ScopedSearchCoverage, ScopeExecutionError> {
    let diagnostics = bundle
        .document
        .as_ref()
        .map_or_else(Vec::new, |document| document.diagnostics.clone());
    if diagnostics.len() > mant_protocol::MAX_SEARCH_DIAGNOSTICS {
        return Err(ScopeExecutionError::Search(SearchError::ResourceLimit));
    }
    Ok(ScopedSearchCoverage {
        address: scoped.address.clone(),
        depth: scoped.depth,
        source_context: bundle
            .document
            .as_ref()
            .map(mant_protocol::SourceContext::from),
        semantics_complete: mant_ir::semantics_complete(&diagnostics),
        coverage_details_omitted: 0,
        diagnostics,
    })
}

fn accumulate_response_bytes(
    hits: &[SearchMatch],
    projection: Option<&SearchContentProjection>,
    presentation_bytes: &mut usize,
    projection_bytes: &mut usize,
) -> Result<(), ScopeExecutionError> {
    let limit = || ScopeExecutionError::Search(SearchError::ResourceLimit);
    for hit in hits {
        *presentation_bytes = presentation_bytes
            .checked_add(hit.matched_text.len())
            .and_then(|value| value.checked_add(hit.preview.len()))
            .ok_or_else(limit)?;
        for line in &hit.context {
            *presentation_bytes = presentation_bytes
                .checked_add(line.text.len())
                .ok_or_else(limit)?;
        }
    }
    if let Some(projection) = projection {
        for fragment in &projection.fragments {
            *projection_bytes = projection_bytes
                .checked_add(fragment.text.len())
                .ok_or_else(limit)?;
        }
        for unit in &projection.units {
            for join in &unit.joins {
                if let SearchTextJoin::AuthoredSeparator { text }
                | SearchTextJoin::RenderSeparator { text } = join
                {
                    *projection_bytes =
                        projection_bytes.checked_add(text.len()).ok_or_else(limit)?;
                }
            }
        }
    }
    Ok(())
}
