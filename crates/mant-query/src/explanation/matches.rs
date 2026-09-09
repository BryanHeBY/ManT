//! Retain bounded source ordinals at the point of matching, before page copying.
use mant_ir::EntryOwner;
use mant_protocol::{EvidenceBasis, MAX_EXPLANATION_MATCH_RECORDS};

#[derive(Default)]
pub(super) struct MatchPlan {
    pub names: Vec<usize>,
    pub forms: Vec<usize>,
    pub omitted: bool,
}

impl MatchPlan {
    pub(super) fn collect(
        owner: EntryOwner<'_>,
        names: &[String],
        query: &str,
    ) -> (Self, Vec<EvidenceBasis>) {
        let mut plan = Self::default();
        let mut bases = Vec::new();
        let case = owner.facts().expect("indexed owner").case;
        let mut has_name = false;
        for (index, name) in names.iter().enumerate() {
            if super::same(name, query, case) {
                has_name = true;
                if plan.names.len() < MAX_EXPLANATION_MATCH_RECORDS {
                    plan.names.push(index);
                } else {
                    plan.omitted = true;
                }
            }
        }
        if has_name {
            bases.push(EvidenceBasis::Name {
                matches: Vec::new(),
            });
        }
        let mut has_form = false;
        if let Some(forms) = owner.forms() {
            for (index, form) in forms.iter().enumerate() {
                if super::same(&mant_ir::inline_plain_text(form), query, case) {
                    has_form = true;
                    if plan.names.len() + plan.forms.len() < MAX_EXPLANATION_MATCH_RECORDS {
                        plan.forms.push(index);
                    } else {
                        plan.omitted = true;
                    }
                }
            }
        }
        if has_form {
            bases.push(EvidenceBasis::Form {
                matches: Vec::new(),
            });
        }
        (plan, bases)
    }
}
