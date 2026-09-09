//! Offline owner capabilities derived only from the returned occurrence page.
use std::collections::HashMap;

use crate::presentation::{ReferenceAttachment, reference_attachment, reference_badge};
use mant_ir::{ContentBlockStep, ContentLocation, ContentReveal, LinkTarget};
use mant_protocol::{
    OutlineNode, QueryOutline, ReferenceAssociation, ReferenceCount, ReferenceCoverageStatus,
    ReferenceProjectionMode,
};

#[derive(PartialEq, Eq, Hash)]
enum Owner {
    Document,
    Section(Vec<u32>),
    Item(Vec<u32>, Vec<ContentBlockStep>, u32),
}

impl Owner {
    fn semantic(owner: &ContentReveal) -> Option<Self> {
        match owner {
            ContentReveal::Owner {
                sections,
                blocks,
                item_index,
            } if sections.len() + blocks.len() <= mant_ir::MAX_CONTENT_DEPTH => {
                Some(Self::Item(sections.clone(), blocks.clone(), *item_index))
            }
            _ => None,
        }
    }
}

#[derive(Default)]
struct Group<'a> {
    targets: Vec<&'a LinkTarget>,
    node: Option<usize>,
    matches: usize,
}

#[derive(Default)]
pub(super) struct Badges(HashMap<usize, String>);

impl Badges {
    pub(super) fn build(outline: &QueryOutline) -> Self {
        let inventory = &outline.references;
        if inventory.policy.mode != ReferenceProjectionMode::All {
            return Self::default();
        }
        let mut groups: HashMap<Owner, Group<'_>> = HashMap::new();
        for record in inventory.records.iter().take(1000) {
            let forms = match &record.association {
                ReferenceAssociation::Valid { forms, .. } => Some(forms.as_slice()),
                _ => None,
            };
            let owner = match reference_attachment(&record.origin, forms) {
                ReferenceAttachment::Heading => match &record.origin {
                    ContentLocation::DocumentHeading { .. } => Some(Owner::Document),
                    ContentLocation::SectionHeading { sections, .. }
                        if sections.len() <= mant_ir::MAX_CONTENT_DEPTH =>
                    {
                        Some(Owner::Section(sections.clone()))
                    }
                    _ => None,
                },
                ReferenceAttachment::Form => match &record.association {
                    ReferenceAssociation::Valid { owner, .. } => Owner::semantic(owner),
                    _ => None,
                },
                ReferenceAttachment::Body => None,
            };
            if let Some(owner) = owner {
                groups
                    .entry(owner)
                    .or_default()
                    .targets
                    .push(&record.target);
            }
        }
        match_nodes(&outline.nodes, &mut groups);
        let complete = matches!(
            inventory.coverage.status,
            ReferenceCoverageStatus::Complete {}
        ) && matches!(inventory.occurrences, ReferenceCount::Exact { value } if value == inventory.records.len() as u64)
            && inventory.page.offset == 0
            && inventory.page.limited.is_none()
            && inventory.records.len() <= 1000;
        Self(
            groups
                .into_values()
                .filter_map(|group| {
                    (group.matches == 1).then(|| {
                        (
                            group.node.expect("matched node"),
                            reference_badge(group.targets, complete),
                        )
                    })
                })
                .collect(),
        )
    }

    pub(super) fn get(&self, node: &OutlineNode) -> Option<&str> {
        self.0
            .get(&(std::ptr::from_ref(node) as usize))
            .map(String::as_str)
    }
}

fn match_nodes(nodes: &[OutlineNode], groups: &mut HashMap<Owner, Group<'_>>) {
    for node in nodes {
        let owner = match node {
            OutlineNode::DocumentRoot { .. } => Some(Owner::Document),
            OutlineNode::DocumentEntry { owner, .. } => Owner::semantic(owner),
            OutlineNode::DocumentSection { path, .. } if path.len() <= 512 => path
                .split('.')
                .map(|part| part.parse::<u32>().ok()?.checked_sub(1))
                .collect::<Option<Vec<_>>>()
                .map(Owner::Section),
            _ => None,
        };
        if let Some(group) = owner.and_then(|owner| groups.get_mut(&owner)) {
            group.matches += 1;
            group.node = Some(std::ptr::from_ref(node) as usize);
        }
        match_nodes(node.children(), groups);
    }
}
