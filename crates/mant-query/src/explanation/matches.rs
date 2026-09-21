//! Retain bounded source ordinals at the point of matching, before page copying.
use mant_ir::{ContentContext, EntryOwner};
use mant_protocol::{EvidenceBasis, MAX_EXPLANATION_MATCH_RECORDS};

#[derive(Default)]
pub(super) struct MatchPlan {
    pub names: Vec<usize>,
    pub forms: Vec<usize>,
    pub omitted: bool,
}

impl MatchPlan {
    pub(super) fn collect(
        content: ContentContext<'_>,
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
        if let Some(forms) = content
            .entry_forms(owner)
            .expect("document entry forms resolve in their own content store")
        {
            for (index, form) in forms.iter().enumerate() {
                let text = content
                    .plain_text(form)
                    .expect("document entry form resolves in its own content store");
                if super::same(&text, query, case) {
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
