//! Operation-local native head evidence, never serialized as a second IR.
use std::collections::HashMap;
use std::ops::Range;

use mant_ir::{DefinitionItem, Inline, SourceSpan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeHeadRole {
    Option,
    Environment,
    /// Authored mdoc Va instance, regardless of its executed font.
    Variable,
    /// Authored mdoc Dv instance: a named symbolic Term, not an exported Va.
    #[cfg_attr(not(feature = "roff"), allow(dead_code))]
    DefinedVariable,
    Literal,
    /// An explicitly styled, otherwise ambiguous man IP operator/key tag.
    LiteralTerm,
    /// An unstyled man IP mark supplies layout, not declaration evidence.
    Presentation,
}

/// One non-option native macro instance bound to its surviving visible HEAD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeHeadComponent {
    pub(crate) role: NativeHeadRole,
    pub(crate) range: Range<usize>,
}

struct HeadWitness {
    source: SourceSpan,
    terms: Vec<Vec<Inline>>,
    role: Option<NativeHeadRole>,
    complete_term: bool,
    option_ranges: Vec<Vec<Range<usize>>>,
    operand_ranges: Vec<Vec<Range<usize>>>,
    argument_ranges: Vec<Vec<Range<usize>>>,
    components: Vec<Vec<NativeHeadComponent>>,
}

/// Locations only select a bucket. Evidence is reusable only when the entire
/// styled head and complete source coordinate still match. Moving an owner or
/// nesting its body preserves the witness; changing/splitting a head invalidates
/// it. Conflicting same-location witnesses deliberately supply no role.
#[derive(Default)]
pub(crate) struct NativeHeadEvidence {
    pub(crate) groups: super::groups::GroupEvidence,
    /// Explicit headless IP continuations already assigned to their source
    /// owner. Later indentation recovery cannot move them into its last child.
    pub(crate) continuations: std::collections::HashSet<(u32, u32)>,
    witnesses: HashMap<(u32, u32), Vec<HeadWitness>>,
}

impl NativeHeadEvidence {
    #[cfg(test)]
    pub(crate) fn record(&mut self, item: &DefinitionItem, role: NativeHeadRole) {
        self.record_with_term_witness(
            item,
            Some(role),
            false,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }

    #[cfg(any(feature = "roff", test))]
    #[expect(
        clippy::too_many_arguments,
        reason = "one conversion-local HEAD witness is recorded atomically"
    )]
    pub(crate) fn record_with_term_witness(
        &mut self,
        item: &DefinitionItem,
        role: Option<NativeHeadRole>,
        complete_term: bool,
        option_ranges: Vec<Vec<Range<usize>>>,
        operand_ranges: Vec<Vec<Range<usize>>>,
        argument_ranges: Vec<Vec<Range<usize>>>,
        components: Vec<Vec<NativeHeadComponent>>,
    ) {
        let Some(source) = item.source else { return };
        self.witnesses
            .entry((source.line, source.column))
            .or_default()
            .push(HeadWitness {
                source,
                terms: head_content(&item.terms),
                role,
                complete_term,
                option_ranges,
                operand_ranges,
                argument_ranges,
                components,
            });
    }

    pub(super) fn role(&self, item: &DefinitionItem) -> Option<NativeHeadRole> {
        self.witness(item)?.role
    }

    pub(super) fn complete_term(&self, item: &DefinitionItem) -> bool {
        self.witness(item)
            .is_some_and(|witness| witness.complete_term)
    }

    pub(super) fn option_ranges(&self, item: &DefinitionItem) -> Option<&[Vec<Range<usize>>]> {
        let witness = self.witness(item)?;
        (witness.role == Some(NativeHeadRole::Option)).then_some(witness.option_ranges.as_slice())
    }

    pub(super) fn operand_ranges(&self, item: &DefinitionItem) -> Option<&[Vec<Range<usize>>]> {
        let witness = self.witness(item)?;
        witness
            .operand_ranges
            .iter()
            .any(|term| !term.is_empty())
            .then_some(witness.operand_ranges.as_slice())
    }

    pub(super) fn argument_ranges(&self, item: &DefinitionItem) -> Option<&[Vec<Range<usize>>]> {
        let witness = self.witness(item)?;
        witness
            .argument_ranges
            .iter()
            .any(|term| !term.is_empty())
            .then_some(witness.argument_ranges.as_slice())
    }

    pub(super) fn components(&self, item: &DefinitionItem) -> Option<&[Vec<NativeHeadComponent>]> {
        let witness = self.witness(item)?;
        witness
            .components
            .iter()
            .any(|term| !term.is_empty())
            .then_some(witness.components.as_slice())
    }

    fn witness(&self, item: &DefinitionItem) -> Option<&HeadWitness> {
        let source = item.source?;
        let candidates = self.witnesses.get(&(source.line, source.column))?;
        let terms = head_content(&item.terms);
        let mut matches = candidates
            .iter()
            .filter(|witness| witness.source == source && witness.terms == terms);
        let first = matches.next()?;
        matches
            .all(|witness| {
                witness.role == first.role
                    && witness.complete_term == first.complete_term
                    && witness.option_ranges == first.option_ranges
                    && witness.operand_ranges == first.operand_ranges
                    && witness.argument_ranges == first.argument_ranges
                    && witness.components == first.components
            })
            .then_some(first)
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
                        occurrence,
                    } => Inline::Link {
                        children: without_anchors(children),
                        occurrence: *occurrence,
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
    use crate::test_content as fixture;

    fn item() -> DefinitionItem {
        DefinitionItem {
            source: Some(SourceSpan {
                source: mant_ir::SourceKey::FIRST,
                line: 12,
                column: 4,
                byte_range: None,
                end_line: None,
                end_column: None,
            }),
            entry: None,
            terms: vec![vec![Inline::Strong {
                children: vec![fixture::text("PATH")],
            }]],
            description: Vec::new(),
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
        }
    }

    #[test]
    fn witnesses_require_full_source_and_styled_content_not_a_line_or_address() {
        let original = item();
        let mut evidence = NativeHeadEvidence::default();
        evidence.record(&original, NativeHeadRole::Environment);
        let mut moved = Box::new(original.clone());
        moved.terms[0].insert(0, fixture::anchor("new-navigation-id"));
        assert_eq!(evidence.role(&moved), Some(NativeHeadRole::Environment));
        moved.source.as_mut().unwrap().column += 1;
        assert_eq!(evidence.role(&moved), None);
        moved.source = original.source;
        moved.terms[0] = vec![Inline::Emphasis {
            children: vec![fixture::text("PATH")],
        }];
        assert_eq!(evidence.role(&moved), None);
        moved.terms = original.terms.clone();
        moved.terms.push(vec![fixture::text("OTHER")]);
        assert_eq!(evidence.role(&moved), None);
        evidence.record(&original, NativeHeadRole::Literal);
        assert_eq!(evidence.role(&original), None);
    }

    #[test]
    fn complete_term_witness_is_bound_to_the_native_head_not_plain_text() {
        let original = item();
        let mut evidence = NativeHeadEvidence::default();
        evidence.record_with_term_witness(
            &original,
            None,
            true,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert!(evidence.complete_term(&original));
        let mut edited = original;
        edited.terms[0] = vec![fixture::text("PATH")];
        assert!(!evidence.complete_term(&edited));
    }
}
