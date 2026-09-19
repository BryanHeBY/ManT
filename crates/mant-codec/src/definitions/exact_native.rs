//! Allocate semantic facts for owners already fixed by native execution.

use super::{
    DefinitionContext, DefinitionDiscovery, NativeHeadRole,
    context::{child_definition_context, definition_group_context},
    identity::{
        IdentityPlan, document_anchor_ids, has_semantic_spelling, identify_item,
        identify_list_item, identity_plan, list_identity_base,
    },
};
use mant_ir::{Block, EntryNameEvidence, Inline, Section};
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

pub(crate) struct ExactNativeIdentityResult {
    pub(crate) retained: HashSet<String>,
    pub(crate) groupable: HashSet<String>,
}

fn prepare_sections(
    sections: &mut [Section],
    parent: DefinitionContext,
    evidence: &mut std::vec::IntoIter<ExactNativeDefinitionEvidence>,
    plans: &mut Vec<ExactPlan>,
    preferred: &mut HashMap<String, usize>,
    target_aliases: &HashMap<String, String>,
    authored_titles: &HashMap<String, String>,
) {
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
        );
        prepare_sections(
            &mut section.children,
            context,
            evidence,
            plans,
            preferred,
            target_aliases,
            authored_titles,
        );
    }
}

fn prepare_blocks(
    blocks: &mut [Block],
    context: DefinitionContext,
    evidence: &mut std::vec::IntoIter<ExactNativeDefinitionEvidence>,
    plans: &mut Vec<ExactPlan>,
    preferred: &mut HashMap<String, usize>,
    target_aliases: &HashMap<String, String>,
) {
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
                    );
                }
            }
            Block::DefinitionList { items, .. } => {
                let item_context = definition_group_context(items, context);
                for item in items {
                    let exact = evidence
                        .next()
                        .expect("every exact native definition has evidence");
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
                    );
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
                    );
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
) {
    for section in sections {
        allocate_blocks(&mut section.blocks, plans, discovery, groupable);
        allocate_sections(&mut section.children, plans, discovery, groupable);
    }
}

fn allocate_blocks(
    blocks: &mut [Block],
    plans: &mut std::vec::IntoIter<ExactPlan>,
    discovery: &mut DefinitionDiscovery<'_>,
    groupable: &mut HashSet<String>,
) {
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
                    allocate_blocks(&mut item.blocks, plans, discovery, groupable);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    let exact = plans
                        .next()
                        .expect("every exact native definition has a plan");
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
                    allocate_blocks(&mut item.description, plans, discovery, groupable);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                    allocate_blocks(&mut cell.blocks, plans, discovery, groupable);
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
}

/// Apply shared name grammar and ID policy to an exact native owner stream.
pub(crate) fn identify_exact_native_definitions(
    blocks: &mut [Block],
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
    document_name: Option<&str>,
    evidence: Vec<ExactNativeDefinitionEvidence>,
    target_aliases: &HashMap<String, String>,
    authored_titles: &HashMap<String, String>,
) -> ExactNativeIdentityResult {
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
    );
    prepare_sections(
        sections,
        root_context,
        &mut evidence,
        &mut plans,
        &mut preferred_counts,
        target_aliases,
        authored_titles,
    );
    assert!(
        evidence.next().is_none(),
        "all exact native evidence was consumed"
    );

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
    allocate_blocks(blocks, &mut plans, &mut discovery, &mut groupable);
    allocate_sections(sections, &mut plans, &mut discovery, &mut groupable);
    assert!(
        plans.next().is_none(),
        "all exact native plans were allocated"
    );
    ExactNativeIdentityResult {
        retained: discovery.retained,
        groupable,
    }
}
