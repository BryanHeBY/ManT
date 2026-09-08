//! Budgeted copies of matched facts and independently available entry metadata.
use super::{Candidate, materialize::Budget};
use mant_ir::{EntryForms, EntryOwner};
use mant_protocol::{EvidenceBasis, ExplanationEntry, ExplanationFormMatch, ExplanationNameMatch};
use serde::Serialize;

pub(super) fn matched(
    candidate: &Candidate<'_>,
    owner: Option<EntryOwner<'_>>,
    budget: &mut Budget,
) -> (Vec<EvidenceBasis>, bool) {
    let mut bases = candidate.bases.clone();
    let mut omitted = candidate.matched.omitted;
    let Some(owner) = owner else {
        return (bases, omitted);
    };
    let facts = owner.facts().expect("indexed owner");
    for basis in &mut bases {
        match basis {
            EvidenceBasis::Name { matches } => {
                for &index in &candidate.matched.names {
                    let record = ExplanationNameMatch {
                        name: facts.names[index].clone(),
                        occurrences: Vec::new(),
                    };
                    omitted |= !append(matches, record, budget);
                }
            }
            EvidenceBasis::Form { matches } => {
                for &index in &candidate.matched.forms {
                    let Some(form) = owner.form(&facts.forms[index]) else {
                        omitted = true;
                        continue;
                    };
                    let Ok(source_form_index) = u32::try_from(index) else {
                        omitted = true;
                        continue;
                    };
                    let record = ExplanationFormMatch {
                        source_form_index,
                        text: crate::inline::plain_text(&form),
                        occurrences: Vec::new(),
                    };
                    omitted |= !append(matches, record, budget);
                }
            }
            _ => {}
        }
    }
    (bases, omitted)
}

fn append<T: Serialize + Clone>(values: &mut Vec<T>, value: T, budget: &mut Budget) -> bool {
    let mut next = values.clone();
    next.push(value);
    let fits = if values.is_empty() {
        budget.take(&next)
    } else {
        budget.take_growth(values, &next)
    };
    if fits {
        *values = next;
    }
    fits
}

struct Forms<'a, 'b>(&'b EntryForms<'a>);
impl Serialize for Forms<'_, '_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryDetails<'a, 'b> {
    kind: mant_ir::EntryKind,
    case: mant_ir::NameCase,
    names: &'a [String],
    forms: Forms<'a, 'b>,
    name_bindings: &'a [mant_protocol::ExplanationNameBinding],
    alias_groups: &'a [Vec<String>],
    #[serde(skip_serializing_if = "Option::is_none")]
    alias_of: Option<&'a mant_ir::NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_domain: Option<&'a mant_ir::ValueDomain>,
}

pub(super) fn entry(
    owner: EntryOwner<'_>,
    rejected: &std::collections::BTreeSet<mant_ir::NodeId>,
    budget: &mut Budget,
) -> Option<ExplanationEntry> {
    let facts = owner.facts()?;
    let names = owner.validated_names().unwrap_or_default();
    let forms = owner.forms().unwrap_or_default();
    let alias_groups = owner.validated_alias_groups().unwrap_or_default();
    let alias_of = facts
        .alias_of
        .as_ref()
        .filter(|_| !rejected.contains(&facts.id));
    let details = EntryDetails {
        kind: facts.kind,
        case: facts.case,
        names,
        forms: Forms(&forms),
        name_bindings: &[],
        alias_groups,
        alias_of,
        value_domain: facts.value_domain.as_ref(),
    };
    budget.take(&details).then(|| ExplanationEntry {
        kind: facts.kind,
        case: facts.case,
        names: names.to_vec(),
        forms: forms.into_owned(),
        name_bindings: Vec::new(),
        alias_groups: alias_groups.to_vec(),
        alias_of: alias_of.cloned(),
        value_domain: facts.value_domain.clone(),
    })
}
