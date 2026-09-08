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
struct OwnerOrigin {
    path: std::sync::Arc<str>,
    item: usize,
    definition: bool,
}
#[derive(Default)]
pub(super) struct SupportIndex<'a> {
    groups: Vec<Group<'a>>,
    owners: HashMap<usize, usize>,
    origins: HashMap<usize, OwnerOrigin>,
}

impl<'a> SupportIndex<'a> {
    pub(super) fn share_owner(
        &self,
        owner: Option<usize>,
        selected_owners: impl Iterator<Item = usize>,
        evidence: &mut ExplanationEvidence,
        pool: &mut Pool,
        budget: &mut Budget,
    ) {
        use mant_protocol::ExplanationContent;
        if !matches!(evidence.content, Some(ExplanationContent::Entry { .. })) {
            return;
        }
        let Some(index) = owner else {
            return;
        };
        let Some(origin) = self.origins.get(&index) else {
            return;
        };
        let prefix = format!(
            "{}/{}{}/",
            origin.path,
            if origin.definition { "d" } else { "i" },
            origin.item
        );
        if !selected_owners
            .filter_map(|owner| self.origins.get(&owner))
            .any(|child| child.path.starts_with(&prefix))
        {
            return;
        }
        let reference = pool.values.len();
        let content = ExplanationContent::SharedEntry {
            support: reference,
            path: Vec::new(),
            item_index: 0,
        };
        if !budget.take(&("owned-entry", &content)) {
            return;
        }
        let Some(ExplanationContent::Entry { block }) = evidence.content.take() else {
            unreachable!("checked owner content");
        };
        pool.values.push(ExplanationSupport::OwnedEntry { block });
        pool.entries.insert(index, reference);
        evidence.content = Some(content);
    }
    pub(super) fn depth(&self, owner: Option<usize>) -> usize {
        owner
            .and_then(|owner| self.origins.get(&owner))
            .map_or(usize::MAX, |origin| origin.path.len())
    }

    pub(super) fn attach_owner(
        &self,
        owner: Option<usize>,
        evidence: &mut ExplanationEvidence,
        pool: &Pool,
        budget: &mut Budget,
    ) {
        if evidence.content.is_some() {
            return;
        }
        let Some(origin) = owner.and_then(|owner| self.origins.get(&owner)) else {
            return;
        };
        for (reference, path) in self.containers(pool, &origin.path) {
            let content = mant_protocol::ExplanationContent::SharedEntry {
                support: reference,
                path,
                item_index: origin.item,
            };
            if content.referenced_owner(&pool.values).is_some_and(|owner| {
                owner
                    .facts()
                    .is_some_and(|facts| facts.id.as_str() == evidence.outline.node.id())
            }) && budget.take(&content)
            {
                evidence.content = Some(content);
                return;
            }
        }
    }

    /// Prefer the nearest returned fragment, with a stable pool-index tie.
    /// Hash iteration order must not select which body fits a tight budget.
    fn containers(
        &self,
        pool: &Pool,
        child: &str,
    ) -> Vec<(usize, Vec<mant_protocol::ExplanationBlockStep>)> {
        let mut matches = self
            .carriers(pool)
            .filter_map(|(parent, range, definition, reference)| {
                relative_source_path(parent, range, definition, child).map(|path| (reference, path))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(reference, path)| (path.len(), *reference));
        matches
    }

    fn carriers<'s>(
        &'s self,
        pool: &'s Pool,
    ) -> impl Iterator<Item = (&'s str, DeclarationGroup, bool, usize)> + 's {
        pool.copied
            .iter()
            .filter_map(|(&parent, &reference)| {
                matches!(
                    pool.values[reference],
                    ExplanationSupport::DeclarationGroup { .. }
                )
                .then_some((
                    self.groups[parent].path.as_str(),
                    self.groups[parent].range,
                    true,
                    reference,
                ))
            })
            .chain(pool.entries.iter().map(|(&parent, &reference)| {
                let origin = &self.origins[&parent];
                (
                    origin.path.as_ref(),
                    DeclarationGroup {
                        start_item: origin.item,
                        end_item: origin.item + 1,
                    },
                    origin.definition,
                    reference,
                )
            }))
    }
    pub(super) fn record(&mut self, block: &'a Block, path: &str, owners: &HashMap<usize, usize>) {
        let path: std::sync::Arc<str> = path.into();
        let pointers = match block {
            Block::List { items, .. } => items
                .iter()
                .map(|item| std::ptr::from_ref(item) as usize)
                .collect::<Vec<_>>(),
            Block::DefinitionList { items, .. } => items
                .iter()
                .map(|item| std::ptr::from_ref(item) as usize)
                .collect(),
            _ => return,
        };
        for (item, pointer) in pointers.into_iter().enumerate() {
            if let Some(&owner) = owners.get(&pointer) {
                self.origins.insert(
                    owner,
                    OwnerOrigin {
                        path: path.clone(),
                        item,
                        definition: matches!(block, Block::DefinitionList { .. }),
                    },
                );
            }
        }
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
                path: path.to_string(),
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
        // Only source containment permits reuse: never equal strings or IDs.
        // Parents are materialized before selected descendants; an inner-only
        // page still copies just its own group.
        let contained = self.containers(pool, &group.path);
        let has_container = !contained.is_empty();
        for (parent_reference, path) in contained {
            let value = group.members(located, budget).map(|members| {
                ExplanationSupport::ContainedDeclarationGroup {
                    block_path: group.path.clone(),
                    group: group.range,
                    members,
                    support: parent_reference,
                    path,
                }
            });
            if let Some(value) =
                value.filter(|value| value.items_in(&pool.values).is_some() && budget.take(value))
            {
                pool.values.push(value);
                pool.copied.insert(index, reference);
                evidence.support = Some(reference);
                evidence.content = Some(content);
                return;
            }
        }
        if has_container {
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
    entries: HashMap<usize, usize>,
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
    fn members(
        &self,
        located: &[LocatedNode<'_>],
        budget: &Budget,
    ) -> Option<Vec<mant_protocol::OutlineTrail>> {
        // Names/details already have their own bounds; do not materialize an
        // unbounded list of member trails outside the response copy budget.
        if self.owners.len() > mant_protocol::MAX_EXPLANATION_RESULTS as usize {
            return None;
        }
        self.owners
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
            .collect()
    }

    fn copy(
        &self,
        located: &[LocatedNode<'_>],
        budget: &mut Budget,
        reference: usize,
    ) -> Option<ExplanationSupport> {
        let members = self.members(located, budget)?;
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

/// Translate collector-owned paths, never user selectors. The first item is
/// rebased into the parent's returned group; all deeper coordinates are intact.
fn relative_source_path(
    parent: &str,
    parent_range: DeclarationGroup,
    definition: bool,
    child: &str,
) -> Option<Vec<mant_protocol::ExplanationBlockStep>> {
    use mant_protocol::ExplanationBlockStep as Step;
    let suffix = child.strip_prefix(parent)?.strip_prefix('/')?;
    let mut parts = suffix.split('/');
    let mut path = Vec::new();
    while let Some(part) = parts.next() {
        let (kind, number) = part.split_at_checked(1)?;
        let index: u32 = number.parse().ok()?;
        let step = match kind {
            "d" => Step::DefinitionItem { index },
            "i" => Step::ListItem { index },
            "b" => Step::Block { index },
            "r" => Step::TableCell {
                row: index,
                column: parts.next()?.strip_prefix('c')?.parse().ok()?,
            },
            _ => return None,
        };
        path.push(step);
    }
    let ((Step::DefinitionItem { index }, true) | (Step::ListItem { index }, false)) =
        (path.first_mut()?, definition)
    else {
        return None;
    };
    let original = usize::try_from(*index).ok()?;
    if !(parent_range.start_item..parent_range.end_item).contains(&original) {
        return None;
    }
    *index = u32::try_from(original - parent_range.start_item).ok()?;
    Some(path)
}
