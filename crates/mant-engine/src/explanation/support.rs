//! Borrowed source groups are indexed once; only selected groups are copied.
use super::{
    LocatedNode,
    materialize::{Budget, trail},
};
use mant_ir::{Block, DeclarationGroup};
use mant_protocol::{EvidenceClass, ExplanationEvidence, ExplanationSupport};
use std::collections::HashMap;

struct Group<'a> {
    block: &'a Block,
    path: String,
    range: DeclarationGroup,
    owners: Vec<usize>,
}
#[derive(Default)]
pub(super) struct SupportIndex<'a> {
    groups: Vec<Group<'a>>,
    owners: HashMap<usize, usize>,
}

impl<'a> SupportIndex<'a> {
    pub(super) fn record(&mut self, block: &'a Block, path: &str, owners: &HashMap<usize, usize>) {
        let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = block
        else {
            return;
        };
        let mut end = 0;
        for &range in declaration_groups {
            if range.start_item < end {
                continue;
            }
            end = range.end_item;
            let Some(members) = range.resolve(items) else {
                continue;
            };
            let located = members
                .iter()
                .filter_map(|item| owners.get(&(std::ptr::from_ref(item) as usize)).copied())
                .collect::<Vec<_>>();
            // No incomplete list of identities masquerades as a complete group.
            if located.len() != members.len() {
                continue;
            }
            let index = self.groups.len();
            for &owner in &located {
                self.owners.insert(owner, index);
            }
            self.groups.push(Group {
                block,
                path: path.into(),
                range,
                owners: located,
            });
        }
    }

    pub(super) fn attach(
        &self,
        owner: Option<usize>,
        evidence: &mut ExplanationEvidence,
        located: &[LocatedNode<'_>],
        pool: &mut Pool,
        budget: &mut Budget,
    ) {
        if evidence.class != EvidenceClass::DirectEntry {
            return;
        }
        let Some(index) = owner.and_then(|owner| self.owners.get(&owner)).copied() else {
            return;
        };
        if let Some(&reference) = pool.copied.get(&index) {
            let content = mant_protocol::ExplanationContent::DeclarationMember {
                support: reference,
                item_index: group_member(&self.groups[index], owner),
            };
            if budget.take(&(reference, &content)) {
                evidence.support = Some(reference);
                evidence.content = Some(content);
            } else {
                evidence.support_omitted = true;
            }
            return;
        }
        let group = &self.groups[index];
        if pool.failed.contains(&index) {
            evidence.support_omitted = true;
            return;
        }
        let reference = pool.values.len();
        let content = mant_protocol::ExplanationContent::DeclarationMember {
            support: reference,
            item_index: group_member(group, owner),
        };
        if !budget.take(&content) {
            evidence.support_omitted = true;
            return;
        }
        if let Some(value) = group.copy(located, budget, reference) {
            pool.values.push(value);
            pool.copied.insert(index, reference);
            evidence.support = Some(reference);
            evidence.content = Some(content);
        } else {
            pool.failed.insert(index);
            evidence.support_omitted = true;
        }
    }
}

fn group_member(group: &Group<'_>, owner: Option<usize>) -> usize {
    group
        .owners
        .iter()
        .position(|&member| Some(member) == owner)
        .expect("indexed group member")
}

#[derive(Default)]
pub(super) struct Pool {
    copied: HashMap<usize, usize>,
    failed: std::collections::HashSet<usize>,
    pub values: Vec<ExplanationSupport>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Body<'a> {
    r#type: &'static str,
    items: &'a [mant_ir::DefinitionItem],
    declaration_groups: [DeclarationGroup; 1],
    compact: bool,
    layout: mant_ir::LayoutHint,
    source: Option<mant_ir::SourceSpan>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Borrowed<'a> {
    kind: &'static str,
    block_path: &'a str,
    group: DeclarationGroup,
    members: &'a [mant_protocol::OutlineTrail],
    block: Body<'a>,
}

impl Group<'_> {
    fn copy(
        &self,
        located: &[LocatedNode<'_>],
        budget: &mut Budget,
        reference: usize,
    ) -> Option<ExplanationSupport> {
        // Names/details already have their own bounds; do not materialize an
        // unbounded list of member trails outside the response copy budget.
        if self.owners.len() > mant_protocol::MAX_EXPLANATION_RESULTS as usize {
            return None;
        }
        let Block::DefinitionList {
            items,
            compact,
            layout,
            source,
            ..
        } = self.block
        else {
            return None;
        };
        let items = self.range.resolve(items)?;
        let members = self
            .owners
            .iter()
            .map(|&i| {
                let LocatedNode::Entry {
                    title,
                    breadcrumbs,
                    entry,
                    ..
                } = &located[i]
                else {
                    return None;
                };
                // Check borrowed string payloads before cloning a member trail.
                // The complete serialized support below accounts for all field
                // names, paths, metadata and array framing before body copying.
                let ancestors = breadcrumbs
                    .iter()
                    .map(|b| (&b.id, &b.title))
                    .collect::<Vec<_>>();
                budget
                    .fits(&(title, entry.names, ancestors))
                    .then(|| trail(&located[i]))
            })
            .collect::<Option<Vec<_>>>()?;
        let rebased = DeclarationGroup {
            start_item: 0,
            end_item: items.len(),
        };
        let value = Borrowed {
            kind: "declaration-group",
            block_path: &self.path,
            group: self.range,
            members: &members,
            block: Body {
                r#type: "definition-list",
                items,
                declaration_groups: [rebased],
                compact: *compact,
                layout: *layout,
                source: *source,
            },
        };
        // Tuple accounts for the evidence reference too. No body is cloned
        // until the whole original context has passed the bounded writer.
        if !budget.take(&(reference, &value)) {
            return None;
        }
        Some(ExplanationSupport::DeclarationGroup {
            block_path: self.path.clone(),
            group: self.range,
            members,
            block: Block::DefinitionList {
                items: items.to_vec(),
                declaration_groups: vec![rebased],
                compact: *compact,
                layout: *layout,
                source: *source,
            },
        })
    }
}
