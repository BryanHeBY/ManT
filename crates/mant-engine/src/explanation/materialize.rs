//! Materialize only the selected page: facts, previews, then atomic original body.
use super::{Candidate, ExplanationQuery, LocatedNode, plan::CollectionPlan};
use mant_ir::{DOCUMENT_ROOT_ID, EntryOwner};
use mant_protocol::{
    ExplanationContent, ExplanationEntry, ExplanationEvidence, ExplanationOutcome,
    ExplanationSchema, OutlineNodeReference, OutlineTrail, QueryExplanation,
};

pub(super) fn response(
    plan: CollectionPlan<'_>,
    query: &ExplanationQuery,
) -> (QueryExplanation, u32) {
    let query = ExplanationQuery {
        entry: query.entry.trim().to_owned(),
        options: query.options,
    };
    let total = u32::try_from(plan.candidates.len()).expect("bounded candidates");
    let mut counts = mant_protocol::EvidenceCounts::default();
    let mut budget = Budget(query.options.content_bytes as usize);
    let mut evidence = Vec::new();
    for (ordinal, candidate) in plan.candidates.iter().enumerate() {
        let selected = ordinal >= query.options.offset as usize
            && evidence.len() < query.options.limit as usize;
        counts.record(candidate.class(), selected);
        if selected {
            evidence.push(materialize(
                u32::try_from(ordinal).expect("bounded candidates"),
                candidate,
                &plan.located,
                &plan.rejected_aliases,
                &mut budget,
            ));
        }
    }
    let returned = u32::try_from(evidence.len()).expect("bounded result page");
    let end = query.options.offset.saturating_add(returned);
    let mut truncation = plan.truncation;
    truncation.content = evidence.iter().any(omitted);
    let used = query
        .options
        .content_bytes
        .saturating_sub(u32::try_from(budget.0).expect("bounded copy budget"));
    (
        QueryExplanation {
            schema: ExplanationSchema::V0Dot11,
            order: mant_protocol::EvidenceOrder::ClassThenSource,
            counts,
            label: plan.content.label.clone(),
            address: plan.content.address.clone(),
            producer: plan
                .content
                .document
                .as_ref()
                .map(mant_protocol::Producer::for_document),
            query,
            outcome: outcome(total),
            total,
            returned,
            next_offset: (end < total).then_some(end),
            truncation,
            semantics_complete: crate::projection::semantics_complete(&plan.diagnostics),
            diagnostics: plan.diagnostics,
            evidence,
        },
        used,
    )
}

pub(super) fn outcome(total: u32) -> ExplanationOutcome {
    if total == 0 {
        ExplanationOutcome::NoEvidence
    } else {
        ExplanationOutcome::Evidence
    }
}
pub(super) fn omitted(evidence: &ExplanationEvidence) -> bool {
    evidence.has_omitted_content()
}

pub(super) fn materialize(
    ordinal: u32,
    candidate: &Candidate<'_>,
    located: &[LocatedNode<'_>],
    rejected_aliases: &std::collections::BTreeSet<mant_ir::NodeId>,
    budget: &mut Budget,
) -> ExplanationEvidence {
    let node = candidate.located.or(candidate.section).map(|i| &located[i]);
    let outline = node.map_or_else(root_trail, trail);
    let mut entry = None;
    let mut details_omitted = false;
    if let Some(index) = candidate.located {
        let owner = super::owner(&located[index]).expect("entry location");
        let facts = owner.facts().expect("entry facts");
        let details = ExplanationEntry {
            role: facts.role,
            case: facts.case,
            names: owner.validated_names().unwrap_or_default().to_vec(),
            forms: owner.forms().unwrap_or_default().into_owned(),
            alias_groups: owner.validated_alias_groups().unwrap_or_default().to_vec(),
            alias_of: facts
                .alias_of
                .clone()
                .filter(|_| !rejected_aliases.contains(&facts.id)),
            value_domain: facts.value_domain.clone(),
        };
        if budget.take(&details) {
            entry = Some(details);
        } else {
            details_omitted = true;
        }
    }
    let mut previews = Vec::new();
    let mut previews_omitted = false;
    for hit in &candidate.hits {
        let preview = hit.preview();
        if budget.take(&preview) {
            previews.push(preview);
        } else {
            previews_omitted = true;
        }
    }
    let content = if let Some(index) = candidate.located {
        let node = &located[index];
        let owner = super::owner(node).expect("entry location");
        let fits = match owner {
            EntryOwner::List(item) => budget.take(item),
            EntryOwner::Definition(item) => budget.take(item),
        };
        fits.then(|| match node {
            LocatedNode::Entry { entry, .. } => ExplanationContent::Entry {
                block: entry.content(),
            },
            LocatedNode::Section { .. } => unreachable!("indexed entry"),
        })
    } else {
        candidate
            .ordinary
            .filter(|block| budget.take(*block))
            .map(|block| ExplanationContent::Block {
                block: block.clone(),
            })
    };
    ExplanationEvidence {
        class: candidate.class(),
        ordinal,
        outline,
        block_path: candidate.block_path.clone(),
        source: candidate.source,
        bases: candidate.bases.clone(),
        entry,
        previews,
        previews_omitted,
        details_omitted,
        content_omitted: content.is_none(),
        content,
    }
}
fn trail(node: &LocatedNode<'_>) -> OutlineTrail {
    let (breadcrumbs, reference) = match node {
        LocatedNode::Section {
            section,
            path,
            breadcrumbs,
            ..
        } => (
            breadcrumbs,
            OutlineNodeReference::DocumentSection {
                path: path.to_string().into(),
                id: section.id.clone(),
                title: section.title.clone(),
            },
        ),
        LocatedNode::Entry {
            entry,
            path,
            title,
            breadcrumbs,
            ..
        } => {
            let facts = entry.item.facts().expect("indexed entry");
            (
                breadcrumbs,
                OutlineNodeReference::DocumentEntry {
                    path: path.to_string().into(),
                    id: facts.id.clone(),
                    title: title.clone(),
                    role: facts.role,
                    case: facts.case,
                    names: entry.names.to_vec(),
                },
            )
        }
    };
    OutlineTrail {
        ancestors: crate::projection::excerpt::project_breadcrumbs(breadcrumbs),
        node: reference,
    }
}
fn root_trail() -> OutlineTrail {
    OutlineTrail {
        ancestors: Vec::new(),
        node: OutlineNodeReference::DocumentRoot {
            path: mant_ir::OutlinePath::DocumentRoot.to_string().into(),
            id: DOCUMENT_ROOT_ID.into(),
            title: "OVERVIEW".into(),
        },
    }
}

pub(super) struct Budget(pub usize);
impl Budget {
    pub(super) fn take(&mut self, value: &impl serde::Serialize) -> bool {
        struct Count {
            remaining: usize,
        }
        impl std::io::Write for Count {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.remaining = self
                    .remaining
                    .checked_sub(bytes.len())
                    .ok_or_else(|| std::io::Error::other("explanation content budget"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut counter = Count { remaining: self.0 };
        if serde_json::to_writer(&mut counter, value).is_err() {
            return false;
        }
        self.0 = counter.remaining;
        true
    }
}
