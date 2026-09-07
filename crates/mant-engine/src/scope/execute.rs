//! Scope execute: preserve request-local ownership and source order.
use super::{
    LoadedDocumentScope, ProjectionError, ScopeQueryError, ScopeQueryResult, ScopeSearch,
    ScopedExplanation, ScopedQueryFailure, ScopedSearchDocument, SearchQuery,
    select_explanation_with_text_hint,
};

pub(super) fn execute_scope_explain(loaded: &LoadedDocumentScope, entry: &str) -> ScopeQueryResult {
    let mut matches = Vec::new();
    let mut missed = 0_u32;
    let mut failures = Vec::new();
    for (scoped, bundle) in loaded.scope.documents.iter().zip(&loaded.documents) {
        match select_explanation_with_text_hint(bundle, entry) {
            Ok(excerpt) => matches.push(ScopedExplanation {
                address: scoped.address.clone(),
                depth: scoped.depth,
                excerpt,
            }),
            Err(ProjectionError::UnknownSelector { .. }) => {
                missed = missed.saturating_add(1);
            }
            Err(error) => failures.push(ScopedQueryFailure {
                address: scoped.address.clone(),
                reason: error.to_string(),
            }),
        }
    }
    ScopeQueryResult::Explain {
        entry: entry.to_owned(),
        matches,
        missed,
        failures,
    }
}

pub(super) fn execute_scope_search(
    loaded: &LoadedDocumentScope,
    query: &SearchQuery,
) -> Result<ScopeQueryResult, ScopeQueryError> {
    let plan = crate::search::SearchPlan::new(query).map_err(ScopeQueryError::Search)?;
    let mut total = 0_u32;
    let mut remaining_skip = query.offset;
    let mut remaining_take = query.limit;
    let mut groups = Vec::new();
    for (scoped, bundle) in loaded.scope.documents.iter().zip(&loaded.documents) {
        let document_ordinal_base = total;
        let local = plan
            .execute(bundle, remaining_skip, remaining_take.max(1))
            .map_err(ScopeQueryError::Search)?;
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
    Ok(ScopeQueryResult::Search {
        search: ScopeSearch {
            query: query.clone(),
            total,
            returned,
            offset: query.offset,
            truncated: end < total,
            next_offset: (end < total).then_some(end),
            documents: groups,
        },
    })
}
