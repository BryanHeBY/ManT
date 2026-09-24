//! Conservative explanation from surviving native Fixed owner evidence.
//!
//! No Flow block, inline tree, literal mention or declaration relationship is
//! synthesized from terminal geometry. Complete native definition heads and
//! proven head components retain their final display selections.

use std::{cell::RefCell, collections::BTreeMap, num::NonZeroU32, ops::Range};

use mant_ir::{
    DOCUMENT_ROOT_ID, Document, FixedBody, FixedSectionReader, OutlinePath, OwnerMark,
    ResolvedContent, SemanticEntry, SemanticIndex,
};
use mant_protocol::{
    EvidenceClass, EvidenceCounts, EvidenceOrder, ExplanationEvidence, ExplanationOutcome,
    ExplanationQuery, ExplanationSchema, ExplanationTruncation, OutlineNodeReference,
    OutlineReference, OutlineTrail, QueryExplanation,
};

use super::{ExplanationError, materialize::Budget, support::Pool};

mod mentions;
mod selection;
mod transfer;

struct IndexedOwner {
    path: OutlinePath,
    matched: (bool, bool, bool, bool),
}

pub(super) struct FixedCandidate {
    pub key: Option<NonZeroU32>,
    pub section: Option<NonZeroU32>,
    pub class: EvidenceClass,
    pub hits: Vec<FixedHit>,
}

#[derive(Clone)]
pub(super) struct FixedHit {
    pub unit: usize,
    pub range: Range<usize>,
}

pub(super) struct FixedPlan<'a> {
    pub content: &'a ResolvedContent,
    document: &'a Document,
    fixed: &'a FixedBody,
    reader: FixedSectionReader<'a>,
    index: SemanticIndex,
    indexed: BTreeMap<NonZeroU32, IndexedOwner>,
    pub candidates: Vec<FixedCandidate>,
    pub truncated: bool,
    // Built only for documents selected by the response page, never for all
    // scoped plans. A scope page releases it between document segments.
    preview_units: RefCell<Option<crate::search::fixed_visible::FixedVisibleUnits>>,
}

/// A Fixed result only claims directly evidenced complete heads. Native body
/// slices are returned separately and never borrowed from a following owner.
pub(super) fn response(
    resolved: &ResolvedContent,
    document: &Document,
    fixed: &FixedBody,
    query: &ExplanationQuery,
) -> Result<(QueryExplanation, u32), ExplanationError> {
    let plan = plan(resolved, document, fixed, query.entry.trim())?;
    let resolved = plan.content;
    let document = plan.document;
    let requested = query.entry.trim();
    let total = u32::try_from(plan.candidates.len()).expect("bounded Fixed candidates");
    let mut budget = Budget(query.options.content_bytes as usize);
    let mut counts = EvidenceCounts::default();
    let mut evidence = Vec::new();
    let mut pool = Pool::default();
    let mut selected_candidates = Vec::new();
    for (ordinal, candidate) in plan.candidates.iter().enumerate() {
        let selected = ordinal >= query.options.offset as usize
            && evidence.len() < query.options.limit as usize;
        counts.record(candidate.class, selected);
        if selected {
            let mut deferred = Budget(0);
            let record = plan.prepare(
                candidate,
                u32::try_from(ordinal).expect("bounded Fixed ordinal"),
                requested,
                if candidate.class == EvidenceClass::DirectEntry {
                    &mut budget
                } else {
                    &mut deferred
                },
            )?;
            selected_candidates.push(ordinal);
            evidence.push(record);
        }
    }
    for (&index, record) in selected_candidates.iter().zip(&mut evidence) {
        plan.copy_body(&plan.candidates[index], record, &mut budget)?;
    }
    for (&index, record) in selected_candidates.iter().zip(&mut evidence) {
        plan.attach_group(&plan.candidates[index], record, &mut pool, &mut budget)?;
    }
    for (&index, record) in selected_candidates.iter().zip(&mut evidence) {
        plan.finish_optional(&plan.candidates[index], record, &mut budget)?;
    }
    for (&index, record) in selected_candidates.iter().zip(&mut evidence) {
        let candidate = &plan.candidates[index];
        if candidate.class != EvidenceClass::DirectEntry {
            *record = plan.prepare(candidate, record.ordinal, requested, &mut budget)?;
        }
        plan.copy_previews(candidate, record, &mut budget)?;
    }
    let returned = u32::try_from(evidence.len()).expect("bounded Fixed page");
    let end = query.options.offset.saturating_add(returned);
    let used = query
        .options
        .content_bytes
        .saturating_sub(u32::try_from(budget.0).expect("bounded Fixed copy budget"));
    let content_omitted = evidence
        .iter()
        .any(ExplanationEvidence::has_omitted_content);
    Ok((
        QueryExplanation {
            supports: pool.values,
            content_projection: None,
            order: EvidenceOrder::ClassThenSource,
            counts,
            schema: ExplanationSchema::V0Dot12,
            query: ExplanationQuery {
                entry: requested.to_owned(),
                options: query.options,
            },
            label: resolved.label.clone(),
            address: resolved.address.clone(),
            producer: Some(mant_protocol::Producer::for_document(document)),
            source_context: Some(mant_protocol::SourceContext::from(document)),
            outcome: if total == 0 {
                ExplanationOutcome::NoEvidence
            } else {
                ExplanationOutcome::Evidence
            },
            total,
            returned,
            next_offset: (end < total).then_some(end),
            truncation: ExplanationTruncation {
                candidates: plan.truncated,
                relations: false,
                content: content_omitted,
            },
            semantics_complete: crate::projection::semantics_complete(&document.diagnostics),
            diagnostics: document.diagnostics.clone(),
            evidence,
        },
        used,
    ))
}

pub(super) fn plan<'a>(
    resolved: &'a ResolvedContent,
    document: &'a Document,
    fixed: &'a FixedBody,
    requested: &str,
) -> Result<FixedPlan<'a>, ExplanationError> {
    mant_ir::validate_document_sources(document).map_err(|_| ExplanationError::InvalidFixed)?;
    let reader = FixedSectionReader::new(fixed).map_err(|_| ExplanationError::InvalidFixed)?;
    let index = SemanticIndex::build(document);
    let indexed = collect_indexed(&index, &reader, requested);
    let mut direct = BTreeMap::new();
    let mut truncated_direct = false;
    for owner in &fixed.owners {
        let Some(indexed) = indexed.get(&owner.key) else {
            continue;
        };
        let (name, form, id, path) = indexed.matched;
        if !(name || form || id || path) {
            continue;
        }
        if direct.len() == mant_protocol::MAX_EXPLANATION_CANDIDATES {
            truncated_direct = true;
            continue;
        }
        direct.insert(
            owner.key,
            FixedCandidate {
                key: Some(owner.key),
                section: owner.section,
                class: EvidenceClass::DirectEntry,
                hits: Vec::new(),
            },
        );
    }
    let (mentions, mut truncated) = mentions::collect(fixed, &indexed, &direct, requested)?;
    truncated |= truncated_direct;
    let mut weak = Vec::new();
    for mention in mentions {
        if let Some(owner) = mention.key
            && let Some(selected) = direct.get_mut(&owner)
        {
            selected
                .hits
                .extend(mention.hits.into_iter().take(2 - selected.hits.len()));
        } else {
            weak.push(mention);
        }
    }
    let mut candidates = direct.into_values().collect::<Vec<_>>();
    candidates.extend(weak);
    if candidates.len() > mant_protocol::MAX_EXPLANATION_CANDIDATES {
        candidates.truncate(mant_protocol::MAX_EXPLANATION_CANDIDATES);
        truncated = true;
    }
    Ok(FixedPlan {
        content: resolved,
        document,
        fixed,
        reader,
        index,
        indexed,
        candidates,
        truncated,
        preview_units: RefCell::new(None),
    })
}

fn collect_indexed(
    index: &SemanticIndex,
    reader: &FixedSectionReader<'_>,
    requested: &str,
) -> BTreeMap<NonZeroU32, IndexedOwner> {
    let mut map = BTreeMap::new();
    add_entries(index, index.root(), None, &[], requested, &mut map);
    for heading in &reader.fixed().headings {
        let Some(OutlinePath::Section(coordinates)) = reader.path(heading.key) else {
            continue;
        };
        let section = coordinates
            .iter()
            .map(|part| part.get())
            .collect::<Vec<_>>();
        let source = section.iter().map(|part| part - 1).collect::<Vec<_>>();
        add_entries(
            index,
            index.section_at(&source),
            Some(&section),
            &[],
            requested,
            &mut map,
        );
    }
    map
}

fn add_entries(
    index: &SemanticIndex,
    entries: &[SemanticEntry],
    section: Option<&[usize]>,
    prefix: &[usize],
    requested: &str,
    result: &mut BTreeMap<NonZeroU32, IndexedOwner>,
) {
    for (position, entry) in entries.iter().enumerate() {
        let mut indices = prefix.to_vec();
        indices.push(position + 1);
        let Some(path) = OutlinePath::nested_entry(section, &indices) else {
            continue;
        };
        if let Some(mant_ir::ContentReveal::FixedOwner { key }) = index.owner_at(&path) {
            let matched = (
                entry.names.iter().any(|value| value == requested),
                entry.forms.iter().any(|value| value == requested),
                entry.id.as_str() == requested,
                path.to_string() == requested,
            );
            result.insert(*key, IndexedOwner { path, matched });
        }
        add_entries(index, &entry.children, section, &indices, requested, result);
    }
}

fn trail(
    reader: &FixedSectionReader<'_>,
    index: &SemanticIndex,
    indexed: &BTreeMap<NonZeroU32, IndexedOwner>,
    owner: &OwnerMark,
    selected: &IndexedOwner,
) -> Result<OutlineTrail, ExplanationError> {
    let mut ancestors = Vec::new();
    ancestors.push(OutlineReference {
        path: OutlinePath::DocumentRoot.to_string().into(),
        id: DOCUMENT_ROOT_ID.into(),
        title: crate::selectors::DOCUMENT_ROOT_TITLE.to_owned(),
    });
    if let Some(section) = owner.section {
        for heading in reader
            .breadcrumbs(section)
            .ok_or(ExplanationError::InvalidFixed)?
        {
            ancestors.push(OutlineReference {
                path: reader
                    .path(heading.key)
                    .ok_or(ExplanationError::InvalidFixed)?
                    .to_string()
                    .into(),
                id: heading.id.clone(),
                title: reader
                    .label(heading.key)
                    .ok_or(ExplanationError::InvalidFixed)?,
            });
        }
    }
    let mut parents = Vec::new();
    let mut parent = owner.parent;
    while let Some(key) = parent {
        let mark = reader
            .fixed()
            .owners
            .get(usize::try_from(key.get() - 1).map_err(|_| ExplanationError::InvalidFixed)?)
            .ok_or(ExplanationError::InvalidFixed)?;
        if let Some(entry) = indexed.get(&key) {
            let facts = index
                .entry_at(&entry.path)
                .ok_or(ExplanationError::InvalidFixed)?;
            parents.push(OutlineReference {
                path: entry.path.to_string().into(),
                id: facts.id.clone(),
                title: facts.forms.first().cloned().unwrap_or_default(),
            });
        }
        parent = mark.parent;
    }
    ancestors.extend(parents.into_iter().rev());
    let facts = index
        .entry_at(&selected.path)
        .ok_or(ExplanationError::InvalidFixed)?;
    Ok(OutlineTrail {
        ancestors,
        node: OutlineNodeReference::DocumentEntry {
            path: selected.path.to_string().into(),
            id: facts.id.clone(),
            title: facts.forms.first().cloned().unwrap_or_default(),
            entry_kind: facts.kind,
            case: facts.case,
            names: facts.names.clone(),
        },
    })
}

fn mention_section_trail(
    reader: &FixedSectionReader<'_>,
    section: Option<NonZeroU32>,
) -> Result<OutlineTrail, ExplanationError> {
    crate::fixed_navigation::section_trail(reader, section).ok_or(ExplanationError::InvalidFixed)
}

#[cfg(test)]
mod tests;
