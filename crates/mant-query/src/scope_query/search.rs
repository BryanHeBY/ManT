//! Global search pagination over validated, already-loaded snapshots.
use super::{QueryScopeView, ScopeExecutionError};
use crate::search::SearchError;
use mant_protocol::{ScopeSearch, ScopedSearchDocument, SearchQuery};

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
    for (scoped, bundle) in input.iter() {
        let document_ordinal_base = total;
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
    Ok(ScopeSearch {
        query: query.clone(),
        total,
        returned,
        offset: query.offset,
        truncated: end < total,
        next_offset: (end < total).then_some(end),
        documents: groups,
    })
}
