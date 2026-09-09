//! Global search pagination over validated, already-loaded snapshots.
use super::{QueryScopeView, ScopeExecutionError};
use mant_protocol::{ScopeSearch, ScopedSearchDocument, SearchQuery};

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
        let local = plan
            .execute(bundle, remaining_skip, remaining_take.max(1))
            .map_err(ScopeExecutionError::Search)?;
        total = total.saturating_add(local.total);
        remaining_skip = remaining_skip.saturating_sub(local.total);
        if remaining_take == 0 || local.matches.is_empty() {
            continue;
        }
        let mut local = local;
        let mut hits = std::mem::take(&mut local.matches);
        if u32::try_from(hits.len()).unwrap_or(u32::MAX) > remaining_take {
            hits.truncate(usize::try_from(remaining_take).unwrap_or(usize::MAX));
        }
        for hit in &mut hits {
            hit.ordinal = document_ordinal_base.saturating_add(hit.ordinal);
        }
        remaining_take =
            remaining_take.saturating_sub(u32::try_from(hits.len()).unwrap_or(u32::MAX));
        groups.push(ScopedSearchDocument {
            address: scoped.address.clone(),
            depth: scoped.depth,
            render: local.render,
            matches: hits,
        });
    }
    let returned = query.limit.saturating_sub(remaining_take);
    let end = query.offset.saturating_add(returned);
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
