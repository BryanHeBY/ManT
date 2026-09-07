//! Budget before copying bodies: omitted content remains directly addressable.
use super::{Candidate, ExplanationQuery, LocatedNode, ResolvedContent};
use mant_ir::{DOCUMENT_ROOT_ID, EntryOwner};
use mant_protocol::{
    ExplanationContent, ExplanationEntry, ExplanationEvidence, ExplanationOutcome,
    ExplanationSchema, OutlineNodeReference, OutlineTrail, QueryExplanation,
};

pub(super) fn response(
    content: &ResolvedContent,
    query: ExplanationQuery,
    located: &[LocatedNode<'_>],
    candidates: Vec<Candidate<'_>>,
    candidates_truncated: bool,
    relations_truncated: bool,
) -> (QueryExplanation, u32) {
    let total = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
    let mut budget = Budget(usize::try_from(query.options.content_bytes).unwrap_or(usize::MAX));
    let evidence = candidates
        .into_iter()
        .enumerate()
        .skip(query.options.offset as usize)
        .take(query.options.limit as usize)
        .map(|(ordinal, candidate)| {
            materialize(
                u32::try_from(ordinal).unwrap_or(u32::MAX),
                candidate,
                located,
                &mut budget,
            )
        })
        .collect::<Vec<_>>();
    let returned = u32::try_from(evidence.len()).unwrap_or(u32::MAX);
    let end = query.options.offset.saturating_add(returned);
    let document = content.document.as_ref();
    let mut diagnostics = document.map(|d| d.diagnostics.clone()).unwrap_or_default();
    if let Some(document) = document {
        for diagnostic in mant_ir::validate_document(document) {
            if !diagnostics.contains(&diagnostic) {
                diagnostics.push(diagnostic);
            }
        }
    }
    let used = query
        .options
        .content_bytes
        .saturating_sub(u32::try_from(budget.0).unwrap_or(u32::MAX));
    let response = QueryExplanation {
        schema: ExplanationSchema::V0Dot11,
        label: content.label.clone(),
        address: content.address.clone(),
        producer: document.map(mant_protocol::Producer::for_document),
        query,
        outcome: if total == 0 {
            ExplanationOutcome::NoEvidence
        } else {
            ExplanationOutcome::Evidence
        },
        total,
        returned,
        next_offset: (end < total).then_some(end),
        truncation: mant_protocol::ExplanationTruncation {
            candidates: candidates_truncated,
            relations: relations_truncated,
            content: evidence
                .iter()
                .any(|e| e.content_omitted || e.details_omitted),
        },
        semantics_complete: crate::projection::semantics_complete(&diagnostics),
        diagnostics,
        evidence,
    };
    (response, used)
}

fn materialize(
    ordinal: u32,
    candidate: Candidate<'_>,
    located: &[LocatedNode<'_>],
    budget: &mut Budget,
) -> ExplanationEvidence {
    let node = candidate.located.or(candidate.section).map(|i| &located[i]);
    let outline = node.map_or_else(root_trail, trail);
    let mut entry = None;
    let mut details_omitted = false;
    let content = if let Some(index) = candidate.located {
        let node = &located[index];
        let owner = super::owner(node).expect("entry location");
        let facts = owner.facts().expect("entry facts");
        // Forms are bounded by the source input and never include nested bodies.
        // They are materialized one owner at a time, not cached for all candidates.
        let details = ExplanationEntry {
            role: facts.role,
            case: facts.case,
            names: facts.names.clone(),
            forms: owner.forms().unwrap_or_default().into_owned(),
            alias_groups: facts.alias_groups.clone(),
            alias_of: facts.alias_of.clone(),
            value_domain: facts.value_domain.clone(),
        };
        if budget.take(&details) {
            entry = Some(details);
        } else {
            details_omitted = true;
        }
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
        ordinal,
        outline,
        block_path: candidate.block_path,
        source: candidate.source,
        bases: candidate.bases,
        entry,
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
                    names: facts.names.clone(),
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

struct Budget(usize);
impl Budget {
    fn take(&mut self, value: &impl serde::Serialize) -> bool {
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
