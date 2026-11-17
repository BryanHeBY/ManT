//! Operation-local native head evidence, never serialized as a second IR.
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use mant_ir::{DefinitionItem, Inline, SourceSpan};

#[cfg(feature = "roff")]
mod publication;
#[cfg(feature = "roff")]
pub(crate) use publication::InlinePublication;

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
    #[cfg(any(feature = "roff", test))]
    Argument,
    /// An `Ar` spelling may name an authored operand definition directly.
    #[cfg(feature = "roff")]
    Placeholder,
    /// The native Ar belongs to a source-proven search form even if native
    /// execution subsequently overwrites the opening sign.
    #[cfg(feature = "roff")]
    SearchArgument(u32),
    #[cfg(feature = "roff")]
    SearchOpen {
        sign: char,
        argument: u32,
        closed: bool,
    },
    #[cfg(feature = "roff")]
    SearchClose,
    /// The first actual man OP operand still has to spell a complete flag.
    #[cfg(feature = "roff")]
    OptionalKey,
    #[cfg(feature = "roff")]
    Subject(NativeSubject, u32),
    /// Publication retained the native subject's role within the form,
    /// while clipping the evidence for its own exact name.
    #[cfg(feature = "roff")]
    ClippedSubject(NativeSubject, u32),
    #[cfg(feature = "roff")]
    Enclosure,
    ExplicitOption,
}

#[cfg(feature = "roff")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeSubject {
    Constant,
    Error,
    Variable,
    Type,
    ReturnType,
    Field,
    Function,
    FunctionBlock,
}

impl NativeOperandRole {
    pub(super) fn is_parameter(self) -> bool {
        match self {
            #[cfg(any(feature = "roff", test))]
            Self::Argument => true,
            #[cfg(feature = "roff")]
            Self::Placeholder
            | Self::SearchArgument(_)
            | Self::OptionalKey
            | Self::Subject(..)
            | Self::ClippedSubject(..) => true,
            _ => false,
        }
    }

    #[cfg(feature = "roff")]
    pub(super) fn subject(self) -> Option<(NativeSubject, u32)> {
        match self {
            Self::Subject(subject, scope) | Self::ClippedSubject(subject, scope) => {
                Some((subject, scope))
            }
            _ => None,
        }
    }
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

#[cfg(feature = "roff")]
impl CapturedHeadOperands {
    pub(crate) fn qualify_optional_keys(&mut self) {
        for operand in &mut self.operands {
            if operand.role != NativeOperandRole::OptionalKey {
                continue;
            }
            let key = self.text.get(operand.bytes.clone()).map(str::trim);
            operand.role = if key.is_some_and(|key| super::option_prefix(key) == Some(key)) {
                NativeOperandRole::ExplicitOption
            } else {
                NativeOperandRole::Argument
            };
        }
    }
}

struct OperandWitness {
    source: SourceSpan,
    terms: HeadSnapshot,
    operands: Vec<TermOperands>,
    #[cfg(feature = "roff")]
    composed: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) enum OperandEvidence<T> {
    #[default]
    Absent,
    Accepted(T),
    Unavailable,
}

pub(crate) type TermOperands = OperandEvidence<Rc<Vec<NativeOperand>>>;
pub(super) type HeadOperands<'a> = OperandEvidence<&'a [TermOperands]>;

impl<'a> HeadOperands<'a> {
    /// A missing receipt permits ordinary lexical recognition. A rejected
    /// receipt must not resurrect an argument as a font-derived name.
    pub(super) fn for_term(self, index: usize) -> Option<&'a [NativeOperand]> {
        match self {
            Self::Absent => Some(&[]),
            Self::Accepted(terms) => match terms.get(index)? {
                TermOperands::Absent => Some(&[]),
                TermOperands::Accepted(operands) => Some(operands),
                TermOperands::Unavailable => None,
            },
            Self::Unavailable => None,
        }
    }

    pub(super) fn proves_option(self) -> bool {
        let Self::Accepted(terms) = self else {
            return false;
        };
        (0..terms.len()).any(|index| {
            self.for_term(index).is_some_and(|operands| {
                operands
                    .iter()
                    .any(|operand| operand.role == NativeOperandRole::ExplicitOption)
            })
        })
    }
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
    operand_witnesses: HashMap<usize, Vec<OperandWitness>>,
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
    pub(crate) fn record_operands(
        &mut self,
        item: &DefinitionItem,
        key: usize,
        retired_break: bool,
    ) {
        let Some(mut captured) = self.pending_operands.remove(&key) else {
            return;
        };
        let Some(source) = item.source else { return };
        let Some(owner) = native_owner(item) else {
            return;
        };
        // Geometry may split/trim accepted HEAD output. A complete original
        // receipt, not a matching substring, is necessary to reuse operands.
        let operands = if let [term] = item.terms.as_slice() {
            let actual = mant_ir::inline_plain_text(term);
            if retired_break && captured.text.strip_suffix('\n') == Some(&actual) {
                // Definition layout moved one actual HEAD endline into
                // the head/body boundary. Only that known trailing cell
                // can disappear without invalidating the original proof.
                captured.operands.retain_mut(|operand| {
                    operand.bytes.end = operand.bytes.end.min(actual.len());
                    operand.bytes.start < operand.bytes.end
                });
                captured.text.truncate(actual.len());
            }
            if actual == captured.text {
                vec![TermOperands::Accepted(Rc::new(captured.operands))]
            } else {
                vec![TermOperands::Unavailable]
            }
        } else {
            vec![TermOperands::Unavailable; item.terms.len()]
        };
        let terms = self.shared_head(item);
        self.operand_witnesses.insert(
            owner,
            vec![OperandWitness {
                source,
                terms,
                operands,
                composed: false,
            }],
        );
    }

    fn matching_operand_witness(
        &self,
        item: &DefinitionItem,
        owner: usize,
    ) -> Option<(usize, &OperandWitness)> {
        let source = item.source?;
        self.operand_witnesses
            .get(&owner)?
            .iter()
            .enumerate()
            .rev()
            .find(|(_, witness)| {
                witness.source == source && publication_matches(&item.terms, &witness.terms)
            })
    }

    pub(super) fn operands(&self, item: &DefinitionItem) -> HeadOperands<'_> {
        let Some(owner) = native_owner(item) else {
            return HeadOperands::Absent;
        };
        if let Some((_, witness)) = self.matching_operand_witness(item, owner) {
            HeadOperands::Accepted(&witness.operands)
        } else if self.operand_witnesses.contains_key(&owner) {
            HeadOperands::Unavailable
        } else {
            HeadOperands::Absent
        }
    }

    #[cfg(feature = "roff")]
    pub(super) fn is_search_template(&self, item: &DefinitionItem, owner: usize) -> bool {
        let [term] = item.terms.as_slice() else {
            return false;
        };
        let Some(witness) = self
            .operand_witnesses
            .get(&owner)
            .and_then(|all| all.last())
        else {
            return false;
        };
        let [TermOperands::Accepted(operands)] = witness.operands.as_slice() else {
            return false;
        };
        let [opening, first_argument, suffix @ ..] = operands.as_slice() else {
            return false;
        };
        let NativeOperandRole::SearchOpen {
            sign,
            argument,
            closed,
        } = opening.role
        else {
            return false;
        };
        if opening.bytes != (0..sign.len_utf8())
            || first_argument.role != NativeOperandRole::SearchArgument(argument)
            || first_argument.bytes.start != opening.bytes.end
            || item.source != Some(witness.source)
            || !publication_matches(&item.terms, &witness.terms)
        {
            return false;
        }
        // The automatic separator may be part of the argument's receipt.
        // Inspect the accepted bytes rather than assuming its start is a glyph.
        let text = mant_ir::inline_plain_text(term);
        text.starts_with(sign)
            && !text.contains('\n')
            && text
                .get(first_argument.bytes.clone())
                .and_then(|word| word.chars().next())
                .is_some_and(|first| !first.is_whitespace())
            && (!closed
                || suffix.iter().any(|operand| {
                    operand.role == NativeOperandRole::SearchClose
                        && text.get(operand.bytes.clone()).is_some_and(|word| {
                            let mut glyphs = word.trim().chars();
                            glyphs.next() == Some(sign) && glyphs.next().is_none()
                        })
                }))
    }

    #[cfg(not(feature = "roff"))]
    pub(super) fn is_search_template(&self, _item: &DefinitionItem, _owner: usize) -> bool {
        false
    }

    /// Snapshot each input independently before TQ moves its terms. Every
    /// term stays in source order, including those with no native receipt.
    #[cfg(feature = "roff")]
    pub(crate) fn combine_operands<'a>(
        &self,
        inputs: impl Iterator<Item = &'a DefinitionItem>,
    ) -> Vec<TermOperands> {
        let mut combined = Vec::new();
        for input in inputs {
            match self.operands(input) {
                HeadOperands::Absent => {
                    combined.resize(combined.len() + input.terms.len(), TermOperands::Absent);
                }
                HeadOperands::Accepted(terms) => combined.extend_from_slice(terms),
                HeadOperands::Unavailable => {
                    combined.resize(
                        combined.len() + input.terms.len(),
                        TermOperands::Unavailable,
                    );
                }
            }
        }
        combined
    }

    /// Issue a new proof only for the structure-approved TQ continuation.
    /// Preserve the original proof, and share the composed styled snapshot
    /// with group evidence instead of keeping another history of merged heads.
    #[cfg(feature = "roff")]
    pub(crate) fn continue_definition(
        &mut self,
        item: &DefinitionItem,
        last_key: usize,
        operands: Vec<TermOperands>,
    ) {
        self.groups.continued(item, last_key);
        if operands
            .iter()
            .all(|term| matches!(term, TermOperands::Absent))
        {
            return;
        }
        debug_assert_eq!(item.terms.len(), operands.len());
        let (Some(owner), Some(source)) = (native_owner(item), item.source) else {
            return;
        };
        let terms = self.shared_head(item);
        let witnesses = self.operand_witnesses.entry(owner).or_default();
        witnesses.retain(|witness| !witness.composed);
        witnesses.push(OperandWitness {
            source,
            terms,
            operands,
            composed: true,
        });
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
