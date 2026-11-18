//! Keep the published canonical section IDs and their authored aliases.
use super::{RoffSectionHeading, RoffSectionTargets};
use crate::mandoc::{LoweringContext, first_part_children, inline, targets};
use libmandoc_rs::{Node, NodeKind};
use mant_ir::Section;
use std::collections::HashSet;

pub(in crate::mandoc) struct PendingSectionIdentity {
    heading: RoffSectionHeading,
    base: String,
    full: crate::producer_identity::FullHeadingId,
}

impl LoweringContext<'_> {
    pub(in crate::mandoc) fn reserve_section_ids(&mut self, ids: &HashSet<String>) {
        self.explicit_targets.clone_from(ids);
        self.section_ids.reserve(ids);
    }

    pub(in crate::mandoc) fn record_section_identity(
        &mut self,
        node: &Node,
    ) -> Vec<mant_ir::FragmentAlias> {
        let heading = RoffSectionHeading::from_node(node);
        let head = first_part_children(node, NodeKind::Head);
        let base = if head.iter().all(|child| child.kind == NodeKind::Text) {
            section_id_base(&heading.title)
        } else {
            // Published IDs for macro-bearing headings historically used a
            // detached display label (for example `label-https-example-org`
            // for `Lk https://example.org Label`). Preserve those IDs while
            // resolving Sx solely through the authored namespace above.
            section_id_base(&inline::plain_text(&inline::lower_inline_nodes(
                head,
                self.default_name,
            )))
        };
        let full = self.section_ids.claim_full(&base);
        self.pending_section_ids.push(PendingSectionIdentity {
            heading,
            base,
            full,
        });
        targets::section_target(node)
            .filter(|target| self.explicit_targets.contains(target))
            .map(mant_ir::FragmentAlias::from)
            .into_iter()
            .collect()
    }

    pub(in crate::mandoc) fn complete_section_identities(&mut self, sections: &mut [Section]) {
        fn complete(
            sections: &mut [Section],
            ids: &mut crate::producer_identity::HeadingIds,
            plans: &mut impl Iterator<Item = PendingSectionIdentity>,
            targets: &mut RoffSectionTargets,
        ) {
            for section in sections {
                let plan = plans.next().expect("one identity per lowered heading");
                let (id, long_fragment) = ids.complete(&plan.base, plan.full);
                if let Some(fragment) = long_fragment
                    && !section.fragment_aliases.contains(&fragment)
                {
                    section.fragment_aliases.push(fragment);
                }
                section.id = id.clone().into();
                targets.register(plan.heading, id);
                complete(&mut section.children, ids, plans, targets);
            }
        }

        // The lowering walk has claimed every full spelling in pre-order.
        // A shortened ID must not occupy a later section's full spelling.
        let mut plans = std::mem::take(&mut self.pending_section_ids).into_iter();
        complete(
            sections,
            &mut self.section_ids,
            &mut plans,
            &mut self.authored_section_targets,
        );
        debug_assert!(plans.next().is_none());
    }
}

pub(in crate::mandoc) fn section_id_base(title: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in title.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            separator = false;
            slug.push(character);
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        "section".to_owned()
    } else if crate::producer_identity::is_reserved_selector(&slug) {
        format!("{slug}-section")
    } else {
        slug
    }
}
