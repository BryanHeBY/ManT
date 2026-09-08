//! One page budget schedule shared by single-document and scoped explanations.
use super::{
    materialize::{self, Budget},
    plan::CollectionPlan,
    support::Pool,
};
use mant_protocol::{EvidenceClass, ScopedExplanationEvidence};

pub(super) struct Page {
    pub evidence: Vec<ScopedExplanationEvidence>,
    pub pools: Vec<Pool>,
}

/// Selection contains (document, candidate, global ordinal). All direct facts
/// are reserved before any body, all necessary direct bodies before optional
/// metadata/windows or weaker evidence. Collection/order is never rerun here.
pub(super) fn materialize(
    plans: &[CollectionPlan<'_>],
    selected: &[(usize, usize, u32)],
    budget: &mut Budget,
) -> Page {
    let mut pools = (0..plans.len())
        .map(|_| Pool::default())
        .collect::<Vec<_>>();
    let mut evidence = selected
        .iter()
        .map(|&(doc, index, ordinal)| {
            let plan = &plans[doc];
            let candidate = &plan.candidates[index];
            let mut deferred = Budget(0);
            let record = materialize::prepare(
                ordinal,
                candidate,
                &plan.located,
                if candidate.class() == EvidenceClass::DirectEntry {
                    budget
                } else {
                    &mut deferred
                },
            );
            ScopedExplanationEvidence {
                document_index: doc,
                evidence: record,
            }
        })
        .collect::<Vec<_>>();
    let mut body_order = (0..selected.len()).collect::<Vec<_>>();
    // Materialization order is independent of evidence ordering. Selected
    // outer source fragments cover nested groups without forcing unselected
    // ancestors into an inner-only page.
    body_order.sort_by_key(|&slot| {
        let (doc, index, _) = selected[slot];
        (
            doc,
            plans[doc]
                .supports
                .depth(plans[doc].candidates[index].located),
        )
    });
    for slot in body_order {
        let (doc, index, _) = selected[slot];
        let result = &mut evidence[slot];
        let plan = &plans[doc];
        let candidate = &plan.candidates[index];
        if candidate.class() == EvidenceClass::DirectEntry {
            plan.supports.attach(
                candidate.located,
                &mut result.evidence,
                &plan.located,
                &mut pools[doc],
                budget,
            );
            plan.supports.attach_owner(
                candidate.located,
                &mut result.evidence,
                &pools[doc],
                budget,
            );
            materialize::body(&mut result.evidence, candidate, &plan.located, budget);
            plan.supports.share_owner(
                candidate.located,
                selected
                    .iter()
                    .filter_map(|&(selected_doc, selected_index, _)| {
                        let candidate = &plans[selected_doc].candidates[selected_index];
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
    for (&(doc, index, ordinal), result) in selected.iter().zip(&mut evidence) {
        let plan = &plans[doc];
        let candidate = &plan.candidates[index];
        if candidate.class() != EvidenceClass::DirectEntry {
            result.evidence = materialize::prepare(ordinal, candidate, &plan.located, budget);
            materialize::body(&mut result.evidence, candidate, &plan.located, budget);
        }
        materialize::materialize(
            &mut result.evidence,
            candidate,
            &plan.located,
            &plan.rejected_aliases,
            budget,
        );
    }
    Page { evidence, pools }
}
