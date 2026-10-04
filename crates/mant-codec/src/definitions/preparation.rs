//! Normalize ownership and recognize each final head once, then freeze it for
//! counting and allocation. No consumer repeats role or name inference.
use std::collections::HashMap;

#[cfg(test)]
use mant_ir::Inline;
use mant_ir::{Block, DefinitionItem, Section, SourceSpan};

use super::groups::GroupMatchingPlan;
use super::{
    NativeHeadEvidence,
    context::{DefinitionContext, child_definition_context, definition_group_context},
    evidence::HeadSnapshot,
    identity::{IdentityPlan, has_semantic_spelling, identity_plan, list_identity_base},
    normalize::{
        normalize_definition_nesting_with_boundaries, normalize_hanging_definitions_with_evidence,
    },
};

pub(super) struct PreparedDefinitions {
    pub(super) preferred_counts: HashMap<String, usize>,
    pub(super) plans: Vec<PreparedDefinition>,
    pub(super) diagnostics: Vec<mant_ir::Diagnostic>,
}

/// Only declaration heads are retained for validation, not descriptions or a
/// second document. Child normalization cannot change its parent's head.
pub(super) struct PreparedDefinition {
    source: Option<SourceSpan>,
    head: HeadSnapshot,
    identity: IdentityPlan,
}

impl PreparedDefinition {
    fn matches(&self, item: &DefinitionItem) -> bool {
        self.source == item.source && super::evidence::head_matches(&item.terms, &self.head)
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
    // Ownership normalization may move definition owners between lists. Build
    // native pointer matching only after that movement is complete, otherwise
    // a valid macro expansion split by an ordinary paragraph looks like a
    // missing same-coordinate sibling.
    let mut diagnostics = Vec::new();
    normalize_blocks(blocks, context, evidence, &mut diagnostics);
    normalize_sections(sections, context, evidence, &mut diagnostics);
    let mut group_matches = evidence.groups.matching_plan(blocks, sections);
    let mut prepared = PreparedDefinitions {
        preferred_counts: HashMap::new(),
        plans: Vec::new(),
        diagnostics,
    };
    prepared.blocks(blocks, context, evidence, &mut group_matches);
    prepared.sections(sections, context, evidence, &mut group_matches);
    prepared
}

impl PreparedDefinitions {
    fn sections(
        &mut self,
        sections: &mut [Section],
        parent: DefinitionContext,
        evidence: &NativeHeadEvidence,
        group_matches: &mut GroupMatchingPlan,
    ) {
        for section in sections {
            let context = DefinitionContext::for_section(&section.heading.plain_text(), parent);
            self.blocks(&mut section.blocks, context, evidence, group_matches);
            self.sections(&mut section.children, context, evidence, group_matches);
        }
    }

    fn blocks(
        &mut self,
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
                        self.blocks(&mut item.blocks, child_context, evidence, group_matches);
                    }
                }
                Block::DefinitionList {
                    items,
                    declaration_groups,
                    ..
                } => {
                    let item_context = definition_group_context(items, context);
                    // Resolve native declaration witnesses while their
                    // parse-local owner markers still exist. Remove those
                    // markers before calculating public content-slice paths:
                    // anchors at the front of a term would otherwise shift
                    // every retained name-binding index.
                    let identities = items
                        .iter()
                        .map(|item| {
                            (
                                identity_plan(
                                    item,
                                    item_context,
                                    evidence.role(item),
                                    evidence.operands(item),
                                ),
                                evidence.shared_head(item),
                            )
                        })
                        .collect::<Vec<_>>();
                    let heads = identities
                        .iter()
                        .map(|(identity, _)| identity.group_head)
                        .collect::<Vec<_>>();
                    *declaration_groups = evidence.groups.resolve(items, &heads, group_matches);
                    crate::definitions::remove_native_definition_owner_markers_from_items(items);
                    for (item, (identity, head)) in items.iter_mut().zip(identities) {
                        record_limit(&mut self.diagnostics, identity.limit, item.source);
                        if has_semantic_spelling(item, &identity) {
                            *self
                                .preferred_counts
                                .entry(identity.preferred.clone())
                                .or_default() += 1;
                        }
                        let child_context = child_definition_context(identity.kind, item_context);
                        self.plans.push(PreparedDefinition {
                            source: item.source,
                            head,
                            identity,
                        });
                        self.blocks(
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
                            self.blocks(&mut cell.blocks, context, evidence, group_matches);
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

pub(super) fn record_limit(
    diagnostics: &mut Vec<mant_ir::Diagnostic>,
    limit: Option<super::syntax::DeclarationLimit>,
    source: Option<SourceSpan>,
) {
    if limit == Some(super::syntax::DeclarationLimit::Names) {
        diagnostics.push(mant_ir::Diagnostic {
            impact: mant_ir::DiagnosticImpact::SemanticCoverage,
            level: mant_ir::DiagnosticLevel::Warning,
            code: Some("manual.semantic-entry.name-limit".to_owned()),
            message: "explicit option declaration exceeded the 256-name recognition limit; complete readable forms are retained".to_owned(),
            source,
        });
    }
}

fn normalize_sections(
    sections: &mut [Section],
    parent: DefinitionContext,
    evidence: &NativeHeadEvidence,
    diagnostics: &mut Vec<mant_ir::Diagnostic>,
) {
    for section in sections {
        let context = DefinitionContext::for_section(&section.heading.plain_text(), parent);
        normalize_blocks(&mut section.blocks, context, evidence, diagnostics);
        normalize_sections(&mut section.children, context, evidence, diagnostics);
    }
}

fn normalize_blocks(
    blocks: &mut Vec<Block>,
    context: DefinitionContext,
    evidence: &NativeHeadEvidence,
    diagnostics: &mut Vec<mant_ir::Diagnostic>,
) {
    super::normalize::normalize_native_hanging_owners(blocks, context, &evidence.hanging);
    normalize_definition_nesting_with_boundaries(blocks, &evidence.continuations);
    normalize_hanging_definitions_with_evidence(blocks, context, evidence, diagnostics);
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    let child_context = item.entry.as_ref().map_or(context, |facts| {
                        child_definition_context(facts.kind, context)
                    });
                    if has_owned_first_block(item) {
                        // An explicit Block0 form already owns this original
                        // head. Do not run layout inference over it again;
                        // descriptions and nested owners still normalize.
                        let mut tail = item.blocks.split_off(1);
                        normalize_blocks(&mut tail, child_context, evidence, diagnostics);
                        item.blocks.append(&mut tail);
                    } else {
                        normalize_blocks(&mut item.blocks, child_context, evidence, diagnostics);
                    }
                }
            }
            Block::DefinitionList { items, .. } => {
                let item_context = definition_group_context(items, context);
                for item in items {
                    let identity = identity_plan(
                        item,
                        item_context,
                        evidence.role(item),
                        evidence.operands(item),
                    );
                    let child_context = child_definition_context(identity.kind, item_context);
                    normalize_blocks(&mut item.description, child_context, evidence, diagnostics);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        normalize_blocks(&mut cell.blocks, context, evidence, diagnostics);
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

pub(super) fn has_owned_first_block(item: &mant_ir::ListItem) -> bool {
    item.entry.as_ref().is_some_and(|entry| {
        entry.forms.iter().any(|form| {
            matches!(form.parts.as_slice(), [part]
                if part.root == (mant_ir::EntryInlineRoot::Block { index: 0 })
                    && part.path.is_empty() && part.bytes.is_none())
        })
    }) && matches!(
        item.blocks.first(),
        Some(Block::Paragraph { .. } | Block::Preformatted { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_freezes_names_before_ids_and_rejects_changed_heads() {
        let mut item = DefinitionItem {
            source: None,
            entry: None,
            head_body_relation: mant_ir::HeadBodyRelation::Separate,
            terms: (vec![vec![Inline::Text {
                value: "--mode=fast".into(),
            }]])
            .into_iter()
            .map(Into::into)
            .collect(),
            description: Vec::new(),
            layout: mant_ir::DefinitionLayout {
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
        item.terms[0]
            .content
            .insert(0, Inline::anchor("allocated-later"));
        assert!(plan.matches(&item));
        item.terms[0].content.push(Inline::Text {
            value: " changed".into(),
        });
        assert!(!plan.matches(&item));
    }
}
