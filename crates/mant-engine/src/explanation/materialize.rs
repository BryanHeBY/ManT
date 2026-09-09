//! Materialize only the selected page, reserving direct facts and bodies/context
//! before optional previews and weaker evidence.
use super::{Candidate, ExplanationQuery, LocatedNode, plan::CollectionPlan};
use mant_ir::DOCUMENT_ROOT_ID;
use mant_protocol::{
    ExplanationContent, ExplanationEvidence, ExplanationOutcome, ExplanationSchema,
    OutlineNodeReference, OutlineTrail, QueryExplanation,
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
    let mut selection = Vec::new();
    for (ordinal, candidate) in plan.candidates.iter().enumerate() {
        let selected = ordinal >= query.options.offset as usize
            && selection.len() < query.options.limit as usize;
        counts.record(candidate.class(), selected);
        if selected {
            selection.push((
                0,
                ordinal,
                u32::try_from(ordinal).expect("bounded candidates"),
            ));
        }
    }
    let mut page = super::page::materialize(std::slice::from_ref(&plan), &selection, &mut budget);
    let evidence = page
        .evidence
        .into_iter()
        .map(|e| e.evidence)
        .collect::<Vec<_>>();
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
            supports: std::mem::take(&mut page.pools[0].values),
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

pub(super) fn prepare(
    ordinal: u32,
    candidate: &Candidate<'_>,
    located: &[LocatedNode<'_>],
    budget: &mut Budget,
) -> ExplanationEvidence {
    let node = candidate.located.or(candidate.section).map(|i| &located[i]);
    let outline = node.map_or_else(root_trail, trail);
    let owner = candidate
        .located
        .and_then(|index| super::owner(&located[index]));
    let (bases, match_details_omitted) = super::details::matched(candidate, owner, budget);
    ExplanationEvidence {
        support: None,
        support_omitted: false,
        class: candidate.class(),
        ordinal,
        outline,
        block_path: candidate.block_path.clone(),
        source: candidate.source,
        bases,
        entry: None,
        previews: Vec::new(),
        previews_omitted: false,
        details_omitted: false,
        match_details_omitted,
        name_bindings_omitted: false,
        content_omitted: false,
        content: None,
    }
}

pub(super) fn materialize(
    record: &mut ExplanationEvidence,
    candidate: &Candidate<'_>,
    located: &[LocatedNode<'_>],
    rejected_aliases: &std::collections::BTreeSet<mant_ir::NodeId>,
    budget: &mut Budget,
) {
    let owner = candidate
        .located
        .and_then(|index| super::owner(&located[index]));
    let mut bases = std::mem::take(&mut record.bases);
    let mut match_details_omitted = record.match_details_omitted;
    // Page facts were reserved before any optional payload. References keep
    // owner-local positions while avoiding a second copy of the group body.
    let content = record.content.take();
    let mut entry = owner.and_then(|owner| super::details::entry(owner, rejected_aliases, budget));
    let details_omitted = owner.is_some() && entry.is_none();
    let mut name_bindings_omitted = false;
    let mut positions = super::positions::PositionBudget::default();
    if let (Some(owner), Some(entry)) = (owner, entry.as_mut()) {
        name_bindings_omitted = super::positions::ordinary(owner, entry, budget);
        let (matched, names) = super::positions::attach(
            owner,
            &mut bases,
            Some(entry),
            super::positions::Domain::Forms,
            budget,
            &mut positions,
        );
        match_details_omitted |= matched;
        name_bindings_omitted |= names;
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
    // Reserve and accept the complete body before committing any reference to
    // it. Optional complete location domains consume only the remaining budget;
    // no rollback/retry can leave a reference to a body that was later dropped.
    if content.is_some() {
        if let Some(owner) = owner {
            let (matched, names) = super::positions::attach(
                owner,
                &mut bases,
                entry.as_mut(),
                super::positions::Domain::Content,
                budget,
                &mut positions,
            );
            match_details_omitted |= matched;
            name_bindings_omitted |= names;
        }
        for preview in &mut previews {
            let hit = candidate
                .hits
                .iter()
                .find(|hit| hit.path == preview.block_path)
                .expect("retained representative hit");
            if let Some(range) = super::positions::preview_range(owner, hit) {
                let ranges = vec![range];
                if positions.remaining() > 0 && budget.take_growth(&preview.content_ranges, &ranges)
                {
                    preview.content_ranges = ranges;
                    positions.charge(1);
                } else {
                    match_details_omitted = true;
                }
            } else {
                match_details_omitted = true;
            }
        }
    }
    record.bases = bases;
    record.entry = entry;
    record.previews = previews;
    record.previews_omitted = previews_omitted;
    record.details_omitted = details_omitted;
    record.match_details_omitted = match_details_omitted;
    record.name_bindings_omitted = name_bindings_omitted;
    record.content_omitted = content.is_none();
    record.content = content;
}

pub(super) fn body(
    record: &mut ExplanationEvidence,
    candidate: &Candidate<'_>,
    located: &[LocatedNode<'_>],
    budget: &mut Budget,
) {
    if record.content.is_none() {
        record.content = copy_body(candidate, located, budget);
    }
}

fn copy_body(
    candidate: &Candidate<'_>,
    located: &[LocatedNode<'_>],
    budget: &mut Budget,
) -> Option<ExplanationContent> {
    #[derive(serde::Serialize)]
    struct Body<'a, T: serde::Serialize> {
        kind: &'static str,
        block: &'a T,
    }
    if let Some(index) = candidate.located {
        match &located[index] {
            LocatedNode::Entry { entry, .. } => budget
                .take(&Body {
                    kind: "entry",
                    block: entry,
                })
                .then(|| ExplanationContent::Entry {
                    block: entry.content(),
                }),
            LocatedNode::Section { .. } => unreachable!("indexed entry"),
        }
    } else {
        candidate
            .ordinary
            .filter(|block| {
                budget.take(&Body {
                    kind: "block",
                    block: *block,
                })
            })
            .map(|block| ExplanationContent::Block {
                block: block.clone(),
            })
    }
}
pub(super) fn trail(node: &LocatedNode<'_>) -> OutlineTrail {
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
                title: section.heading.plain_text(),
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
                    entry_kind: facts.kind,
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
    pub(super) fn fits(&self, value: &impl serde::Serialize) -> bool {
        Self::size(value, self.0).is_some()
    }
    pub(super) fn take_growth(
        &mut self,
        old: &impl serde::Serialize,
        new: &impl serde::Serialize,
    ) -> bool {
        let Some(old_size) = Self::size(old, usize::MAX) else {
            return false;
        };
        let Some(new_size) = Self::size(new, old_size.saturating_add(self.0)) else {
            return false;
        };
        self.0 = self.0.saturating_sub(new_size.saturating_sub(old_size));
        true
    }

    pub(super) fn take(&mut self, value: &impl serde::Serialize) -> bool {
        let Some(size) = Self::size(value, self.0) else {
            return false;
        };
        self.0 -= size;
        true
    }

    fn size(value: &impl serde::Serialize, limit: usize) -> Option<usize> {
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
        let mut counter = Count { remaining: limit };
        if serde_json::to_writer(&mut counter, value).is_err() {
            return None;
        }
        Some(limit - counter.remaining)
    }
}
