//! Budgeted transfer from one native Fixed plan into explanation DTOs.

use mant_ir::{DOCUMENT_ROOT_ID, OutlinePath, OwnerMark, TextSelection};
use mant_protocol::{
    EvidenceBasis, EvidenceClass, ExplanationContent, ExplanationEntry, ExplanationEvidence,
    ExplanationFixedFormRange, ExplanationFixedGroupMember, ExplanationFixedPreview,
    ExplanationFixedSelection, ExplanationFormMatch, ExplanationIdentityField,
    ExplanationNameBinding, ExplanationNameMatch, ExplanationOccurrence, ExplanationSupport,
};

use super::selection::{
    fixed_name_positions, hit_selection, retained_fixed_positions, selection, selection_bytes,
};
use super::{Budget, ExplanationError, Pool};
use super::{FixedCandidate, FixedPlan, IndexedOwner, mention_section_trail, trail};

impl FixedPlan<'_> {
    fn owner(&self, candidate: &FixedCandidate) -> Result<&OwnerMark, ExplanationError> {
        let key = candidate.key.ok_or(ExplanationError::InvalidFixed)?;
        self.fixed
            .owners
            .get((key.get() - 1) as usize)
            .ok_or(ExplanationError::InvalidFixed)
    }

    pub(in super::super) fn prepare(
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

    pub(in super::super) fn copy_body(
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
    pub(in super::super) fn attach_group(
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
    pub(in super::super) fn finish_optional(
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
        // EntryFacts is the current operation's validated name/form authority.
        // Reconstructing forms from option components here would discard one
        // complete non-option form containing several independent names.
        let selections = &owner
            .entry
            .as_ref()
            .ok_or(ExplanationError::InvalidFixed)?
            .forms;
        if entry.forms.len() != selections.len() {
            return Err(ExplanationError::InvalidFixed);
        }
        let name_positions = fixed_name_positions(self.fixed, owner, entry)?;
        let truncated_names = name_positions
            .iter()
            .map(|positions| {
                positions.len() > mant_protocol::MAX_EXPLANATION_OCCURRENCES
                    || positions
                        .iter()
                        .any(|forms| forms.len() > mant_protocol::MAX_EXPLANATION_FRAGMENTS)
            })
            .collect::<Vec<_>>();
        record.name_bindings_omitted = entry.names.len()
            > mant_protocol::MAX_EXPLANATION_NAME_BINDINGS
            || truncated_names.iter().any(|&truncated| truncated);
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
        let binding_positions = name_positions
            .iter()
            .take(mant_protocol::MAX_EXPLANATION_NAME_BINDINGS)
            .map(|positions| {
                positions
                    .iter()
                    .filter(|forms| forms.len() <= mant_protocol::MAX_EXPLANATION_FRAGMENTS)
                    .take(mant_protocol::MAX_EXPLANATION_OCCURRENCES)
                    .map(Vec::len)
                    .sum::<usize>()
            })
            .sum::<usize>();
        let match_positions = record.bases.iter().try_fold(0usize, |count, basis| {
            let added = match basis {
                EvidenceBasis::Name { matches } => {
                    matches.iter().try_fold(0usize, |sum, matched| {
                        let index = entry
                            .names
                            .iter()
                            .position(|name| name == &matched.name)
                            .ok_or(ExplanationError::InvalidFixed)?;
                        Ok::<usize, ExplanationError>(sum.saturating_add(
                            name_positions.get(index).map_or(0, |positions| {
                                positions
                                    .iter()
                                    .filter(|forms| {
                                        forms.len() <= mant_protocol::MAX_EXPLANATION_FRAGMENTS
                                    })
                                    .take(mant_protocol::MAX_EXPLANATION_OCCURRENCES)
                                    .map(Vec::len)
                                    .sum::<usize>()
                            }),
                        ))
                    })?
                }
                EvidenceBasis::Form { matches } => matches.len(),
                _ => 0,
            };
            Ok::<usize, ExplanationError>(count.saturating_add(added))
        })?;
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
            for (index, positions) in name_positions
                .into_iter()
                .take(mant_protocol::MAX_EXPLANATION_NAME_BINDINGS)
                .enumerate()
            {
                bindings.push(ExplanationNameBinding {
                    name_index: u32::try_from(index).map_err(|_| ExplanationError::InvalidFixed)?,
                    occurrences: positions
                        .into_iter()
                        .enumerate()
                        .filter(|(_, forms)| {
                            forms.len() <= mant_protocol::MAX_EXPLANATION_FRAGMENTS
                        })
                        .take(mant_protocol::MAX_EXPLANATION_OCCURRENCES)
                        .map(|(ordinal, fixed_forms)| {
                            Ok(ExplanationOccurrence {
                                source_occurrence_index: u32::try_from(ordinal)
                                    .map_err(|_| ExplanationError::InvalidFixed)?,
                                forms: Vec::new(),
                                fixed_forms,
                                content: Vec::new(),
                            })
                        })
                        .collect::<Result<Vec<_>, ExplanationError>>()?,
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
                        if let Some(binding) = details
                            .name_bindings
                            .iter()
                            .find(|binding| binding.name_index as usize == index)
                        {
                            if truncated_names.get(index) == Some(&true) {
                                record.match_details_omitted = true;
                            }
                            matched
                                .occurrences
                                .extend(binding.occurrences.iter().cloned());
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

    pub(in super::super) fn copy_previews(
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
    pub(in super::super) fn release_preview_units(&self) {
        self.preview_units.borrow_mut().take();
    }
}
