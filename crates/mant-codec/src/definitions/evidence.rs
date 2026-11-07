//! Operation-local native head evidence, never serialized as a second IR.
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use mant_ir::{DefinitionItem, Inline, SourceSpan};

#[cfg(feature = "roff")]
mod publication;
#[cfg(feature = "roff")]
pub(super) use publication::retain_unchanged_operands;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeHeadRole {
    Option,
    Environment,
    Literal,
    /// An explicitly styled, otherwise ambiguous man IP operator/key tag.
    LiteralTerm,
    /// An explicitly styled single dash in a TP/TQ head is a shell operand.
    #[cfg(feature = "roff")]
    Operand,
    /// An unstyled man IP mark supplies layout, not declaration evidence.
    Presentation,
}

struct HeadWitness {
    source: SourceSpan,
    terms: HeadSnapshot,
    role: NativeHeadRole,
}

/// Actual `pre_alternate` operands and generated `Fl` words retain their
/// declaration roles through font escapes and accepted HEAD output only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeOperandRole {
    #[cfg(any(feature = "roff", test))]
    Literal,
    Argument,
    ExplicitOption,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeOperand {
    pub(crate) bytes: Range<usize>,
    pub(crate) role: NativeOperandRole,
}

#[derive(Clone, Default)]
#[cfg(feature = "roff")]
pub(crate) struct CapturedHeadOperands {
    pub(crate) text: String,
    pub(crate) operands: Vec<NativeOperand>,
}

struct OperandWitness {
    source: SourceSpan,
    terms: HeadSnapshot,
    operands: Vec<Vec<NativeOperand>>,
}

/// Actual native owners index witnesses directly. The complete source and
/// styled head must still match; moving an owner preserves its identity,
/// changing/splitting it invalidates the witness. Constructed role controls
/// without a native identity use a coordinate bucket and reject conflicts.
#[derive(Default)]
pub(crate) struct NativeHeadEvidence {
    pub(crate) groups: super::groups::GroupEvidence,
    pub(crate) hanging: super::hanging_owner::HangingOwnerEvidence,
    /// Explicit headless IP continuations already assigned to their source
    /// owner. Later indentation recovery cannot move them into its last child.
    pub(crate) continuations: std::collections::HashSet<(u32, u32)>,
    witnesses: HashMap<(u32, u32), Vec<HeadWitness>>,
    native_witnesses: HashMap<usize, HeadWitness>,
    operand_witnesses: HashMap<usize, OperandWitness>,
    #[cfg(feature = "roff")]
    pending_operands: HashMap<usize, CapturedHeadOperands>,
}

impl NativeHeadEvidence {
    /// Reuse a proven immutable snapshot, never update an older role or
    /// operand witness when a TQ continuation changes the accepted head.
    pub(super) fn shared_head(&self, item: &DefinitionItem) -> HeadSnapshot {
        self.groups
            .shared_head(item)
            .unwrap_or_else(|| head_snapshot(&item.terms))
    }

    #[cfg(feature = "roff")]
    pub(crate) fn capture_operands(&mut self, key: usize, captured: CapturedHeadOperands) {
        if !captured.operands.is_empty() {
            self.pending_operands.insert(key, captured);
        }
    }

    #[cfg(feature = "roff")]
    pub(crate) fn record_operands(&mut self, item: &DefinitionItem, key: usize) {
        let Some(captured) = self.pending_operands.remove(&key) else {
            return;
        };
        let Some(source) = item.source else { return };
        let Some(owner) = native_owner(item) else {
            return;
        };
        let [term] = item.terms.as_slice() else {
            return;
        };
        // Geometry may split/trim accepted HEAD output. A complete original
        // receipt, not a matching substring, is necessary to reuse operands.
        if mant_ir::inline_plain_text(term) != captured.text {
            return;
        }
        let terms = self.shared_head(item);
        self.operand_witnesses.insert(
            owner,
            OperandWitness {
                source,
                terms,
                operands: vec![captured.operands],
            },
        );
    }

    pub(super) fn operands(&self, item: &DefinitionItem) -> Option<&[Vec<NativeOperand>]> {
        let source = item.source?;
        let witness = self.operand_witnesses.get(&native_owner(item)?)?;
        (witness.source == source && head_matches(&item.terms, &witness.terms))
            .then_some(witness.operands.as_slice())
    }
    #[cfg(any(feature = "roff", test))]
    pub(crate) fn record(&mut self, item: &DefinitionItem, role: NativeHeadRole) {
        let Some(source) = item.source else { return };
        let terms = self.shared_head(item);
        if let Some(owner) = native_owner(item) {
            self.native_witnesses.insert(
                owner,
                HeadWitness {
                    source,
                    terms,
                    role,
                },
            );
            return;
        }
        self.witnesses
            .entry((source.line, source.column))
            .or_default()
            .push(HeadWitness {
                source,
                terms,
                role,
            });
    }

    pub(super) fn role(&self, item: &DefinitionItem) -> Option<NativeHeadRole> {
        let source = item.source?;
        if let Some(owner) = native_owner(item) {
            let witness = self.native_witnesses.get(&owner)?;
            return (witness.source == source && head_matches(&item.terms, &witness.terms))
                .then_some(witness.role);
        }
        let candidates = self.witnesses.get(&(source.line, source.column))?;
        let mut matches = candidates.iter().filter(|witness| {
            witness.source == source && head_matches(&item.terms, &witness.terms)
        });
        let role = matches.next()?.role;
        matches.all(|witness| witness.role == role).then_some(role)
    }
}

fn native_owner(item: &DefinitionItem) -> Option<usize> {
    item.terms
        .iter()
        .find_map(|term| super::groups::native_inline_owner(term))
}

/// Compare the entire accepted head without cloning it on each lookup.
/// Parse-private and allocated navigation anchors never change glyph identity.
pub(super) fn head_matches(actual: &[impl AsRef<[Inline]>], expected: &[Vec<Inline>]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, e)| inline_matches(a.as_ref(), e, false))
}

#[cfg(feature = "roff")]
pub(super) fn publication_matches(
    actual: &[impl AsRef<[Inline]>],
    expected: &[Vec<Inline>],
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, e)| inline_matches(a.as_ref(), e, true))
}

fn retained_in_proof(node: &Inline, retain_owner: bool) -> bool {
    match node {
        Inline::Anchor { id, .. } => {
            retain_owner && super::groups::is_internal_definition_owner_marker(id.as_str())
        }
        _ => true,
    }
}

pub(super) fn inline_matches(actual: &[Inline], expected: &[Inline], retain_owner: bool) -> bool {
    let mut actual = actual
        .iter()
        .filter(|node| retained_in_proof(node, retain_owner));
    let mut expected = expected
        .iter()
        .filter(|node| retained_in_proof(node, retain_owner));
    loop {
        match (actual.next(), expected.next()) {
            (None, None) => return true,
            (Some(Inline::Strong { children: a }), Some(Inline::Strong { children: e }))
            | (Some(Inline::Emphasis { children: a }), Some(Inline::Emphasis { children: e })) => {
                if !inline_matches(a, e, retain_owner) {
                    return false;
                }
            }
            (
                Some(Inline::Link {
                    children: a,
                    target: at,
                    title: ai,
                }),
                Some(Inline::Link {
                    children: e,
                    target: et,
                    title: ei,
                }),
            ) => {
                if at != et || ai != ei || !inline_matches(a, e, retain_owner) {
                    return false;
                }
            }
            (Some(a), Some(e)) if a == e => {}
            _ => return false,
        }
    }
}

/// Target allocation changes zero-width anchors, not declaration content.
/// Retain all other structure, including emphasis ancestry and link targets.
pub(super) type HeadSnapshot = Rc<[Vec<Inline>]>;

pub(super) fn head_snapshot(terms: &[impl AsRef<[Inline]>]) -> HeadSnapshot {
    copy_head(terms, true).into()
}

#[cfg(feature = "roff")]
pub(super) fn head_content(terms: &[impl AsRef<[Inline]>]) -> Vec<Vec<Inline>> {
    copy_head(terms, false)
}

fn copy_head(terms: &[impl AsRef<[Inline]>], retain_owner: bool) -> Vec<Vec<Inline>> {
    fn copy_inlines(inlines: &[Inline], retain_owner: bool) -> Vec<Inline> {
        inlines
            .iter()
            .filter(|inline| retained_in_proof(inline, retain_owner))
            .map(|inline| match inline {
                Inline::Strong { children } => Inline::Strong {
                    children: copy_inlines(children, retain_owner),
                },
                Inline::Emphasis { children } => Inline::Emphasis {
                    children: copy_inlines(children, retain_owner),
                },
                Inline::Link {
                    children,
                    target,
                    title,
                } => Inline::Link {
                    children: copy_inlines(children, retain_owner),
                    target: target.clone(),
                    title: title.clone(),
                },
                _ => inline.clone(),
            })
            .collect()
    }
    terms
        .iter()
        .map(|term| copy_inlines(term.as_ref(), retain_owner))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> DefinitionItem {
        DefinitionItem {
            head_body_relation: mant_ir::HeadBodyRelation::Separate,
            source: Some(SourceSpan {
                line: 12,
                column: 4,
                byte_range: None,
                end_line: None,
                end_column: None,
            }),
            entry: None,
            terms: (vec![vec![Inline::Strong {
                children: vec![Inline::Text {
                    value: "PATH".into(),
                }],
            }]])
            .into_iter()
            .map(Into::into)
            .collect(),
            description: Vec::new(),
            layout: mant_ir::DefinitionLayout {
                body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                spacing_before_lines: None,
                ..Default::default()
            },
        }
    }

    #[test]
    fn linked_head_navigation_does_not_invalidate_native_evidence() {
        let mut original = item();
        original.terms[0].content = vec![Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.invalid".into(),
            },
            title: None,
            children: original.terms[0].content.clone(),
        }];
        let mut evidence = NativeHeadEvidence::default();
        evidence.record(&original, NativeHeadRole::Environment);
        let mut moved = original.clone();
        let Inline::Link { children, .. } = &mut moved.terms[0].content[0] else {
            panic!("linked head");
        };
        children.insert(0, Inline::anchor("allocated-navigation-id"));
        assert_eq!(evidence.role(&moved), Some(NativeHeadRole::Environment));
        moved.terms[0].content.push(Inline::Text {
            value: "different native text".into(),
        });
        assert_eq!(evidence.role(&moved), None);
    }

    #[test]
    fn witnesses_require_full_source_and_styled_content_not_a_line_or_address() {
        let original = item();
        let mut evidence = NativeHeadEvidence::default();
        evidence.record(&original, NativeHeadRole::Environment);
        let mut moved = Box::new(original.clone());
        moved.terms[0]
            .content
            .insert(0, Inline::anchor("new-navigation-id"));
        assert_eq!(evidence.role(&moved), Some(NativeHeadRole::Environment));
        moved.source.as_mut().unwrap().column += 1;
        assert_eq!(evidence.role(&moved), None);
        moved.source = original.source;
        moved.terms[0].content = vec![Inline::Emphasis {
            children: vec![Inline::Text {
                value: "PATH".into(),
            }],
        }];
        assert_eq!(evidence.role(&moved), None);
        moved.terms = original.terms.clone();
        moved.terms.push(
            vec![Inline::Text {
                value: "OTHER".into(),
            }]
            .into(),
        );
        assert_eq!(evidence.role(&moved), None);
        evidence.record(&original, NativeHeadRole::Literal);
        assert_eq!(evidence.role(&original), None);
    }

    #[test]
    fn actual_owner_identity_separates_same_coordinate_macro_expansions() {
        let mut evidence = NativeHeadEvidence::default();
        for key in 1..=256 {
            let mut native = item();
            native.terms[0].content.insert(
                0,
                Inline::anchor(format!("\0mant-native-definition-owner:{key:x}")),
            );
            evidence.record(
                &native,
                if key % 2 == 0 {
                    NativeHeadRole::Environment
                } else {
                    NativeHeadRole::Literal
                },
            );
        }
        assert_eq!(evidence.native_witnesses.len(), 256);
        assert_eq!(evidence.witnesses.len(), 0);
        for key in 1..=256 {
            let mut moved = item();
            moved.terms[0].content.insert(
                0,
                Inline::anchor(format!("\0mant-native-definition-owner:{key:x}")),
            );
            assert_eq!(
                evidence.role(&moved),
                Some(if key % 2 == 0 {
                    NativeHeadRole::Environment
                } else {
                    NativeHeadRole::Literal
                })
            );
            moved.source.as_mut().unwrap().column += 1;
            assert_eq!(evidence.role(&moved), None);
        }
    }

    #[cfg(feature = "roff")]
    fn recorded_owner() -> (DefinitionItem, NativeHeadEvidence) {
        let mut original = item();
        super::super::groups::mark_native_definition_owner(&mut original, 10);
        let mut evidence = NativeHeadEvidence::default();
        evidence.groups.record(&original, 10);
        evidence.record(&original, NativeHeadRole::Environment);
        evidence.capture_operands(
            20,
            CapturedHeadOperands {
                text: "PATH".into(),
                operands: vec![NativeOperand {
                    bytes: 0..4,
                    role: NativeOperandRole::Literal,
                }],
            },
        );
        evidence.record_operands(&original, 20);
        (original, evidence)
    }

    #[test]
    #[cfg(feature = "roff")]
    fn shared_owner_proofs_still_reject_source_style_and_identity_changes() {
        // Constructed proof mutations test admission, not a new roff gold.
        // Sharing must not make a group snapshot authorize another owner.
        let (original, evidence) = recorded_owner();
        assert_eq!(evidence.role(&original), Some(NativeHeadRole::Environment));
        assert!(evidence.operands(&original).is_some());

        let mut changed = original.clone();
        changed.source.as_mut().unwrap().end_column = Some(9);
        assert_eq!(evidence.role(&changed), None);
        assert!(evidence.operands(&changed).is_none());

        changed = original.clone();
        changed.terms[0].content[1] = Inline::Emphasis {
            children: vec![Inline::Text {
                value: "PATH".into(),
            }],
        };
        assert_eq!(evidence.role(&changed), None);
        assert!(evidence.operands(&changed).is_none());

        changed = original;
        changed.terms[0].content[0] = Inline::anchor("\0mant-native-definition-owner:1e");
        assert_eq!(evidence.role(&changed), None);
        assert!(evidence.operands(&changed).is_none());
    }

    #[test]
    #[cfg(feature = "roff")]
    fn a_continued_group_cannot_rewrite_an_earlier_operand_or_role_proof() {
        // TQ creates a new merged-head witness. Earlier accepted operand
        // ranges keep their original complete HEAD/source proof immutable.
        let (original, mut evidence) = recorded_owner();
        let mut continued = original.clone();
        continued.terms.push(
            vec![Inline::Text {
                value: "OTHER".into(),
            }]
            .into(),
        );
        evidence.groups.continued(&continued, 30);
        assert!(evidence.groups.shared_head(&continued).is_some());
        assert_eq!(evidence.role(&continued), None);
        assert!(evidence.operands(&continued).is_none());
        assert_eq!(evidence.role(&original), Some(NativeHeadRole::Environment));
        assert!(evidence.operands(&original).is_some());
    }

    #[test]
    #[cfg(feature = "roff")]
    fn shared_head_proof_preserves_link_destination_and_title_identity() {
        let mut original = item();
        original.terms[0].content = vec![Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.invalid".into(),
            },
            title: Some("original".into()),
            children: original.terms[0].content.clone(),
        }];
        super::super::groups::mark_native_definition_owner(&mut original, 10);
        let mut evidence = NativeHeadEvidence::default();
        evidence.groups.record(&original, 10);
        evidence.record(&original, NativeHeadRole::Environment);
        let mut changed = original.clone();
        let Inline::Link { title, .. } = &mut changed.terms[0].content[1] else {
            unreachable!()
        };
        *title = Some("changed".into());
        assert_eq!(evidence.role(&changed), None);
        let Inline::Link { target, title, .. } = &mut changed.terms[0].content[1] else {
            unreachable!()
        };
        *title = Some("original".into());
        *target = mant_ir::LinkTarget::External {
            uri: "https://other.invalid".into(),
        };
        assert_eq!(evidence.role(&changed), None);
        assert_eq!(evidence.role(&original), Some(NativeHeadRole::Environment));
    }

    #[test]
    #[cfg(feature = "roff")]
    fn publication_rebinds_only_the_proved_owner_once() {
        use mant_ir::{Block, LinkTarget};
        let (mut original, mut evidence) = recorded_owner();
        let source = original.source;
        let link = Inline::Link {
            target: LinkTarget::External {
                uri: "private".into(),
            },
            title: None,
            children: vec![Inline::Text {
                value: " proof".into(),
            }],
        };
        original.terms[0].content.push(link);
        evidence.groups.record(&original, 10);
        evidence.record(&original, NativeHeadRole::Environment);
        evidence.capture_operands(
            20,
            CapturedHeadOperands {
                text: "PATH proof".into(),
                operands: vec![NativeOperand {
                    bytes: 0..4,
                    role: NativeOperandRole::Literal,
                }],
            },
        );
        evidence.record_operands(&original, 20);

        let mut invalid = original.clone();
        super::super::remove_native_definition_owner_markers_from_items(std::slice::from_mut(
            &mut invalid,
        ));
        super::super::groups::mark_native_definition_owner(&mut invalid, 30);
        evidence.groups.record(&invalid, 30);
        evidence.record(&invalid, NativeHeadRole::Environment);
        let Inline::Link { title, .. } = &mut invalid.terms[0].content[2] else {
            unreachable!()
        };
        *title = Some("changed before publication".into());

        let mut block = Block::DefinitionList {
            items: vec![original, invalid],
            declaration_groups: Vec::new(),
            compact: false,
            source,
            layout: mant_ir::LayoutHint::default(),
        };
        let mut calls = 0;
        evidence.publish_annotations(std::slice::from_mut(&mut block), &mut [], |nodes| {
            calls += 1;
            let Some(Inline::Link { target, title, .. }) = nodes.last_mut() else {
                return false;
            };
            *target = LinkTarget::External {
                uri: "https://example.test".into(),
            };
            *title = None;
            true
        });
        assert_eq!(calls, 2);
        let Block::DefinitionList { items, .. } = &block else {
            unreachable!()
        };
        assert_eq!(evidence.role(&items[0]), Some(NativeHeadRole::Environment));
        assert_eq!(evidence.operands(&items[0]).unwrap()[0][0].bytes, 0..4);
        assert!(evidence.groups.shared_head(&items[0]).is_some());
        assert_eq!(evidence.role(&items[1]), None);
        assert!(evidence.groups.shared_head(&items[1]).is_none());
    }

    #[test]
    #[cfg(feature = "roff")]
    fn publication_refuses_duplicated_owner_markers() {
        let (original, mut evidence) = recorded_owner();
        let mut block = mant_ir::Block::DefinitionList {
            items: vec![original.clone(), original],
            declaration_groups: Vec::new(),
            compact: false,
            source: None,
            layout: mant_ir::LayoutHint::default(),
        };
        evidence.publish_annotations(std::slice::from_mut(&mut block), &mut [], |_| false);
        let mant_ir::Block::DefinitionList { items, .. } = &block else {
            unreachable!()
        };
        for item in items {
            assert_eq!(evidence.role(item), None);
            assert!(evidence.operands(item).is_none());
            assert!(evidence.groups.shared_head(item).is_none());
        }
    }
}
