//! Normalize ownership and recognize each final head once, then freeze it for
//! counting and allocation. No consumer repeats role or name inference.
use std::collections::HashMap;

use mant_ir::{Block, DefinitionItem, Inline, Section, SourceSpan};

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
    blocks: &mut Vec<Block>,
    sections: &mut [Section],
    context: DefinitionContext,
    evidence: &NativeHeadEvidence,
) -> PreparedDefinitions {
    let mut prepared = PreparedDefinitions {
        preferred_counts: HashMap::new(),
        plans: Vec::new(),
    };
    prepared.blocks(blocks, context, evidence);
    prepared.sections(sections, context, evidence);
    prepared
}

impl PreparedDefinitions {
    fn sections(
        &mut self,
        sections: &mut [Section],
        parent: DefinitionContext,
        evidence: &NativeHeadEvidence,
    ) {
        for section in sections {
            let context = DefinitionContext::for_section(&section.heading.plain_text(), parent);
            self.blocks(&mut section.blocks, context, evidence);
            self.sections(&mut section.children, context, evidence);
        }
    }

    fn blocks(
        &mut self,
        blocks: &mut Vec<Block>,
        context: DefinitionContext,
        evidence: &NativeHeadEvidence,
    ) {
        normalize_definition_nesting_with_boundaries(blocks, &evidence.continuations);
        normalize_hanging_definitions(blocks, context);
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
                        self.blocks(&mut item.blocks, child_context, evidence);
                    }
                }
                Block::DefinitionList {
                    items,
                    declaration_groups,
                    ..
                } => {
                    let item_context = definition_group_context(items, context);
                    let mut heads = Vec::with_capacity(items.len());
                    for item in items.iter_mut() {
                        let identity = identity_plan(item, item_context, evidence.role(item));
                        heads.push(identity.group_head);
                        if has_semantic_spelling(item, &identity) {
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
                        self.blocks(&mut item.description, child_context, evidence);
                    }
                    *declaration_groups = evidence.groups.resolve(items, &heads);
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &mut row.cells {
                            self.blocks(&mut cell.blocks, context, evidence);
                        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_freezes_names_before_ids_and_rejects_changed_heads() {
        let mut item = DefinitionItem {
            source: None,
            entry: None,
            terms: vec![vec![Inline::Text {
                value: "--mode=fast".into(),
            }]],
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
        item.terms[0].insert(0, Inline::anchor("allocated-later"));
        assert!(plan.matches(&item));
        item.terms[0].push(Inline::Text {
            value: " changed".into(),
        });
        assert!(!plan.matches(&item));
    }
}
