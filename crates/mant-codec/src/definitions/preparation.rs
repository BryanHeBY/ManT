//! Normalize ownership and recognize each final head once, then freeze it for
//! counting and allocation. No consumer repeats role or name inference.
#![allow(clippy::similar_names)] // ContentContext and DefinitionContext are distinct inputs.
use std::collections::HashMap;

use mant_ir::{Block, ContentContext, DefinitionItem, Inline, Section, SourceSpan};

use super::groups::GroupMatchingPlan;
use super::{
    NativeHeadEvidence,
    context::{DefinitionContext, child_definition_context, definition_group_context},
    evidence::head_content,
    identity::{IdentityPlan, has_semantic_spelling, identity_plan, list_identity_base},
    normalize::{normalize_definition_nesting_with_boundaries, normalize_hanging_definitions},
};

pub(super) struct PreparedDefinitions {
    pub(super) preferred_counts: HashMap<String, usize>,
    pub(super) plans: Vec<PreparedDefinition>,
}

/// Only declaration heads are retained for validation, not descriptions or a
/// second document. Child normalization cannot change its parent's head.
pub(super) struct PreparedDefinition {
    source: Option<SourceSpan>,
    head: Vec<Vec<Inline>>,
    identity: IdentityPlan,
}

impl PreparedDefinition {
    fn matches(&self, item: &DefinitionItem) -> bool {
        self.source == item.source && self.head == head_content(&item.terms)
    }

    pub(super) fn for_item(self, item: &DefinitionItem) -> IdentityPlan {
        assert!(self.matches(item), "prepared declaration/source changed");
        self.identity
    }
}

pub(super) fn prepare(
    content: ContentContext<'_>,
    blocks: &mut Vec<Block>,
    sections: &mut [Section],
    context: DefinitionContext,
    evidence: &NativeHeadEvidence,
) -> PreparedDefinitions {
    // Ownership normalization may move definition owners between lists. Build
    // native pointer matching only after that movement is complete, otherwise
    // a valid macro expansion split by an ordinary paragraph looks like a
    // missing same-coordinate sibling.
    normalize_blocks(content, blocks, context, evidence);
    normalize_sections(content, sections, context, evidence);
    let mut group_matches = evidence.groups.matching_plan(blocks, sections);
    let mut prepared = PreparedDefinitions {
        preferred_counts: HashMap::new(),
        plans: Vec::new(),
    };
    prepared.blocks(content, blocks, context, evidence, &mut group_matches);
    prepared.sections(content, sections, context, evidence, &mut group_matches);
    prepared
}

impl PreparedDefinitions {
    fn sections(
        &mut self,
        content: ContentContext<'_>,
        sections: &mut [Section],
        parent: DefinitionContext,
        evidence: &NativeHeadEvidence,
        group_matches: &mut GroupMatchingPlan,
    ) {
        for section in sections {
            let context =
                DefinitionContext::for_section(&section.heading.plain_text(content), parent);
            self.blocks(
                content,
                &mut section.blocks,
                context,
                evidence,
                group_matches,
            );
            self.sections(
                content,
                &mut section.children,
                context,
                evidence,
                group_matches,
            );
        }
    }

    fn blocks(
        &mut self,
        content: ContentContext<'_>,
        blocks: &mut Vec<Block>,
        context: DefinitionContext,
        evidence: &NativeHeadEvidence,
        group_matches: &mut GroupMatchingPlan,
    ) {
        for block in blocks {
            match block {
                Block::List { items, .. } => {
                    for item in items {
                        if let Some(preferred) = list_identity_base(item) {
                            *self.preferred_counts.entry(preferred).or_default() += 1;
                        }
                        let child_context = item.entry.as_ref().map_or(context, |facts| {
                            child_definition_context(facts.kind, context)
                        });
                        self.blocks(
                            content,
                            &mut item.blocks,
                            child_context,
                            evidence,
                            group_matches,
                        );
                    }
                }
                Block::DefinitionList {
                    items,
                    declaration_groups,
                    ..
                } => {
                    let item_context = definition_group_context(content, items, context);
                    // Resolve native declaration witnesses while their
                    // parse-local owner markers still exist. Remove those
                    // markers before calculating public content-slice paths:
                    // anchors at the front of a term would otherwise shift
                    // every retained name-binding index.
                    let heads = items
                        .iter()
                        .map(|item| {
                            identity_plan(content, item, item_context, evidence.role(item))
                                .group_head
                        })
                        .collect::<Vec<_>>();
                    *declaration_groups =
                        evidence
                            .groups
                            .resolve(content, items, &heads, group_matches);
                    crate::definitions::remove_native_definition_owner_markers_from_items(items);
                    for item in items.iter_mut() {
                        let identity =
                            identity_plan(content, item, item_context, evidence.role(item));
                        if has_semantic_spelling(content, item, &identity) {
                            *self
                                .preferred_counts
                                .entry(identity.preferred.clone())
                                .or_default() += 1;
                        }
                        let child_context = child_definition_context(identity.kind, item_context);
                        self.plans.push(PreparedDefinition {
                            source: item.source,
                            head: head_content(&item.terms),
                            identity,
                        });
                        self.blocks(
                            content,
                            &mut item.description,
                            child_context,
                            evidence,
                            group_matches,
                        );
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &mut row.cells {
                            self.blocks(
                                content,
                                &mut cell.blocks,
                                context,
                                evidence,
                                group_matches,
                            );
                        }
                    }
                }
                Block::Paragraph { .. }
                | Block::Preformatted { .. }
                | Block::FixedDisplay { .. }
                | Block::Equation { .. }
                | Block::VerticalSpace { .. }
                | Block::ThematicBreak { .. }
                | Block::Unsupported { .. } => {}
            }
        }
    }
}

fn normalize_sections(
    content: ContentContext<'_>,
    sections: &mut [Section],
    parent: DefinitionContext,
    evidence: &NativeHeadEvidence,
) {
    for section in sections {
        let context = DefinitionContext::for_section(&section.heading.plain_text(content), parent);
        normalize_blocks(content, &mut section.blocks, context, evidence);
        normalize_sections(content, &mut section.children, context, evidence);
    }
}

fn normalize_blocks(
    content: ContentContext<'_>,
    blocks: &mut Vec<Block>,
    context: DefinitionContext,
    evidence: &NativeHeadEvidence,
) {
    normalize_definition_nesting_with_boundaries(blocks, &evidence.continuations);
    normalize_hanging_definitions(content, blocks, context);
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    let child_context = item.entry.as_ref().map_or(context, |facts| {
                        child_definition_context(facts.kind, context)
                    });
                    normalize_blocks(content, &mut item.blocks, child_context, evidence);
                }
            }
            Block::DefinitionList { items, .. } => {
                let item_context = definition_group_context(content, items, context);
                for item in items {
                    let identity = identity_plan(content, item, item_context, evidence.role(item));
                    let child_context = child_definition_context(identity.kind, item_context);
                    normalize_blocks(content, &mut item.description, child_context, evidence);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        normalize_blocks(content, &mut cell.blocks, context, evidence);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::FixedDisplay { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_content as fixture;

    #[test]
    fn preparation_freezes_names_before_ids_and_rejects_changed_heads() {
        let mut item = DefinitionItem {
            source: None,
            entry: None,
            terms: vec![vec![fixture::text("--mode=fast")]],
            description: Vec::new(),
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
        };
        let mut blocks = vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![item.clone()],
            compact: false,
            source: None,
            layout: mant_ir::LayoutHint::default(),
        }];
        let prepared = prepare(
            fixture::content(),
            &mut blocks,
            &mut [],
            DefinitionContext::Parameters,
            &NativeHeadEvidence::default(),
        );
        assert_eq!(prepared.plans.len(), 1);
        assert_eq!(prepared.preferred_counts.get("option-mode"), Some(&1));
        let plan = prepared.plans.into_iter().next().unwrap();
        assert_eq!(plan.identity.names, ["--mode"]);
        assert!(plan.matches(&item));
        let Block::DefinitionList { items, .. } = &blocks[0] else {
            unreachable!()
        };
        assert!(
            items[0].entry.is_none(),
            "preparation must not allocate a public ID"
        );
        item.terms[0].insert(0, fixture::anchor("allocated-later"));
        assert!(plan.matches(&item));
        item.terms[0].push(fixture::text(" changed"));
        assert!(!plan.matches(&item));
    }
}
