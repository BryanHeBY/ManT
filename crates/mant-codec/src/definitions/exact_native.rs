//! Allocate semantic facts for owners already fixed by native execution.

use super::{
    DefinitionContext, DefinitionDiscovery, NativeHeadRole,
    context::{child_definition_context, definition_group_context},
    identity::{
        IdentityPlan, document_anchor_ids, has_semantic_spelling, identify_item,
        identify_list_item, identity_plan, list_identity_base,
    },
};
use mant_ir::{Block, EntryNameEvidence, Inline, NodeId, Section};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExactNativeNameEvidence {
    pub(crate) term_index: usize,
    pub(crate) parts: Vec<Range<usize>>,
}

/// Exact producer evidence paired with one already materialized native
/// definition slot. The stream follows the final structural owner walk; no
/// source coordinate, rendered text, pointer, or marker rediscovers an owner.
#[derive(Clone, Debug, Default)]
pub(crate) struct ExactNativeDefinitionEvidence {
    pub(crate) role: Option<NativeHeadRole>,
    pub(crate) markup: Vec<ExactNativeNameEvidence>,
    pub(crate) native_arguments: Vec<ExactNativeNameEvidence>,
}

impl ExactNativeDefinitionEvidence {
    pub(crate) fn shift_terms(&mut self, offset: usize) {
        for occurrence in &mut self.markup {
            occurrence.term_index += offset;
        }
        for occurrence in &mut self.native_arguments {
            occurrence.term_index += offset;
        }
    }

    pub(crate) fn append(&mut self, mut other: Self) {
        if other.role.is_some() {
            self.role = other.role;
        }
        self.markup.append(&mut other.markup);
        self.native_arguments.append(&mut other.native_arguments);
    }
}

struct ExactPlan {
    identity: IdentityPlan,
    markup_names: HashSet<String>,
    group_head: bool,
}

#[derive(Debug)]
pub(crate) struct ExactNativeIdentityResult {
    pub(crate) retained: HashSet<String>,
    pub(crate) groupable: HashSet<String>,
    /// Identity assigned to each exact owner in the input evidence order.
    /// `None` denotes a native definition that intentionally remained prose.
    pub(crate) allocated: Vec<Option<NodeId>>,
}

/// Structural mismatch between the final definition walk and K23's exact
/// native evidence stream.
///
/// The checked K23 entry point returns these errors instead of relying on
/// iterator `expect`/`assert` calls.  The legacy wrapper remains temporarily
/// infallible until the production route is switched atomically.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExactNativeIdentityError {
    MissingEvidence,
    ExcessEvidence,
    MissingPlan,
    ExcessPlan,
}

impl std::fmt::Display for ExactNativeIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MissingEvidence => "final native definition has no semantic evidence",
            Self::ExcessEvidence => "semantic evidence has no final native definition",
            Self::MissingPlan => "final native definition has no prepared identity plan",
            Self::ExcessPlan => "prepared identity plan has no final native definition",
        })
    }
}

impl std::error::Error for ExactNativeIdentityError {}

fn prepare_sections(
    sections: &mut [Section],
    parent: DefinitionContext,
    evidence: &mut std::vec::IntoIter<ExactNativeDefinitionEvidence>,
    plans: &mut Vec<ExactPlan>,
    preferred: &mut HashMap<String, usize>,
    target_aliases: &HashMap<String, String>,
    authored_titles: &HashMap<String, String>,
) -> Result<(), ExactNativeIdentityError> {
    for section in sections {
        let title = authored_titles
            .get(section.id.as_str())
            .map_or_else(|| section.heading.plain_text(), Clone::clone);
        let context = DefinitionContext::for_section(&title, parent);
        prepare_blocks(
            &mut section.blocks,
            context,
            evidence,
            plans,
            preferred,
            target_aliases,
        )?;
        prepare_sections(
            &mut section.children,
            context,
            evidence,
            plans,
            preferred,
            target_aliases,
            authored_titles,
        )?;
    }
    Ok(())
}

fn prepare_blocks(
    blocks: &mut [Block],
    context: DefinitionContext,
    evidence: &mut std::vec::IntoIter<ExactNativeDefinitionEvidence>,
    plans: &mut Vec<ExactPlan>,
    preferred: &mut HashMap<String, usize>,
    target_aliases: &HashMap<String, String>,
) -> Result<(), ExactNativeIdentityError> {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    if let Some(base) = list_identity_base(item) {
                        *preferred.entry(base).or_default() += 1;
                    }
                    let child = item.entry.as_ref().map_or(context, |facts| {
                        child_definition_context(facts.kind, context)
                    });
                    prepare_blocks(
                        &mut item.blocks,
                        child,
                        evidence,
                        plans,
                        preferred,
                        target_aliases,
                    )?;
                }
            }
            Block::DefinitionList { items, .. } => {
                let item_context = definition_group_context(items, context);
                for item in items {
                    let exact = evidence
                        .next()
                        .ok_or(ExactNativeIdentityError::MissingEvidence)?;
                    let mut identity = identity_plan(item, item_context, exact.role);
                    identity.add_native_environment_arguments(
                        item,
                        exact.role,
                        &exact.native_arguments,
                    );
                    identity.retain_names_with_native_origin(exact.role, &exact.markup);
                    if let Some(anchor) = target_aliases.get(&identity.preferred) {
                        for term in &mut item.terms {
                            merge_native_anchor(term, anchor, &identity.preferred);
                        }
                    }
                    let markup_names = identity.names_with_native_markup(&exact.markup);
                    let group_head = identity.group_head;
                    if has_semantic_spelling(item, &identity) {
                        *preferred.entry(identity.preferred.clone()).or_default() += 1;
                    }
                    let child = child_definition_context(identity.kind, item_context);
                    plans.push(ExactPlan {
                        identity,
                        markup_names,
                        group_head,
                    });
                    prepare_blocks(
                        &mut item.description,
                        child,
                        evidence,
                        plans,
                        preferred,
                        target_aliases,
                    )?;
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                    prepare_blocks(
                        &mut cell.blocks,
                        context,
                        evidence,
                        plans,
                        preferred,
                        target_aliases,
                    )?;
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
    Ok(())
}

fn merge_native_anchor(inlines: &mut [Inline], old_id: &str, entry_id: &str) {
    for inline in inlines {
        match inline {
            Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } if id.as_str() == old_id => {
                *id = entry_id.into();
                fragment_aliases.retain(|alias| alias.as_str() != entry_id);
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => merge_native_anchor(children, old_id, entry_id),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
    }
}

fn allocate_sections(
    sections: &mut [Section],
    plans: &mut std::vec::IntoIter<ExactPlan>,
    discovery: &mut DefinitionDiscovery<'_>,
    groupable: &mut HashSet<String>,
    allocated: &mut Vec<Option<NodeId>>,
) -> Result<(), ExactNativeIdentityError> {
    for section in sections {
        allocate_blocks(&mut section.blocks, plans, discovery, groupable, allocated)?;
        allocate_sections(
            &mut section.children,
            plans,
            discovery,
            groupable,
            allocated,
        )?;
    }
    Ok(())
}

fn allocate_blocks(
    blocks: &mut [Block],
    plans: &mut std::vec::IntoIter<ExactPlan>,
    discovery: &mut DefinitionDiscovery<'_>,
    groupable: &mut HashSet<String>,
    allocated: &mut Vec<Option<NodeId>>,
) -> Result<(), ExactNativeIdentityError> {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    identify_list_item(
                        item,
                        &mut discovery.used,
                        discovery.reserved,
                        &mut discovery.retained,
                        discovery.preferred_counts,
                    );
                    allocate_blocks(&mut item.blocks, plans, discovery, groupable, allocated)?;
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    let exact = plans.next().ok_or(ExactNativeIdentityError::MissingPlan)?;
                    identify_item(
                        item,
                        exact.identity,
                        &mut discovery.used,
                        discovery.reserved,
                        &mut discovery.retained,
                        discovery.preferred_counts,
                    );
                    if exact.group_head
                        && let Some(facts) = item.entry.as_ref()
                    {
                        groupable.insert(facts.id.to_string());
                    }
                    if let Some(facts) = item.entry.as_mut() {
                        for binding in &mut facts.name_bindings {
                            if facts
                                .names
                                .get(binding.name)
                                .is_some_and(|name| exact.markup_names.contains(name))
                            {
                                binding.evidence = EntryNameEvidence::NativeMarkup;
                            }
                        }
                    }
                    allocated.push(item.entry.as_ref().map(|facts| facts.id.clone()));
                    allocate_blocks(
                        &mut item.description,
                        plans,
                        discovery,
                        groupable,
                        allocated,
                    )?;
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                    allocate_blocks(&mut cell.blocks, plans, discovery, groupable, allocated)?;
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
    Ok(())
}

/// Checked K23 entry point for applying semantic identities to an exact,
/// stable native owner stream.
pub(crate) fn identify_exact_native_definitions_checked(
    blocks: &mut [Block],
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
    document_name: Option<&str>,
    evidence: Vec<ExactNativeDefinitionEvidence>,
    target_aliases: &HashMap<String, String>,
    authored_titles: &HashMap<String, String>,
) -> Result<ExactNativeIdentityResult, ExactNativeIdentityError> {
    let root_context = document_name.map_or(DefinitionContext::Generic, |name| {
        let name = name.to_ascii_lowercase();
        if name.ends_with("_config") || name.ends_with("-config") {
            DefinitionContext::ConfigurationKeys
        } else {
            DefinitionContext::Generic
        }
    });
    let mut evidence = evidence.into_iter();
    let mut plans = Vec::new();
    let mut preferred_counts = HashMap::new();
    prepare_blocks(
        blocks,
        root_context,
        &mut evidence,
        &mut plans,
        &mut preferred_counts,
        target_aliases,
    )?;
    prepare_sections(
        sections,
        root_context,
        &mut evidence,
        &mut plans,
        &mut preferred_counts,
        target_aliases,
        authored_titles,
    )?;
    if evidence.next().is_some() {
        return Err(ExactNativeIdentityError::ExcessEvidence);
    }

    let used = document_anchor_ids(blocks, sections);
    let mut discovery = DefinitionDiscovery {
        retained: used.clone(),
        used,
        reserved: reserved_targets,
        preferred_counts: &preferred_counts,
        plans: Vec::new().into_iter(),
    };
    let mut plans = plans.into_iter();
    let mut groupable = HashSet::new();
    let mut allocated = Vec::new();
    allocate_blocks(
        blocks,
        &mut plans,
        &mut discovery,
        &mut groupable,
        &mut allocated,
    )?;
    allocate_sections(
        sections,
        &mut plans,
        &mut discovery,
        &mut groupable,
        &mut allocated,
    )?;
    if plans.next().is_some() {
        return Err(ExactNativeIdentityError::ExcessPlan);
    }
    Ok(ExactNativeIdentityResult {
        retained: discovery.retained,
        groupable,
        allocated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::{DefinitionItem, DefinitionLayout};

    fn checked(
        blocks: &mut [Block],
        evidence: Vec<ExactNativeDefinitionEvidence>,
    ) -> Result<ExactNativeIdentityResult, ExactNativeIdentityError> {
        identify_exact_native_definitions_checked(
            blocks,
            &mut [],
            &HashSet::new(),
            None,
            evidence,
            &HashMap::new(),
            &HashMap::new(),
        )
    }

    #[test]
    fn checked_entry_rejects_excess_evidence() {
        assert_eq!(
            checked(&mut [], vec![ExactNativeDefinitionEvidence::default()]).unwrap_err(),
            ExactNativeIdentityError::ExcessEvidence
        );
    }

    #[test]
    fn checked_entry_rejects_missing_evidence() {
        let mut blocks = vec![Block::DefinitionList {
            items: vec![DefinitionItem {
                terms: vec![vec![Inline::Text {
                    value: "--probe".to_owned(),
                }]],
                description: Vec::new(),
                entry: None,
                layout: DefinitionLayout::default(),
                source: None,
            }],
            declaration_groups: Vec::new(),
            compact: false,
            layout: Default::default(),
            source: None,
        }];
        assert_eq!(
            checked(&mut blocks, Vec::new()).unwrap_err(),
            ExactNativeIdentityError::MissingEvidence
        );
    }
}
