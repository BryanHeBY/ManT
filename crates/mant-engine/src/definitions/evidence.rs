//! Operation-local native head evidence, never serialized as a second IR.
use std::collections::HashMap;

use mant_ir::{DefinitionItem, Inline, SourceSpan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeHeadRole {
    Option,
    Environment,
    Literal,
}

struct HeadWitness {
    source: SourceSpan,
    terms: Vec<Vec<Inline>>,
    role: NativeHeadRole,
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
    pub(crate) fn record(&mut self, item: &DefinitionItem, role: NativeHeadRole) {
        let Some(source) = item.source else { return };
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
        let candidates = self.witnesses.get(&(source.line, source.column))?;
        let terms = head_content(&item.terms);
        let mut matches = candidates
            .iter()
            .filter(|witness| witness.source == source && witness.terms == terms);
        let role = matches.next()?.role;
        matches.all(|witness| witness.role == role).then_some(role)
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
}
