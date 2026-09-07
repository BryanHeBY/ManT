//! Scope execute: preserve request-local ownership and source order.
use super::{
    LoadedDocumentScope, ScopeQueryError, ScopeQueryResult, ScopeSearch, ScopedExplanation,
    ScopedQueryFailure, ScopedSearchDocument, SearchQuery,
};

pub(super) fn execute_scope_explain(
    loaded: &LoadedDocumentScope,
    query: &mant_protocol::ExplanationQuery,
) -> Result<ScopeQueryResult, ScopeQueryError> {
    let mut documents = Vec::new();
    let mut failures = Vec::new();
    let mut remaining = query.options;
    let mut total = 0_u32;
    let mut returned = 0_u32;
    let mut truncation = mant_protocol::ExplanationTruncation::default();
    for (scoped, bundle) in loaded.scope.documents.iter().zip(&loaded.documents) {
        let mut local = mant_protocol::ExplanationQuery {
            entry: query.entry.clone(),
            options: remaining,
        };
        if local.options.limit == 0 {
            local.options.limit = 1;
            local.options.offset = u32::MAX;
        }
        local.options.content_bytes = local.options.content_bytes.max(1);
        match crate::explanation::explain_with_usage(bundle, &local) {
            Ok((mut explanation, used)) => {
                // The scope owns one global cursor; local totals still report
                // each source's coverage, but must not advertise a competing cursor.
                explanation.next_offset = None;
                for evidence in &mut explanation.evidence {
                    evidence.ordinal = evidence.ordinal.saturating_add(total);
                }
                remaining.offset = remaining.offset.saturating_sub(explanation.total);
                remaining.limit = remaining.limit.saturating_sub(explanation.returned);
                remaining.content_bytes = remaining.content_bytes.saturating_sub(used);
                total = total.saturating_add(explanation.total);
                returned = returned.saturating_add(explanation.returned);
                truncation.candidates |= explanation.truncation.candidates;
                truncation.relations |= explanation.truncation.relations;
                truncation.content |= explanation.truncation.content;
                documents.push(ScopedExplanation {
                    address: scoped.address.clone(),
                    depth: scoped.depth,
                    explanation,
                });
            }
            Err(error) => failures.push(ScopedQueryFailure {
                address: scoped.address.clone(),
                reason: error.to_string(),
            }),
        }
    }
    if documents.is_empty() {
        return Err(ScopeQueryError::NoResolvedDocuments {
            reasons: failures.iter().map(|f| f.reason.clone()).collect(),
        });
    }
    let end = query.options.offset.saturating_add(returned);
    Ok(ScopeQueryResult::Explain {
        explanation: mant_protocol::ScopeExplanation {
            query: query.clone(),
            outcome: if total == 0 {
                mant_protocol::ExplanationOutcome::NoEvidence
            } else {
                mant_protocol::ExplanationOutcome::Evidence
            },
            total,
            returned,
            next_offset: (end < total).then_some(end),
            truncation,
            documents,
            failures,
        },
    })
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
