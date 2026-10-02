//! Operation-local native head evidence, never serialized as a second IR.
use std::collections::HashMap;
use std::ops::Range;

use mant_ir::{DefinitionItem, Inline, SourceSpan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeHeadRole {
    Option,
    Environment,
    Literal,
    /// An explicitly styled, otherwise ambiguous man IP operator/key tag.
    LiteralTerm,
    /// An explicitly styled single dash in a TP/TQ head is a shell operand.
    #[cfg_attr(not(feature = "roff"), allow(dead_code))]
    Operand,
    /// An unstyled man IP mark supplies layout, not declaration evidence.
    Presentation,
}

struct HeadWitness {
    source: SourceSpan,
    terms: Vec<Vec<Inline>>,
    role: NativeHeadRole,
}

/// Actual `pre_alternate` operands and generated `Fl` words retain their
/// declaration roles through font escapes and accepted HEAD output only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeOperandRole {
    Literal,
    Argument,
    ExplicitOption,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeOperand {
    pub(crate) bytes: Range<usize>,
    pub(crate) role: NativeOperandRole,
}

#[derive(Default)]
#[cfg(feature = "roff")]
pub(crate) struct CapturedHeadOperands {
    pub(crate) text: String,
    pub(crate) operands: Vec<NativeOperand>,
}

struct OperandWitness {
    source: SourceSpan,
    terms: Vec<Vec<Inline>>,
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
        self.operand_witnesses.insert(
            owner,
            OperandWitness {
                source,
                terms: head_content(&item.terms),
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
        if let Some(owner) = native_owner(item) {
            self.native_witnesses.insert(
                owner,
                HeadWitness {
                    source,
                    terms: head_content(&item.terms),
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
                terms: head_content(&item.terms),
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
        let terms = head_content(&item.terms);
        let mut matches = candidates
            .iter()
            .filter(|witness| witness.source == source && witness.terms == terms);
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
pub(super) fn head_matches(actual: &[Vec<Inline>], expected: &[Vec<Inline>]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, e)| inline_matches(a, e))
}

fn inline_matches(actual: &[Inline], expected: &[Inline]) -> bool {
    let mut actual = actual
        .iter()
        .filter(|node| !matches!(node, Inline::Anchor { .. }));
    let mut expected = expected
        .iter()
        .filter(|node| !matches!(node, Inline::Anchor { .. }));
    loop {
        match (actual.next(), expected.next()) {
            (None, None) => return true,
            (Some(Inline::Strong { children: a }), Some(Inline::Strong { children: e }))
            | (Some(Inline::Emphasis { children: a }), Some(Inline::Emphasis { children: e })) => {
                if !inline_matches(a, e) {
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
                if at != et || ai != ei || !inline_matches(a, e) {
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
pub(super) fn head_content(terms: &[Vec<Inline>]) -> Vec<Vec<Inline>> {
    fn without_anchors(inlines: &[Inline]) -> Vec<Inline> {
        inlines
            .iter()
            .filter_map(|inline| {
                Some(match inline {
                    Inline::Anchor { .. } => return None,
                    Inline::Strong { children } => Inline::Strong {
                        children: without_anchors(children),
                    },
                    Inline::Emphasis { children } => Inline::Emphasis {
                        children: without_anchors(children),
                    },
                    Inline::Link {
                        children,
                        target,
                        title,
                    } => Inline::Link {
                        children: without_anchors(children),
                        target: target.clone(),
                        title: title.clone(),
                    },
                    _ => inline.clone(),
                })
            })
            .collect()
    }
    terms.iter().map(|term| without_anchors(term)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> DefinitionItem {
        DefinitionItem {
            source: Some(SourceSpan {
                line: 12,
                column: 4,
                byte_range: None,
                end_line: None,
                end_column: None,
            }),
            entry: None,
            terms: vec![vec![Inline::Strong {
                children: vec![Inline::Text {
                    value: "PATH".into(),
                }],
            }]],
            description: Vec::new(),
            layout: mant_ir::DefinitionLayout {
                spacing_before_lines: None,
                ..Default::default()
            },
        }
    }

    #[test]
    fn linked_head_navigation_does_not_invalidate_native_evidence() {
        let mut original = item();
        original.terms[0] = vec![Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.invalid".into(),
            },
            title: None,
            children: original.terms[0].clone(),
        }];
        let mut evidence = NativeHeadEvidence::default();
        evidence.record(&original, NativeHeadRole::Environment);
        let mut moved = original.clone();
        let Inline::Link { children, .. } = &mut moved.terms[0][0] else {
            panic!("linked head");
        };
        children.insert(0, Inline::anchor("allocated-navigation-id"));
        assert_eq!(evidence.role(&moved), Some(NativeHeadRole::Environment));
        moved.terms[0].push(Inline::Text {
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
        moved.terms[0].insert(0, Inline::anchor("new-navigation-id"));
        assert_eq!(evidence.role(&moved), Some(NativeHeadRole::Environment));
        moved.source.as_mut().unwrap().column += 1;
        assert_eq!(evidence.role(&moved), None);
        moved.source = original.source;
        moved.terms[0] = vec![Inline::Emphasis {
            children: vec![Inline::Text {
                value: "PATH".into(),
            }],
        }];
        assert_eq!(evidence.role(&moved), None);
        moved.terms = original.terms.clone();
        moved.terms.push(vec![Inline::Text {
            value: "OTHER".into(),
        }]);
        assert_eq!(evidence.role(&moved), None);
        evidence.record(&original, NativeHeadRole::Literal);
        assert_eq!(evidence.role(&original), None);
    }

    #[test]
    fn actual_owner_identity_separates_same_coordinate_macro_expansions() {
        let mut evidence = NativeHeadEvidence::default();
        for key in 1..=256 {
            let mut native = item();
            native.terms[0].insert(
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
            moved.terms[0].insert(
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
}
