//! Global classification precedes pagination; source reports stay in BFS order.
use super::{
    collection_plan,
    materialize::{Budget, omitted, outcome},
};
use mant_protocol::{
    EvidenceCounts, EvidenceOrder, ScopeExplanation, ScopedExplanation, ScopedQueryFailure,
};

#[cfg(test)]
mod tests;

pub(crate) fn explain(
    loaded: &crate::scope::LoadedDocumentScope,
    query: &mant_protocol::ExplanationQuery,
) -> Result<ScopeExplanation, crate::ScopeQueryError> {
    super::validate_explanation_query(query).map_err(crate::ScopeQueryError::Explanation)?;
    let mut plans = Vec::new();
    let mut sources = Vec::new();
    let mut failures = Vec::new();
    for (source, content) in loaded.scope.documents.iter().zip(&loaded.documents) {
        match collection_plan(content, query.entry.trim()) {
            Ok(plan) => {
                sources.push(source);
                plans.push(plan);
            }
            Err(error) => failures.push(ScopedQueryFailure {
                address: source.address.clone(),
                reason: error.to_string(),
            }),
        }
    }
    if plans.is_empty() {
        return Err(crate::ScopeQueryError::NoResolvedDocuments {
            reasons: failures.iter().map(|f| f.reason.clone()).collect(),
        });
    }
    let order = ordered_candidates(&plans);
    let total = u32::try_from(order.len()).expect("bounded documents and candidates");
    let mut counts = EvidenceCounts::default();
    let mut local_counts = vec![EvidenceCounts::default(); plans.len()];
    let mut budget = Budget(query.options.content_bytes as usize);
    let mut selection = Vec::new();
    for (ordinal, ((class, doc, _), index)) in order.iter().copied().enumerate() {
        let selected = ordinal >= query.options.offset as usize
            && selection.len() < query.options.limit as usize;
        counts.record(class, selected);
        local_counts[doc].record(class, selected);
        if selected {
            selection.push((
                doc,
                index,
                u32::try_from(ordinal).expect("bounded candidates"),
            ));
        }
    }
    let page = super::page::materialize(&plans, &selection, &mut budget);
    let evidence = page.evidence;
    let mut supports = page.pools;
    let mut copy_omitted = vec![false; plans.len()];
    for result in &evidence {
        copy_omitted[result.document_index] |= omitted(&result.evidence);
    }
    let mut truncation = mant_protocol::ExplanationTruncation::default();
    let documents = plans
        .into_iter()
        .enumerate()
        .map(|(index, plan)| {
            let report = source_report(
                plan,
                sources[index],
                local_counts[index],
                copy_omitted[index],
                std::mem::take(&mut supports[index].values),
            );
            truncation.candidates |= report.truncation.candidates;
            truncation.relations |= report.truncation.relations;
            truncation.content |= report.truncation.content;
            report
        })
        .collect();
    let returned = u32::try_from(evidence.len()).expect("bounded result page");
    let end = query.options.offset.saturating_add(returned);
    Ok(ScopeExplanation {
        query: query.clone(),
        order: EvidenceOrder::ClassThenSource,
        counts,
        outcome: outcome(total),
        total,
        returned,
        next_offset: (end < total).then_some(end),
        truncation,
        documents,
        evidence,
        failures,
    })
}

fn ordered_candidates(
    plans: &[super::plan::CollectionPlan<'_>],
) -> Vec<((mant_protocol::EvidenceClass, usize, usize), usize)> {
    let mut order = plans
        .iter()
        .enumerate()
        .flat_map(|(doc, plan)| {
            plan.candidates
                .iter()
                .enumerate()
                .map(move |(index, c)| ((c.class(), doc, c.order), index))
        })
        .collect::<Vec<_>>();
    order.sort_unstable_by_key(|(key, _)| *key);
    order
}

fn source_report(
    plan: super::plan::CollectionPlan<'_>,
    source: &mant_protocol::ScopedDocument,
    counts: EvidenceCounts,
    copy_omitted: bool,
    supports: Vec<mant_protocol::ExplanationSupport>,
) -> ScopedExplanation {
    let total = u32::try_from(plan.candidates.len()).expect("bounded candidates");
    let returned = mant_protocol::EvidenceClass::ALL
        .into_iter()
        .map(|c| counts.get(c).returned)
        .sum();
    let mut truncation = plan.truncation;
    truncation.content = copy_omitted;
    ScopedExplanation {
        supports,
        address: source.address.clone(),
        depth: source.depth,
        label: plan.content.label.clone(),
        producer: plan
            .content
            .document
            .as_ref()
            .map(mant_protocol::Producer::for_document),
        semantics_complete: crate::projection::semantics_complete(&plan.diagnostics),
        diagnostics: plan.diagnostics,
        outcome: outcome(total),
        total,
        returned,
        counts,
        truncation,
    }
}
