//! Conservative explanation from surviving native Fixed owner evidence.
//!
//! No Flow block, inline tree, literal mention or declaration relationship is
//! synthesized from terminal geometry. Complete native definition heads and
//! proven head components retain their final display selections.

use std::{cell::RefCell, collections::BTreeMap, num::NonZeroU32, ops::Range};

use mant_ir::{
    DOCUMENT_ROOT_ID, Document, FixedBody, FixedSectionReader, OutlinePath, OwnerMark,
    ResolvedContent, SemanticEntry, SemanticIndex, TextSelection,
};
use mant_protocol::{
    EvidenceBasis, EvidenceClass, EvidenceCounts, EvidenceOrder, ExplanationContent,
    ExplanationEntry, ExplanationEvidence, ExplanationFixedFormRange, ExplanationFixedGroupMember,
    ExplanationFixedPart, ExplanationFixedPreview, ExplanationFixedSelection, ExplanationFormMatch,
    ExplanationIdentityField, ExplanationNameBinding, ExplanationNameMatch, ExplanationOccurrence,
    ExplanationOutcome, ExplanationQuery, ExplanationSchema, ExplanationSupport,
    ExplanationTruncation, OutlineNodeReference, OutlineReference, OutlineTrail, QueryExplanation,
};

use super::{ExplanationError, materialize::Budget, support::Pool};

mod mentions;

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

impl FixedPlan<'_> {
    fn owner(&self, candidate: &FixedCandidate) -> Result<&OwnerMark, ExplanationError> {
        let key = candidate.key.ok_or(ExplanationError::InvalidFixed)?;
        self.fixed
            .owners
            .get((key.get() - 1) as usize)
            .ok_or(ExplanationError::InvalidFixed)
    }

    pub(super) fn prepare(
        &self,
        candidate: &FixedCandidate,
        ordinal: u32,
        requested: &str,
        budget: &mut Budget,
    ) -> Result<ExplanationEvidence, ExplanationError> {
        if candidate.class != EvidenceClass::DirectEntry {
            return self.prepare_mention(candidate, ordinal, budget);
        }
        let owner = self.owner(candidate)?;
        let key = candidate.key.ok_or(ExplanationError::InvalidFixed)?;
        let selected = self
            .indexed
            .get(&key)
            .ok_or(ExplanationError::InvalidFixed)?;
        let entry = self
            .index
            .entry_at(&selected.path)
            .ok_or(ExplanationError::InvalidFixed)?;
        let (name_match, form_match, id_match, path_match) = selected.matched;
        let outline = trail(&self.reader, &self.index, &self.indexed, owner, selected)?;
        if entry.forms.is_empty() {
            return Err(ExplanationError::InvalidFixed);
        }
        let mut evidence_bases = Vec::new();
        let mut match_details_omitted = false;
        if name_match {
            let basis = EvidenceBasis::Name {
                matches: vec![ExplanationNameMatch {
                    name: requested.to_owned(),
                    occurrences: Vec::new(),
                }],
            };
            if budget.take(&basis) {
                evidence_bases.push(basis);
            } else {
                match_details_omitted = true;
            }
        }
        if form_match {
            let basis = EvidenceBasis::Form {
                matches: entry
                    .forms
                    .iter()
                    .enumerate()
                    .filter(|(_, form)| *form == requested)
                    .map(|(index, _)| ExplanationFormMatch {
                        source_form_index: u32::try_from(index).expect("bounded Fixed forms"),
                        text: requested.to_owned(),
                        occurrences: Vec::new(),
                    })
                    .collect(),
            };
            if budget.take(&basis) {
                evidence_bases.push(basis);
            } else {
                match_details_omitted = true;
            }
        }
        if id_match || path_match {
            let mut fields = Vec::new();
            if id_match {
                fields.push(ExplanationIdentityField::Id);
            }
            if path_match {
                fields.push(ExplanationIdentityField::Path);
            }
            evidence_bases.push(EvidenceBasis::Identity { fields });
        }
        if !candidate.hits.is_empty() {
            if budget.take(&EvidenceBasis::Literal) {
                evidence_bases.push(EvidenceBasis::Literal);
            } else {
                match_details_omitted = true;
            }
        }
        Ok(ExplanationEvidence {
            support: None,
            support_omitted: false,
            class: EvidenceClass::DirectEntry,
            ordinal,
            outline,
            block_path: None,
            source: owner.source,
            bases: evidence_bases,
            previews: Vec::new(),
            fixed_previews: Vec::new(),
            previews_omitted: false,
            entry: None,
            content: None,
            details_omitted: true,
            match_details_omitted,
            name_bindings_omitted: false,
            content_omitted: true,
        })
    }

    fn prepare_mention(
        &self,
        candidate: &FixedCandidate,
        ordinal: u32,
        budget: &mut Budget,
    ) -> Result<ExplanationEvidence, ExplanationError> {
        let outline = if let Some(key) = candidate.key {
            let owner = self
                .fixed
                .owners
                .get((key.get() - 1) as usize)
                .ok_or(ExplanationError::InvalidFixed)?;
            let selected = self
                .indexed
                .get(&key)
                .ok_or(ExplanationError::InvalidFixed)?;
            trail(&self.reader, &self.index, &self.indexed, owner, selected)?
        } else {
            mention_section_trail(&self.reader, candidate.section)?
        };
        let retained = budget.take(&EvidenceBasis::Literal);
        Ok(ExplanationEvidence {
            support: None,
            support_omitted: false,
            class: candidate.class,
            ordinal,
            outline,
            block_path: None,
            source: candidate
                .key
                .and_then(|key| self.fixed.owners.get((key.get() - 1) as usize))
                .and_then(|owner| owner.source),
            bases: if retained {
                vec![EvidenceBasis::Literal]
            } else {
                Vec::new()
            },
            previews: Vec::new(),
            fixed_previews: Vec::new(),
            previews_omitted: false,
            entry: None,
            content: None,
            details_omitted: false,
            match_details_omitted: !retained,
            name_bindings_omitted: false,
            content_omitted: false,
        })
    }

    pub(super) fn copy_body(
        &self,
        candidate: &FixedCandidate,
        record: &mut ExplanationEvidence,
        budget: &mut Budget,
    ) -> Result<(), ExplanationError> {
        if candidate.class != EvidenceClass::DirectEntry {
            return Ok(());
        }
        let key = candidate.key.ok_or(ExplanationError::InvalidFixed)?;
        let minimum = ExplanationContent::FixedOwner {
            key,
            reading_body: ExplanationFixedSelection {
                parts: Vec::new(),
                joins: Vec::new(),
            },
        };
        if !budget.fits(&minimum) {
            record.content_omitted = true;
            return Ok(());
        }
        let owner = self.owner(candidate)?;
        let body_selection = self.reading_body_selection(owner)?;
        if retained_fixed_positions(record).saturating_add(body_selection.parts.len())
            > mant_protocol::MAX_EXPLANATION_POSITIONS
        {
            record.content_omitted = true;
            return Ok(());
        }
        record.content = selection(self.fixed, &body_selection, budget.0).and_then(|body| {
            let content = ExplanationContent::FixedOwner {
                key: owner.key,
                reading_body: body,
            };
            budget.take(&content).then_some(content)
        });
        record.content_omitted = record.content.is_none();
        Ok(())
    }

    fn reading_body_selection(&self, owner: &OwnerMark) -> Result<TextSelection, ExplanationError> {
        let parts = self
            .reader
            .owner_body_parts(owner.key)
            .ok_or(ExplanationError::InvalidFixed)?;
        if parts
            .iter()
            .map(|part| part.slice)
            .eq(owner.direct_body.parts.iter().copied())
        {
            return Ok(owner.direct_body.clone());
        }
        // A union of direct, nested-owner and transparent-region selections
        // has no single native logical-join chain. Its physical rows remain
        // authoritative; do not invent searchable continuity across regions.
        Ok(TextSelection {
            parts: parts.iter().map(|part| part.slice).collect(),
            joins: vec![mant_ir::TextJoin::HardBoundary; parts.len().saturating_sub(1)],
        })
    }

    /// Attach one response-local native reading group without moving body
    /// bytes into an earlier owner or asserting name equivalence. The index
    /// has already closed the sibling chain against final owner facts.
    pub(super) fn attach_group(
        &self,
        candidate: &FixedCandidate,
        record: &mut ExplanationEvidence,
        pool: &mut Pool,
        budget: &mut Budget,
    ) -> Result<(), ExplanationError> {
        if candidate.class != EvidenceClass::DirectEntry {
            return Ok(());
        }
        let key = candidate.key.ok_or(ExplanationError::InvalidFixed)?;
        let Some(group) = self.index.fixed_reading_group(key) else {
            return Ok(());
        };
        if record.content.is_none() {
            record.support_omitted = true;
            return Ok(());
        }
        let provider = *group.members.last().ok_or(ExplanationError::InvalidFixed)?;
        if pool.failed_fixed_groups.contains(&provider) {
            record.support_omitted = true;
            return Ok(());
        }
        if let Some(&reference) = pool.fixed_groups.get(&provider) {
            if budget.take(&reference) {
                record.support = Some(reference);
            } else {
                record.support_omitted = true;
            }
            return Ok(());
        }
        let Some(support) = self.copy_group(group, budget.0)? else {
            pool.failed_fixed_groups.insert(provider);
            record.support_omitted = true;
            return Ok(());
        };
        let reference = pool.values.len();
        if !budget.take(&(reference, &support)) {
            pool.failed_fixed_groups.insert(provider);
            record.support_omitted = true;
            return Ok(());
        }
        pool.values.push(support);
        pool.fixed_groups.insert(provider, reference);
        record.support = Some(reference);
        Ok(())
    }

    fn copy_group(
        &self,
        group: &mant_ir::FixedReadingGroup,
        maximum_bytes: usize,
    ) -> Result<Option<ExplanationSupport>, ExplanationError> {
        let mut members = Vec::with_capacity(group.members.len());
        let mut positions = 0usize;
        let mut remaining_bytes = maximum_bytes;
        for &key in &group.members {
            let owner = self
                .fixed
                .owners
                .get((key.get() - 1) as usize)
                .ok_or(ExplanationError::InvalidFixed)?;
            let selected = self
                .indexed
                .get(&key)
                .ok_or(ExplanationError::InvalidFixed)?;
            positions = positions.saturating_add(owner.head.parts.len());
            if positions > mant_protocol::MAX_EXPLANATION_POSITIONS {
                return Ok(None);
            }
            let outline_bytes = self.trail_copy_bound(owner, selected)?;
            if outline_bytes > remaining_bytes {
                return Ok(None);
            }
            remaining_bytes -= outline_bytes;
            let Some(bytes) =
                selection_bytes(&owner.head).filter(|&bytes| bytes <= remaining_bytes)
            else {
                return Ok(None);
            };
            remaining_bytes -= bytes;
            let Some(head) = selection(self.fixed, &owner.head, bytes) else {
                return Ok(None);
            };
            members.push(ExplanationFixedGroupMember {
                key,
                outline: trail(&self.reader, &self.index, &self.indexed, owner, selected)?,
                head,
            });
        }
        let provider = self
            .fixed
            .owners
            .get(
                (group
                    .members
                    .last()
                    .ok_or(ExplanationError::InvalidFixed)?
                    .get()
                    - 1) as usize,
            )
            .ok_or(ExplanationError::InvalidFixed)?;
        let body = self.reading_body_selection(provider)?;
        if positions.saturating_add(body.parts.len()) > mant_protocol::MAX_EXPLANATION_POSITIONS {
            return Ok(None);
        }
        let Some(bytes) = selection_bytes(&body).filter(|&bytes| bytes <= remaining_bytes) else {
            return Ok(None);
        };
        let Some(reading_body) = selection(self.fixed, &body, bytes) else {
            return Ok(None);
        };
        Ok(Some(ExplanationSupport::FixedDeclarationGroup {
            members,
            reading_body,
        }))
    }

    /// Conservative allocation preflight for strings and bounded DTO nodes
    /// that `trail` would clone. It can omit a group whose eventual JSON might
    /// fit; the exact serialized-byte charge still happens after transfer.
    /// In particular, one long section title is repeated in every member;
    /// never copy all 256 before discovering it exceeds the work allowance.
    fn trail_copy_bound(
        &self,
        owner: &OwnerMark,
        selected: &IndexedOwner,
    ) -> Result<usize, ExplanationError> {
        let mut bytes = OutlinePath::DocumentRoot
            .to_string()
            .len()
            .saturating_add(DOCUMENT_ROOT_ID.len())
            .saturating_add(crate::selectors::DOCUMENT_ROOT_TITLE.len())
            .saturating_add(256);
        if let Some(section) = owner.section {
            for heading in self
                .reader
                .breadcrumbs(section)
                .ok_or(ExplanationError::InvalidFixed)?
            {
                let title_bytes =
                    selection_bytes(&heading.title).ok_or(ExplanationError::InvalidFixed)?;
                bytes = bytes
                    .saturating_add(title_bytes.saturating_mul(3))
                    .saturating_add(
                        self.reader
                            .path(heading.key)
                            .ok_or(ExplanationError::InvalidFixed)?
                            .to_string()
                            .len(),
                    )
                    .saturating_add(heading.id.as_str().len())
                    .saturating_add(256);
            }
        }
        let mut parent = owner.parent;
        while let Some(key) = parent {
            let mark = self
                .fixed
                .owners
                .get(usize::try_from(key.get() - 1).map_err(|_| ExplanationError::InvalidFixed)?)
                .ok_or(ExplanationError::InvalidFixed)?;
            if let Some(entry) = self.indexed.get(&key) {
                let facts = self
                    .index
                    .entry_at(&entry.path)
                    .ok_or(ExplanationError::InvalidFixed)?;
                bytes = bytes
                    .saturating_add(entry.path.to_string().len())
                    .saturating_add(facts.id.as_str().len())
                    .saturating_add(facts.forms.first().map_or(0, String::len))
                    .saturating_add(256);
            }
            parent = mark.parent;
        }
        let facts = self
            .index
            .entry_at(&selected.path)
            .ok_or(ExplanationError::InvalidFixed)?;
        bytes = bytes
            .saturating_add(selected.path.to_string().len())
            .saturating_add(facts.id.as_str().len())
            .saturating_add(facts.forms.first().map_or(0, String::len))
            .saturating_add(facts.names.iter().map(String::len).sum::<usize>())
            .saturating_add(256);
        Ok(bytes)
    }

    #[allow(clippy::too_many_lines)] // One bounded, all-or-none Fixed DTO transfer.
    pub(super) fn finish_optional(
        &self,
        candidate: &FixedCandidate,
        record: &mut ExplanationEvidence,
        budget: &mut Budget,
    ) -> Result<(), ExplanationError> {
        if candidate.class != EvidenceClass::DirectEntry {
            return Ok(());
        }
        let key = candidate.key.ok_or(ExplanationError::InvalidFixed)?;
        let owner = self.owner(candidate)?;
        let selected = self
            .indexed
            .get(&key)
            .ok_or(ExplanationError::InvalidFixed)?;
        let entry = self
            .index
            .entry_at(&selected.path)
            .ok_or(ExplanationError::InvalidFixed)?;
        let selections = if entry.forms.len() == 1 {
            vec![
                owner
                    .entry
                    .as_ref()
                    .and_then(|native| native.forms.first())
                    .ok_or(ExplanationError::InvalidFixed)?
                    .clone(),
            ]
        } else {
            self.fixed
                .option_component_forms(owner)
                .ok_or(ExplanationError::InvalidFixed)?
                .into_iter()
                .map(|(_, selection)| selection)
                .collect()
        };
        if entry.forms.len() != selections.len() {
            return Err(ExplanationError::InvalidFixed);
        }
        let name_positions = fixed_name_positions(entry)?;
        record.name_bindings_omitted =
            entry.names.len() > mant_protocol::MAX_EXPLANATION_NAME_BINDINGS;
        let mut forms = Vec::with_capacity(selections.len());
        let mut bindings = Vec::with_capacity(
            entry
                .names
                .len()
                .min(mant_protocol::MAX_EXPLANATION_NAME_BINDINGS),
        );
        let form_parts = selections
            .iter()
            .try_fold(0usize, |count, source| {
                count.checked_add(source.parts.len())
            })
            .unwrap_or(usize::MAX);
        let binding_positions = entry
            .names
            .len()
            .min(mant_protocol::MAX_EXPLANATION_NAME_BINDINGS);
        let match_positions = record
            .bases
            .iter()
            .filter(|basis| {
                matches!(
                    basis,
                    EvidenceBasis::Name { .. } | EvidenceBasis::Form { .. }
                )
            })
            .count();
        let mut complete = retained_fixed_positions(record)
            .saturating_add(form_parts)
            .saturating_add(binding_positions)
            .saturating_add(match_positions)
            <= mant_protocol::MAX_EXPLANATION_POSITIONS;
        for (index, source) in selections.iter().enumerate() {
            if !complete {
                break;
            }
            let expected = &entry.forms[index];
            let Some(form) = selection(self.fixed, source, budget.0) else {
                complete = false;
                break;
            };
            if form.complete_text().as_deref() != Some(expected.as_str()) {
                return Err(ExplanationError::InvalidFixed);
            }
            forms.push(form);
        }
        if complete {
            for (index, (form_index, start_scalar, end_scalar)) in name_positions
                .into_iter()
                .take(mant_protocol::MAX_EXPLANATION_NAME_BINDINGS)
                .enumerate()
            {
                bindings.push(ExplanationNameBinding {
                    name_index: u32::try_from(index).map_err(|_| ExplanationError::InvalidFixed)?,
                    occurrences: vec![ExplanationOccurrence {
                        source_occurrence_index: 0,
                        forms: Vec::new(),
                        fixed_forms: vec![ExplanationFixedFormRange {
                            form_index: u32::try_from(form_index)
                                .map_err(|_| ExplanationError::InvalidFixed)?,
                            start_scalar,
                            end_scalar,
                        }],
                        content: Vec::new(),
                    }],
                });
            }
        }
        record.entry = if complete {
            let details = ExplanationEntry {
                kind: entry.kind,
                case: entry.case,
                names: entry.names.clone(),
                forms: Vec::new(),
                fixed_forms: forms,
                name_bindings: bindings,
                alias_groups: Vec::new(),
                alias_of: None,
                value_domain: None,
            };
            budget.take(&details).then_some(details)
        } else {
            None
        };
        record.details_omitted = record.entry.is_none();
        for basis in &mut record.bases {
            let Some(details) = &record.entry else {
                if matches!(
                    basis,
                    EvidenceBasis::Name { .. } | EvidenceBasis::Form { .. }
                ) {
                    record.match_details_omitted = true;
                }
                continue;
            };
            let mut with_position = basis.clone();
            match &mut with_position {
                EvidenceBasis::Name { matches } => {
                    for matched in matches {
                        let index = details
                            .names
                            .iter()
                            .position(|name| name == &matched.name)
                            .ok_or(ExplanationError::InvalidFixed)?;
                        if let Some(binding) = details.name_bindings.get(index) {
                            matched.occurrences.push(binding.occurrences[0].clone());
                        } else {
                            record.match_details_omitted = true;
                        }
                    }
                }
                EvidenceBasis::Form { matches } => {
                    for matched in matches {
                        let index = matched.source_form_index as usize;
                        let expected = details
                            .fixed_forms
                            .get(index)
                            .ok_or(ExplanationError::InvalidFixed)?;
                        matched.occurrences.push(ExplanationOccurrence {
                            source_occurrence_index: 0,
                            forms: Vec::new(),
                            fixed_forms: vec![ExplanationFixedFormRange {
                                form_index: matched.source_form_index,
                                start_scalar: 0,
                                end_scalar: expected
                                    .complete_text()
                                    .ok_or(ExplanationError::InvalidFixed)?
                                    .chars()
                                    .count() as u64,
                            }],
                            content: Vec::new(),
                        });
                    }
                }
                EvidenceBasis::Identity { .. } | EvidenceBasis::Literal => continue,
                _ => return Err(ExplanationError::InvalidFixed),
            }
            if budget.take_growth(basis, &with_position) {
                *basis = with_position;
            } else {
                record.match_details_omitted = true;
            }
        }
        let (name_match, form_match, _, _) = selected.matched;
        record.match_details_omitted |= record.details_omitted && (name_match || form_match);
        Ok(())
    }

    pub(super) fn copy_previews(
        &self,
        candidate: &FixedCandidate,
        record: &mut ExplanationEvidence,
        budget: &mut Budget,
    ) -> Result<(), ExplanationError> {
        if candidate.hits.is_empty() {
            return Ok(());
        }
        if candidate
            .hits
            .iter()
            .all(|hit| hit.range.end - hit.range.start > budget.0)
        {
            record.previews_omitted = true;
            return Ok(());
        }
        let mut cache = self.preview_units.borrow_mut();
        if cache.is_none() {
            *cache = Some(
                crate::search::fixed_visible::FixedVisibleUnits::new(self.fixed)
                    .map_err(|_| ExplanationError::InvalidFixed)?,
            );
        }
        let units = cache.as_ref().ok_or(ExplanationError::InvalidFixed)?;
        for hit in &candidate.hits {
            if hit.range.end - hit.range.start > budget.0 {
                record.previews_omitted = true;
                continue;
            }
            let materialized = units
                .get(hit.unit)
                .ok_or(ExplanationError::InvalidFixed)?
                .materialize(self.fixed)
                .map_err(|_| ExplanationError::InvalidFixed)?;
            let expected = materialized
                .text
                .get(hit.range.clone())
                .ok_or(ExplanationError::InvalidFixed)?;
            let preview = hit_selection(&materialized, &hit.range).and_then(|source| {
                let selected_text = self.fixed.selection_text(&source)?;
                if selected_text != expected {
                    return None;
                }
                let selected = selection(self.fixed, &source, budget.0)?;
                let scalar_count = u32::try_from(selected_text.chars().count()).ok()?;
                let preview = ExplanationFixedPreview {
                    selection: selected,
                    match_start_scalar: 0,
                    match_end_scalar: scalar_count,
                    source: None,
                    clipped_before: hit.range.start != 0,
                    clipped_after: hit.range.end != materialized.text.len(),
                };
                preview.validate().ok()?;
                Some(preview)
            });
            if let Some(preview) = preview.filter(|preview| budget.take(preview)) {
                record.fixed_previews.push(preview);
            } else {
                record.previews_omitted = true;
            }
        }
        Ok(())
    }

    /// Scope pagination can revisit a document in another evidence class;
    /// releasing between segments bounds the live index to one document.
    pub(super) fn release_preview_units(&self) {
        self.preview_units.borrow_mut().take();
    }
}

fn hit_selection(
    unit: &crate::search::fixed_visible::units::FixedUnitText<'_>,
    hit: &Range<usize>,
) -> Option<TextSelection> {
    let mut parts = Vec::new();
    let mut joins = Vec::new();
    for part in &unit.parts {
        let start = hit.start.max(part.text_range.start);
        let end = hit.end.min(part.text_range.end);
        if start >= end {
            continue;
        }
        if parts.len() == mant_protocol::MAX_EXPLANATION_POSITIONS {
            return None;
        }
        if !parts.is_empty() {
            joins.push(part.piece.join_before.clone()?);
        }
        let start_byte = part
            .piece
            .slice
            .start_byte
            .checked_add(u64::try_from(start - part.text_range.start).ok()?)?;
        let end_byte = part
            .piece
            .slice
            .start_byte
            .checked_add(u64::try_from(end - part.text_range.start).ok()?)?;
        parts.push(mant_ir::OutputSlice {
            run: part.piece.slice.run,
            start_byte,
            end_byte,
        });
    }
    (!parts.is_empty()).then_some(TextSelection { parts, joins })
}

fn name_span(expected: &str, name: &str) -> Result<(u64, u64), ExplanationError> {
    let start_byte = if name == expected {
        0
    } else {
        expected.len() - expected.trim_start().len()
    };
    let end_byte = start_byte
        .checked_add(name.len())
        .ok_or(ExplanationError::InvalidFixed)?;
    if expected.get(start_byte..end_byte) != Some(name) {
        return Err(ExplanationError::InvalidFixed);
    }
    let start_scalar = expected[..start_byte].chars().count() as u64;
    let end_scalar = start_scalar + name.chars().count() as u64;
    Ok((start_scalar, end_scalar))
}

fn fixed_name_positions(
    entry: &mant_ir::SemanticEntry,
) -> Result<Vec<(usize, u64, u64)>, ExplanationError> {
    if entry.forms.len() == 1 && entry.names.len() > 1 {
        let form = &entry.forms[0];
        let aliases =
            mant_ir::literal_option_aliases(form).ok_or(ExplanationError::InvalidFixed)?;
        if aliases.len() != entry.names.len() {
            return Err(ExplanationError::InvalidFixed);
        }
        return aliases
            .into_iter()
            .zip(&entry.names)
            .map(|((name, range), expected)| {
                if &name != expected {
                    return Err(ExplanationError::InvalidFixed);
                }
                let start = form[..range.start].chars().count() as u64;
                let end = start + name.chars().count() as u64;
                Ok((0, start, end))
            })
            .collect();
    }
    if entry.forms.len() != entry.names.len() {
        return Err(ExplanationError::InvalidFixed);
    }
    entry
        .forms
        .iter()
        .zip(&entry.names)
        .enumerate()
        .map(|(index, (form, name))| {
            let (start, end) = name_span(form, name)?;
            Ok((index, start, end))
        })
        .collect()
}

fn retained_fixed_positions(record: &ExplanationEvidence) -> usize {
    let forms = record.entry.as_ref().map_or(0, |entry| {
        let form_parts = entry.fixed_forms.iter().fold(0usize, |count, selection| {
            count.saturating_add(selection.parts.len())
        });
        entry
            .name_bindings
            .iter()
            .fold(form_parts, |count, binding| {
                binding.occurrences.iter().fold(count, |count, occurrence| {
                    count.saturating_add(occurrence.fixed_forms.len())
                })
            })
    });
    let matches = record
        .bases
        .iter()
        .fold(0usize, |count, basis| match basis {
            EvidenceBasis::Name { matches } => matches.iter().fold(count, |count, matched| {
                matched.occurrences.iter().fold(count, |count, occurrence| {
                    count.saturating_add(occurrence.fixed_forms.len())
                })
            }),
            EvidenceBasis::Form { matches } => matches.iter().fold(count, |count, matched| {
                matched.occurrences.iter().fold(count, |count, occurrence| {
                    count.saturating_add(occurrence.fixed_forms.len())
                })
            }),
            _ => count,
        });
    let body = match &record.content {
        Some(ExplanationContent::FixedOwner { reading_body, .. }) => reading_body.parts.len(),
        _ => 0,
    };
    forms.saturating_add(matches).saturating_add(body)
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

fn selection(
    fixed: &FixedBody,
    source: &TextSelection,
    maximum_bytes: usize,
) -> Option<ExplanationFixedSelection> {
    if source.parts.len() > mant_protocol::MAX_EXPLANATION_POSITIONS
        || selection_bytes(source)? > maximum_bytes
    {
        return None;
    }
    let mut parts = Vec::with_capacity(source.parts.len());
    for slice in &source.parts {
        let run = fixed
            .surface
            .runs
            .get(usize::try_from(slice.run.get() - 1).ok()?)?;
        let text = fixed.surface.run_text(slice.run)?;
        let start = usize::try_from(slice.start_byte).ok()?;
        let end = usize::try_from(slice.end_byte).ok()?;
        let (column, width) = if start == 0 && end == text.len() {
            (run.column, run.width)
        } else {
            // A clipped run needs a checked UTF-8-scalar to terminal-cell
            // mapping. Never treat byte offsets as cell offsets or guess when
            // the native run width disagrees with this mapping.
            // Pinned term_ascii.c::utf8_getwidth calls the per-scalar
            // mant_mandoc_utf8_width hook. Grapheme-wide measurement would
            // undercount joined emoji and misplace a clipped native run.
            if native_cell_width(text)? != run.width {
                return None;
            }
            (
                run.column
                    .checked_add(native_cell_width(text.get(..start)?)?)?,
                native_cell_width(text.get(start..end)?)?,
            )
        };
        parts.push(ExplanationFixedPart {
            slice: *slice,
            row: run.row,
            run_column: run.column,
            column,
            width,
            style: run.label.style,
            text: text.get(start..end)?.to_owned(),
            source: run.label.source,
        });
    }
    let selection = ExplanationFixedSelection {
        parts,
        joins: source.joins.clone(),
    };
    selection.validate().ok()?;
    Some(selection)
}

fn native_cell_width(text: &str) -> Option<u32> {
    use unicode_width::UnicodeWidthChar;

    text.chars().try_fold(0_u32, |total, scalar| {
        total.checked_add(u32::try_from(scalar.width().unwrap_or(0)).ok()?)
    })
}

fn selection_bytes(source: &TextSelection) -> Option<usize> {
    let text = source.parts.iter().try_fold(0usize, |total, part| {
        total.checked_add(usize::try_from(part.end_byte.checked_sub(part.start_byte)?).ok()?)
    })?;
    source.joins.iter().try_fold(text, |total, join| {
        total.checked_add(match join {
            mant_ir::TextJoin::AuthoredSeparator(text)
            | mant_ir::TextJoin::GeneratedSeparator(text) => text.len(),
            _ => 0,
        })
    })
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
    let Some(section) = section else {
        return Ok(OutlineTrail {
            ancestors: Vec::new(),
            node: OutlineNodeReference::DocumentRoot {
                path: OutlinePath::DocumentRoot.to_string().into(),
                id: DOCUMENT_ROOT_ID.into(),
                title: crate::selectors::DOCUMENT_ROOT_TITLE.to_owned(),
            },
        });
    };
    let chain = reader
        .breadcrumbs(section)
        .ok_or(ExplanationError::InvalidFixed)?;
    let mut ancestors = vec![OutlineReference {
        path: OutlinePath::DocumentRoot.to_string().into(),
        id: DOCUMENT_ROOT_ID.into(),
        title: crate::selectors::DOCUMENT_ROOT_TITLE.to_owned(),
    }];
    for heading in chain.iter().take(chain.len().saturating_sub(1)) {
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
    let heading = chain.last().ok_or(ExplanationError::InvalidFixed)?;
    Ok(OutlineTrail {
        ancestors,
        node: OutlineNodeReference::DocumentSection {
            path: reader
                .path(section)
                .ok_or(ExplanationError::InvalidFixed)?
                .to_string()
                .into(),
            id: heading.id.clone(),
            title: reader
                .label(section)
                .ok_or(ExplanationError::InvalidFixed)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use mant_ir::{
        DisplayLabel, DisplayPoint, DisplayRole, DisplayRow, DisplayRun, DisplayStyle,
        DisplaySurface, DocumentBody, DocumentMeta, HeadingMark, LinkMark, LinkTarget, NodeId,
        OutputSlice, OwnerRole, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey,
        SourceRecord, TextJoin,
    };
    use mant_protocol::{ContentSelector, EntryProjection, OutlineNode};

    use super::*;

    fn key(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("fixture key is positive")
    }

    fn slices(keys: &[u32], joins: Vec<TextJoin>, lengths: &[u64]) -> TextSelection {
        TextSelection {
            parts: keys
                .iter()
                .map(|&run| OutputSlice {
                    run: key(run),
                    start_byte: 0,
                    end_byte: lengths[(run - 1) as usize],
                })
                .collect(),
            joins,
        }
    }

    #[allow(clippy::too_many_lines)] // One complete Fixed snapshot exposes every tested owner/section edge.
    fn fixture() -> ResolvedContent {
        // This is a synthetic, already-final Fixed snapshot: it tests IR and
        // projection contracts only, not a claim about roff line wrapping.
        let texts = [
            "PREFACE",
            "OPTIONS",
            "printf",
            "(3)",
            "first body",
            "empty body",
            "NESTED",
            "child text",
        ];
        let lengths = texts.map(|text| text.len() as u64);
        let mut arena = String::new();
        let mut rows = Vec::new();
        let mut runs = Vec::new();
        for (index, text) in texts.iter().enumerate() {
            let number = u32::try_from(index + 1).unwrap();
            let owner = match number {
                3..=5 => Some(key(1)),
                6 => Some(key(2)),
                _ => None,
            };
            let link = (3..=4).contains(&number).then(|| key(1));
            rows.push(DisplayRow {
                key: key(number),
                first_run: key(number),
                run_count: 1,
                column_count: u32::try_from(text.len()).unwrap(),
                break_after: index + 1 < texts.len(),
            });
            runs.push(DisplayRun {
                key: key(number),
                row: key(number),
                column: 0,
                width: u32::try_from(text.len()).unwrap(),
                byte_start: u64::try_from(arena.len()).unwrap(),
                byte_count: lengths[index],
                label: DisplayLabel {
                    owner,
                    link,
                    source: Some(SourceKey::FIRST),
                    style: DisplayStyle {
                        bold: false,
                        underline: false,
                    },
                    role: DisplayRole::Body,
                },
            });
            arena.push_str(text);
        }
        let hard = || TextJoin::HardBoundary;
        let mut fixed = FixedBody {
            surface: DisplaySurface {
                text: arena,
                rows,
                runs,
            },
            headings: vec![
                HeadingMark {
                    key: key(1),
                    id: NodeId::from("options"),
                    fragment_aliases: Vec::new(),
                    generated_fragment_aliases: Vec::new(),
                    rendered_fragment_aliases: Vec::new(),
                    parent: None,
                    level_hint: 1,
                    at: DisplayPoint::RunBoundary {
                        run: key(2),
                        byte: 0,
                    },
                    title: slices(&[2], Vec::new(), &lengths),
                    direct_body: slices(&[3, 4, 5, 6], vec![hard(), hard(), hard()], &lengths),
                    source: None,
                },
                HeadingMark {
                    key: key(2),
                    id: NodeId::from("nested"),
                    fragment_aliases: Vec::new(),
                    generated_fragment_aliases: Vec::new(),
                    rendered_fragment_aliases: Vec::new(),
                    parent: Some(key(1)),
                    level_hint: 2,
                    at: DisplayPoint::RunBoundary {
                        run: key(7),
                        byte: 0,
                    },
                    title: slices(&[7], Vec::new(), &lengths),
                    direct_body: slices(&[8], Vec::new(), &lengths),
                    source: None,
                },
            ],
            owners: vec![
                OwnerMark {
                    key: key(1),
                    id: NodeId::from("owner-printf"),
                    parent: None,
                    preceding_owner: None,
                    section: Some(key(1)),
                    role: OwnerRole::Definition,
                    head_role: None,
                    head_role_prefix: None,
                    head_components: Vec::new(),
                    entry: None,
                    head: slices(&[3, 4], vec![TextJoin::DirectContact], &lengths),
                    direct_body: slices(&[5], Vec::new(), &lengths),
                    empty_point: None,
                    source: None,
                },
                OwnerMark {
                    key: key(2),
                    id: NodeId::from("owner-empty"),
                    parent: None,
                    preceding_owner: None,
                    section: Some(key(1)),
                    role: OwnerRole::Definition,
                    head_role: None,
                    head_role_prefix: None,
                    head_components: Vec::new(),
                    entry: None,
                    head: TextSelection {
                        parts: Vec::new(),
                        joins: Vec::new(),
                    },
                    direct_body: slices(&[6], Vec::new(), &lengths),
                    empty_point: None,
                    source: None,
                },
            ],
            links: vec![LinkMark {
                key: key(1),
                target: Some(LinkTarget::External {
                    uri: "https://example.test/printf".into(),
                }),
                label: slices(&[3, 4], vec![TextJoin::DirectContact], &lengths),
                source: None,
            }],
            anchors: Vec::new(),
            regions: Vec::new(),
        };
        let form = fixed.owners[0].head.clone();
        fixed.owners[0].entry = Some(mant_ir::EntryFacts {
            name_bindings: vec![mant_ir::EntryNameBinding {
                name: 0,
                occurrences: vec![form.clone()],
                evidence: mant_ir::EntryNameEvidence::Lexical,
            }],
            alias_groups: Vec::new(),
            alias_of: None,
            forms: vec![form],
            id: fixed.owners[0].id.clone(),
            kind: mant_ir::EntryKind::Term,
            case: mant_ir::NameCase::Sensitive,
            names: vec!["printf(3)".into()],
            value_domain: None,
        });
        fixed.validate().expect("self-contained Fixed fixture");
        ResolvedContent {
            label: "Fixed fixture".into(),
            address: None,
            document: Some(Document {
                parser: None,
                sources: vec![SourceRecord {
                    key: SourceKey::FIRST,
                    identity: SourceIdentity::Anonymous {
                        name: "synthetic-fixed".into(),
                    },
                    format: SourceFormat::Man,
                    decoded_byte_length: 0,
                    content_sha256: None,
                    coordinates: SourceCoordinates::NativeNormalizedBytes,
                }],
                root_source: SourceKey::FIRST,
                body: DocumentBody::Fixed(fixed),
                meta: DocumentMeta::default(),
                fragment_aliases: Vec::new(),
                diagnostics: Vec::new(),
            }),
            tldr: None,
        }
    }

    #[test]
    fn complete_linked_cross_row_head_is_the_only_semantic_entry() {
        let resolved = fixture();
        let document = resolved.document.as_ref().unwrap();
        let index = SemanticIndex::build(document);
        let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
            unreachable!()
        };
        assert_eq!(fixed.links[0].label, fixed.owners[0].head);
        assert!(index.root().is_empty());
        assert_eq!(index.section("options").len(), 1);
        assert_eq!(index.section("options")[0].forms, ["printf(3)"]);
        assert_eq!(index.section("options")[0].names, ["printf(3)"]);
        assert!(index.section("nested").is_empty());

        let outline =
            crate::projection::build_outline_projection(&resolved, EntryProjection::All, None)
                .expect("Fixed semantic outline");
        assert_eq!(outline.nodes.len(), 2); // root preface, then OPTIONS
        assert_eq!(outline.nodes[0].path(), "root");
        let section = &outline.nodes[1];
        assert_eq!(section.path(), "1");
        assert_eq!(section.children().len(), 2); // entry, then nested section
        assert!(matches!(
            &section.children()[0],
            OutlineNode::DocumentEntry { forms, owner, .. }
                if forms.len() == 1 && forms[0] == "printf(3)" && matches!(owner.as_ref(), mant_ir::ContentReveal::FixedOwner { key: owner_key } if *owner_key == key(1))
        ));
        assert_eq!(section.children()[1].path(), "1.1");

        let selected = crate::projection::build_outline_projection(
            &resolved,
            EntryProjection::All,
            Some(ContentSelector::path("1.1")),
        )
        .expect("nested native section path");
        assert_eq!(selected.nodes[0].path(), "1.1");
    }

    #[test]
    fn explain_keeps_linked_form_coordinates_and_owner_local_body() {
        let resolved = fixture();
        let result = super::super::select_explanation(&resolved, "printf(3)").unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.counts.direct_entry.total, 1);
        assert_eq!(result.evidence[0].outline.path(), "1/e1");
        let record = &result.evidence[0];
        let entry = record.entry.as_ref().expect("bounded Fixed form details");
        assert_eq!(
            entry.fixed_forms[0].complete_text().as_deref(),
            Some("printf(3)")
        );
        assert_eq!(
            entry.fixed_forms[0]
                .parts
                .iter()
                .map(|part| part.row.get())
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        let occurrence = match &record.bases[0] {
            EvidenceBasis::Name { matches } => &matches[0].occurrences[0],
            other => panic!("expected native name evidence, got {other:?}"),
        };
        assert_eq!(
            occurrence.fixed_forms[0]
                .resolve(&entry.fixed_forms)
                .as_deref(),
            Some("printf(3)")
        );
        assert!(!record.name_bindings_omitted);
        assert_eq!(entry.name_bindings[0].name_index, 0);
        assert_eq!(
            entry.name_bindings[0].occurrences.as_slice(),
            std::slice::from_ref(occurrence)
        );
        let Some(ExplanationContent::FixedOwner {
            key: owner_key,
            reading_body,
        }) = &record.content
        else {
            panic!("Fixed owner body must not become a Flow block");
        };
        assert_eq!(*owner_key, key(1));
        assert_eq!(reading_body.complete_text().as_deref(), Some("first body"));
        assert!(
            !reading_body
                .parts
                .iter()
                .any(|part| part.text == "empty body")
        );
        result.validate_references().unwrap();
        serde_json::from_value::<QueryExplanation>(serde_json::to_value(&result).unwrap()).unwrap();

        // Pinned CVS man_macro.c::blk_imp retains the TP BODY after a zero-
        // width HEAD. Its visible text is ordinary context, not a declaration.
        let ordinary = super::super::select_explanation(&resolved, "empty body").unwrap();
        assert_eq!(ordinary.total, 1);
        assert_eq!(ordinary.evidence[0].class, EvidenceClass::ContextMention);
        assert_eq!(
            super::super::select_explanation(&resolved, "owner-empty")
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn fixed_match_facts_precede_optional_entry_and_binding_copies() {
        let resolved = fixture();
        let document = resolved.document.as_ref().unwrap();
        let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
            unreachable!()
        };
        let basis = EvidenceBasis::Name {
            matches: vec![ExplanationNameMatch {
                name: "printf(3)".into(),
                occurrences: Vec::new(),
            }],
        };
        let query = ExplanationQuery {
            entry: "printf(3)".into(),
            options: mant_protocol::ExplanationOptions {
                content_bytes: u32::try_from(serde_json::to_vec(&basis).unwrap().len()).unwrap(),
                ..Default::default()
            },
        };
        let (result, _) = super::response(&resolved, document, fixed, &query).unwrap();
        let record = &result.evidence[0];
        assert!(matches!(
            record.bases.as_slice(),
            [EvidenceBasis::Name { .. }]
        ));
        assert!(record.entry.is_none());
        assert!(record.details_omitted);
        assert!(record.match_details_omitted);
        result.validate_references().unwrap();
    }

    #[test]
    fn section_reader_keeps_direct_subtree_and_root_preface_distinct() {
        let resolved = fixture();
        let document = resolved.document.as_ref().unwrap();
        let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
            unreachable!()
        };
        let reader = FixedSectionReader::new(fixed).unwrap();
        let preface = reader.root_preface_parts().unwrap();
        assert_eq!(
            preface.iter().map(|part| part.text).collect::<Vec<_>>(),
            ["PREFACE"]
        );
        let direct = reader.direct_parts(key(1)).unwrap();
        assert_eq!(
            direct.iter().map(|part| part.text).collect::<Vec<_>>(),
            ["OPTIONS", "printf", "(3)", "first body", "empty body"]
        );
        let subtree = reader.subtree_parts(key(1)).unwrap();
        assert_eq!(
            subtree.iter().map(|part| part.text).collect::<Vec<_>>(),
            [
                "OPTIONS",
                "printf",
                "(3)",
                "first body",
                "empty body",
                "NESTED",
                "child text"
            ]
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One cross-source response fixture exercises all three domains.
    fn scoped_fixed_flow_and_tldr_share_one_candidate_page_and_source_index() {
        use mant_ir::{DocumentAddress, MarkdownOrigin};
        use mant_protocol::{
            DocumentScope, DocumentTraversal, ExplanationOptions, ResolvedDocumentScope,
            ScopedDocument,
        };

        let mut documents = vec![
            fixture(),
            crate::query_fixture::markdown(
                "# OPTIONS\n\n<!-- mant:entries role=term case=sensitive -->\n- `printf(3)`: Flow body.\n",
                None,
            )
            .unwrap(),
            crate::query_fixture::markdown(
                "<!-- mant:tldr:start -->\n# Short\n\n> Quick reference.\n<!-- mant:tldr:end -->\n",
                None,
            )
            .unwrap(),
        ];
        documents[2].document = None;
        let sources = ["fixed", "flow", "tldr"]
            .into_iter()
            .map(|path| ScopedDocument {
                address: DocumentAddress::Markdown {
                    path: path.into(),
                    origin: MarkdownOrigin::Documents,
                },
                depth: 0,
                root_indices: vec![],
                reached_from: vec![],
            })
            .collect::<Vec<_>>();
        for (source, content) in sources.iter().zip(&mut documents) {
            content.address = Some(source.address.clone());
        }
        let graph = ResolvedDocumentScope {
            reference_limits: Vec::new(),
            query: DocumentScope {
                documents: vec![],
                traversal: DocumentTraversal::default(),
            },
            documents: sources,
            edges: vec![],
            frontier: vec![],
            unresolved: vec![],
        };
        let input = crate::QueryScopeView::new(&graph, &documents).unwrap();
        let mut query = ExplanationQuery {
            entry: "printf(3)".into(),
            options: ExplanationOptions {
                limit: 1,
                ..ExplanationOptions::default()
            },
        };
        let first = super::super::scoped::explain(input, &query).unwrap();
        assert_eq!(first.total, 2);
        assert_eq!(first.evidence[0].document_index, 0);
        assert_eq!(first.evidence[0].evidence.ordinal, 0);
        assert!(matches!(
            first.evidence[0].evidence.content,
            Some(ExplanationContent::FixedOwner { .. })
        ));
        assert!(first.documents[0].content_projection.is_none());
        assert!(first.documents[0].source_context.is_some());
        assert!(first.documents[2].source_context.is_none());
        assert_eq!(first.documents[2].total, 0);
        assert_eq!(first.next_offset, Some(1));
        let decoded: mant_protocol::ScopeExplanation =
            serde_json::from_value(serde_json::to_value(&first).unwrap()).unwrap();
        assert_eq!(decoded.evidence[0].document_index, 0);

        query.options.offset = 1;
        let second = super::super::scoped::explain(input, &query).unwrap();
        assert_eq!(second.total, 2);
        assert_eq!(second.evidence[0].document_index, 1);
        assert_eq!(second.evidence[0].evidence.ordinal, 1);
        assert_eq!(second.next_offset, None);
        serde_json::from_value::<mant_protocol::ScopeExplanation>(
            serde_json::to_value(&second).unwrap(),
        )
        .unwrap();

        query.options.offset = 0;
        query.options.limit = 2;
        let together = super::super::scoped::explain(input, &query).unwrap();
        assert_eq!(
            together
                .evidence
                .iter()
                .map(|record| record.document_index)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert!(together.documents[0].content_projection.is_none());
        assert!(together.documents[1].content_projection.is_some());
        serde_json::from_value::<mant_protocol::ScopeExplanation>(
            serde_json::to_value(&together).unwrap(),
        )
        .unwrap();

        documents[0].document.as_mut().unwrap().sources.clear();
        let input = crate::QueryScopeView::new(&graph, &documents).unwrap();
        query.options.offset = 0;
        let failed = super::super::scoped::explain(input, &query).unwrap();
        assert_eq!(failed.failures.len(), 1);
        assert_eq!(failed.total, 1);
        assert_eq!(failed.documents.len(), 2);
        assert_eq!(failed.evidence[0].document_index, 0);
    }

    #[test]
    fn scoped_budget_reserves_all_fixed_match_facts_before_any_body_or_entry() {
        let documents = [fixture(), fixture()];
        let plans = documents
            .iter()
            .map(|content| {
                let document = content.document.as_ref().unwrap();
                let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
                    unreachable!()
                };
                super::super::plan::DocumentPlan::Fixed(
                    super::plan(content, document, fixed, "printf(3)").unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let name = EvidenceBasis::Name {
            matches: vec![ExplanationNameMatch {
                name: "printf(3)".into(),
                occurrences: Vec::new(),
            }],
        };
        let form = EvidenceBasis::Form {
            matches: vec![ExplanationFormMatch {
                source_form_index: 0,
                text: "printf(3)".into(),
                occurrences: Vec::new(),
            }],
        };
        let bytes = 2
            * (serde_json::to_vec(&name).unwrap().len() + serde_json::to_vec(&form).unwrap().len());
        let mut budget = Budget(bytes);
        let page = super::super::page::materialize(
            &plans,
            &[(0, 0, 0), (1, 0, 1)],
            "printf(3)",
            &mut budget,
        )
        .unwrap();
        assert_eq!(budget.0, 0);
        assert_eq!(page.evidence.len(), 2);
        for (index, record) in page.evidence.iter().enumerate() {
            assert_eq!(record.document_index, index);
            assert_eq!(record.evidence.bases, [name.clone(), form.clone()]);
            assert!(record.evidence.entry.is_none());
            assert!(record.evidence.content.is_none());
            assert!(record.evidence.details_omitted);
            assert!(record.evidence.content_omitted);
        }
    }
}
