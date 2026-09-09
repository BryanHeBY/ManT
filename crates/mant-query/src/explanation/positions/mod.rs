//! Bound location domains only after their response targets are accepted.
mod projection;
use super::materialize::Budget;
use mant_ir::EntryOwner;
use mant_protocol::{
    EvidenceBasis, ExplanationEntry, ExplanationNameBinding, ExplanationOccurrence,
    MAX_EXPLANATION_NAME_BINDINGS, MAX_EXPLANATION_OCCURRENCES, MAX_EXPLANATION_POSITIONS,
};
pub(super) use projection::preview_range;

#[derive(Clone, Copy)]
pub(super) enum Domain {
    Forms,
    Content,
}

#[derive(Default)]
pub(super) struct PositionBudget {
    used: usize,
}

impl PositionBudget {
    pub(super) fn remaining(&self) -> usize {
        MAX_EXPLANATION_POSITIONS.saturating_sub(self.used)
    }
    pub(super) fn charge(&mut self, fragments: usize) {
        self.used += fragments;
    }
}

/// Add only bounded ordinary binding records; original names/forms stay intact.
pub(super) fn ordinary(
    owner: EntryOwner<'_>,
    entry: &mut ExplanationEntry,
    budget: &mut Budget,
) -> bool {
    if owner.validated_names().is_none() {
        return false;
    }
    let facts = owner.facts().expect("entry metadata has owner");
    let mut omitted = facts.name_bindings.len() > MAX_EXPLANATION_NAME_BINDINGS;
    for binding in facts
        .name_bindings
        .iter()
        .take(MAX_EXPLANATION_NAME_BINDINGS)
    {
        let Ok(name_index) = u32::try_from(binding.name) else {
            omitted = true;
            continue;
        };
        let mut next = entry.name_bindings.clone();
        next.push(ExplanationNameBinding {
            name_index,
            occurrences: Vec::new(),
        });
        if budget.take_growth(&entry.name_bindings, &next) {
            entry.name_bindings = next;
        } else {
            omitted = true;
        }
    }
    omitted
}

pub(super) fn attach(
    owner: EntryOwner<'_>,
    bases: &mut [EvidenceBasis],
    entry: Option<&mut ExplanationEntry>,
    domain: Domain,
    budget: &mut Budget,
    positions: &mut PositionBudget,
) -> (bool, bool) {
    let facts = owner.facts().expect("indexed owner");
    let mut matched_omitted = false;
    let mut names_omitted = false;
    for basis in bases {
        match basis {
            EvidenceBasis::Name { matches } => {
                for record in matches {
                    let mut bindings = facts
                        .name_bindings
                        .iter()
                        .filter(|binding| facts.names.get(binding.name) == Some(&record.name));
                    let (Some(binding), None) = (bindings.next(), bindings.next()) else {
                        matched_omitted = true;
                        continue;
                    };
                    matched_omitted |= attach_occurrences(
                        owner,
                        &binding.occurrences,
                        &mut record.occurrences,
                        domain,
                        budget,
                        positions,
                    );
                }
            }
            EvidenceBasis::Form { matches } => {
                for record in matches {
                    let Some(form) = facts.forms.get(record.source_form_index as usize) else {
                        matched_omitted = true;
                        continue;
                    };
                    let projected = match domain {
                        Domain::Forms => Some(ExplanationOccurrence {
                            forms: vec![mant_protocol::ExplanationFormRange {
                                form_index: record.source_form_index,
                                start_char: 0,
                                end_char: u32::try_from(record.text.chars().count())
                                    .expect("bounded matched form"),
                            }],
                            ..Default::default()
                        }),
                        Domain::Content => projection::occurrence(owner, form, domain),
                    };
                    matched_omitted |= !attach_one(
                        &mut record.occurrences,
                        0,
                        projected,
                        domain,
                        budget,
                        positions,
                    );
                }
            }
            _ => {}
        }
    }
    if let Some(entry) = entry {
        for record in &mut entry.name_bindings {
            let Some(binding) = facts
                .name_bindings
                .iter()
                .find(|binding| binding.name == record.name_index as usize)
            else {
                names_omitted = true;
                continue;
            };
            names_omitted |= attach_occurrences(
                owner,
                &binding.occurrences,
                &mut record.occurrences,
                domain,
                budget,
                positions,
            );
        }
    }
    (matched_omitted, names_omitted)
}

fn attach_occurrences(
    owner: EntryOwner<'_>,
    source: &[mant_ir::EntryForm],
    target: &mut Vec<ExplanationOccurrence>,
    domain: Domain,
    budget: &mut Budget,
    positions: &mut PositionBudget,
) -> bool {
    let mut omitted = source.len() > MAX_EXPLANATION_OCCURRENCES;
    for (index, form) in source.iter().take(MAX_EXPLANATION_OCCURRENCES).enumerate() {
        let projected = if positions.remaining() == 0 {
            None
        } else {
            projection::occurrence(owner, form, domain)
        };
        omitted |= !attach_one(
            target,
            u32::try_from(index).expect("bounded occurrence"),
            projected,
            domain,
            budget,
            positions,
        );
    }
    omitted
}

fn attach_one(
    target: &mut Vec<ExplanationOccurrence>,
    index: u32,
    projected: Option<ExplanationOccurrence>,
    domain: Domain,
    budget: &mut Budget,
    positions: &mut PositionBudget,
) -> bool {
    let Some(projected) = projected else {
        return false;
    };
    let fragments = projected.forms.len() + projected.content.len();
    if fragments > positions.remaining() {
        return false;
    }
    let mut next = target.clone();
    let ordinal = next.partition_point(|occurrence| occurrence.source_occurrence_index < index);
    if next
        .get(ordinal)
        .is_none_or(|value| value.source_occurrence_index != index)
    {
        next.insert(
            ordinal,
            ExplanationOccurrence {
                source_occurrence_index: index,
                ..Default::default()
            },
        );
    }
    match domain {
        Domain::Forms => next[ordinal].forms = projected.forms,
        Domain::Content => next[ordinal].content = projected.content,
    }
    if !budget.take_growth(target, &next) {
        return false;
    }
    *target = next;
    positions.charge(fragments);
    true
}
