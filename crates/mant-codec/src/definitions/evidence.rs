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
mod tests;
