//! Borrowed collection state: bounded priority retention before any body copy.
use super::{Candidate, LocatedNode};
use mant_ir::{Diagnostic, ResolvedContent};
use mant_protocol::{
    EvidenceBasis, EvidenceClass, ExplanationTruncation, MAX_EXPLANATION_CANDIDATES,
};
use std::collections::BTreeMap;

pub(super) struct CollectionPlan<'a> {
    pub supports: super::support::SupportIndex<'a>,
    pub content: &'a ResolvedContent,
    pub located: Vec<LocatedNode<'a>>,
    pub candidates: Vec<Candidate<'a>>,
    pub diagnostics: Vec<Diagnostic>,
    pub rejected_aliases: std::collections::BTreeSet<mant_ir::NodeId>,
    pub truncation: ExplanationTruncation,
}
impl Candidate<'_> {
    pub(super) fn class(&self) -> EvidenceClass {
        if self.located.is_none() {
            return EvidenceClass::ContextMention;
        }
        if self.bases.iter().any(|b| {
            matches!(
                b,
                EvidenceBasis::Name { .. }
                    | EvidenceBasis::Form { .. }
                    | EvidenceBasis::Identity { .. }
            )
        }) {
            EvidenceClass::DirectEntry
        } else if self
            .bases
            .iter()
            .any(|b| matches!(b, EvidenceBasis::Related { .. }))
        {
            EvidenceClass::RelatedEntry
        } else {
            EvidenceClass::EntryMention
        }
    }
}

#[derive(Default)]
pub(super) struct Candidates<'a> {
    records: BTreeMap<(EvidenceClass, usize), Candidate<'a>>,
    owners: BTreeMap<usize, (EvidenceClass, usize)>,
    pub truncated: bool,
}
impl<'a> Candidates<'a> {
    pub fn insert(&mut self, mut candidate: Candidate<'a>) {
        if let Some(owner) = candidate.located
            && let Some(key) = self.owners.remove(&owner)
        {
            let mut old = self.records.remove(&key).expect("registered owner");
            for basis in candidate.bases {
                if !old.bases.contains(&basis) {
                    old.bases.push(basis);
                }
            }
            for hit in candidate.hits {
                if old.hits.len() < 2 && !old.hits.iter().any(|h| h.path == hit.path) {
                    old.hits.push(hit);
                }
            }
            candidate = old;
        }
        let key = (candidate.class(), candidate.order);
        if self.records.len() == MAX_EXPLANATION_CANDIDATES {
            self.truncated = true;
            if self
                .records
                .last_key_value()
                .is_some_and(|(worst, _)| *worst <= key)
            {
                return;
            }
            let (_, removed) = self.records.pop_last().expect("full pool");
            if let Some(owner) = removed.located {
                self.owners.remove(&owner);
            }
        }
        if let Some(owner) = candidate.located {
            self.owners.insert(owner, key);
        }
        self.records.insert(key, candidate);
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Candidate<'a>> {
        self.records.values_mut()
    }
    pub fn finish(self) -> (Vec<Candidate<'a>>, bool) {
        (self.records.into_values().collect(), self.truncated)
    }
}
