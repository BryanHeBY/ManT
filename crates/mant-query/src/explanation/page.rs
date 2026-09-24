//! One page budget schedule shared by single-document and scoped explanations.
use super::{
    ExplanationError,
    materialize::{self, Budget},
    plan::DocumentPlan,
    projection::ProjectionAdmission,
    support::Pool,
};
use mant_protocol::{EvidenceClass, ScopedExplanationEvidence};

pub(super) struct Page<'a> {
    pub evidence: Vec<ScopedExplanationEvidence>,
    pub pools: Vec<Pool>,
    pub projections: Vec<ProjectionAdmission<'a>>,
}

/// Selection contains (document, candidate, global ordinal). All direct facts
/// are reserved before any body, all necessary direct bodies before optional
/// metadata/windows or weaker evidence. Collection/order is never rerun here.
#[allow(clippy::too_many_lines)]
pub(super) fn materialize<'a>(
    plans: &[DocumentPlan<'a>],
    selected: &[(usize, usize, u32)],
    requested: &str,
    budget: &mut Budget,
) -> Result<Page<'a>, ExplanationError> {
    let mut pools = (0..plans.len())
        .map(|_| Pool::default())
        .collect::<Vec<_>>();
    let mut projections = plans
        .iter()
        .map(|plan| match plan {
            DocumentPlan::Flow(flow) => ProjectionAdmission::new(flow.content.document.as_ref()),
            DocumentPlan::Fixed(_) => ProjectionAdmission::new(None),
        })
        .collect::<Vec<_>>();
    let mut evidence = selected
        .iter()
        .map(|&(doc, index, ordinal)| {
            let record = match &plans[doc] {
                DocumentPlan::Flow(plan) => {
                    let candidate = &plan.candidates[index];
                    let mut deferred = Budget(0);
                    materialize::prepare(
                        plan.document_content(),
                        ordinal,
                        candidate,
                        &plan.located,
                        if candidate.class() == EvidenceClass::DirectEntry {
                            budget
                        } else {
                            &mut deferred
                        },
                    )
                }
                DocumentPlan::Fixed(plan) => {
                    let candidate = &plan.candidates[index];
                    let mut deferred = Budget(0);
                    plan.prepare(
                        candidate,
                        ordinal,
                        requested,
                        if candidate.class == EvidenceClass::DirectEntry {
                            budget
                        } else {
                            &mut deferred
                        },
                    )?
                }
            };
            Ok(ScopedExplanationEvidence {
                document_index: doc,
                evidence: record,
            })
        })
        .collect::<Result<Vec<_>, ExplanationError>>()?;
    let mut body_order = (0..selected.len()).collect::<Vec<_>>();
    // Materialization order is independent of evidence ordering. Selected
    // outer source fragments cover nested groups without forcing unselected
    // ancestors into an inner-only page.
    body_order.sort_by_key(|&slot| {
        let (doc, index, _) = selected[slot];
        (
            doc,
            match &plans[doc] {
                DocumentPlan::Flow(plan) => plan.supports.depth(plan.candidates[index].located),
                DocumentPlan::Fixed(_) => 0,
            },
        )
    });
    for &slot in &body_order {
        let (doc, index, _) = selected[slot];
        let result = &mut evidence[slot];
        if let DocumentPlan::Fixed(plan) = &plans[doc] {
            plan.copy_body(&plan.candidates[index], &mut result.evidence, budget)?;
        } else if let DocumentPlan::Flow(plan) = &plans[doc] {
            let candidate = &plan.candidates[index];
            if candidate.class() != EvidenceClass::DirectEntry {
                continue;
            }
            plan.supports.attach(
                plan.document_content(),
                candidate.located,
                &mut result.evidence,
                &plan.located,
                &mut pools[doc],
                budget,
                &mut projections[doc],
            );
            plan.supports.attach_owner(
                plan.document_content(),
                candidate.located,
                &mut result.evidence,
                &pools[doc],
                budget,
            );
            materialize::body(
                &mut result.evidence,
                candidate,
                &plan.located,
                budget,
                &mut projections[doc],
            );
            plan.supports.share_owner(
                candidate.located,
                selected
                    .iter()
                    .filter_map(|&(selected_doc, selected_index, _)| {
                        let DocumentPlan::Flow(selected_plan) = &plans[selected_doc] else {
                            return None;
                        };
                        let candidate = &selected_plan.candidates[selected_index];
                        (selected_doc == doc && candidate.class() == EvidenceClass::DirectEntry)
                            .then_some(candidate.located)
                            .flatten()
                    }),
                &mut result.evidence,
                &mut pools[doc],
                budget,
            );
        }
    }
    for slot in body_order {
        let (doc, index, _) = selected[slot];
        if let DocumentPlan::Fixed(plan) = &plans[doc] {
            plan.attach_group(
                &plan.candidates[index],
                &mut evidence[slot].evidence,
                &mut pools[doc],
                budget,
            )?;
        }
    }
    // Class-first order groups each document only within a class. Keep at
    // most one selected document's native unit index live; a later class may
    // rebuild it rather than multiplying a scope page's peak storage.
    let mut cached_fixed_doc = None;
    for (&(doc, index, ordinal), result) in selected.iter().zip(&mut evidence) {
        if cached_fixed_doc != Some(doc)
            && let Some(previous) = cached_fixed_doc.take()
            && let DocumentPlan::Fixed(plan) = &plans[previous]
        {
            plan.release_preview_units();
        }
        match &plans[doc] {
            DocumentPlan::Fixed(plan) => {
                let candidate = &plan.candidates[index];
                if candidate.class != EvidenceClass::DirectEntry {
                    result.evidence = plan.prepare(candidate, ordinal, requested, budget)?;
                }
                plan.finish_optional(candidate, &mut result.evidence, budget)?;
                plan.copy_previews(candidate, &mut result.evidence, budget)?;
                if !candidate.hits.is_empty() {
                    cached_fixed_doc = Some(doc);
                }
            }
            DocumentPlan::Flow(plan) => {
                let candidate = &plan.candidates[index];
                let content = plan.document_content();
                if candidate.class() != EvidenceClass::DirectEntry {
                    result.evidence =
                        materialize::prepare(content, ordinal, candidate, &plan.located, budget);
                    materialize::body(
                        &mut result.evidence,
                        candidate,
                        &plan.located,
                        budget,
                        &mut projections[doc],
                    );
                }
                materialize::materialize(
                    content,
                    &mut result.evidence,
                    candidate,
                    &plan.located,
                    &plan.rejected_aliases,
                    budget,
                    &mut projections[doc],
                );
            }
        }
    }
    if let Some(previous) = cached_fixed_doc
        && let DocumentPlan::Fixed(plan) = &plans[previous]
    {
        plan.release_preview_units();
    }
    Ok(Page {
        evidence,
        pools,
        projections,
    })
}
