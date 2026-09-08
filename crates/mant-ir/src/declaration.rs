//! Reading context is distinct from ownership and name equivalence.
use crate::{
    Block, DefinitionItem, Inline,
    visit::{self, Visit},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A recovered run of consecutive declarations in one definition list.
///
/// Earlier members have no readable description; the final member supplies
/// the group's context. This never asserts that the members are aliases or
/// that every sentence applies to every member. The half-open item coordinates
/// belong to the exact containing list, not a durable or cross-document ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclarationGroup {
    /// First declaration, inclusive.
    pub start_item: usize,
    /// End of declarations, exclusive; the preceding item supplies context.
    pub end_item: usize,
}

impl DeclarationGroup {
    /// Resolve a structurally valid group without trusting producer indices.
    #[must_use]
    pub fn resolve(self, items: &[DefinitionItem]) -> Option<&[DefinitionItem]> {
        if self.end_item.checked_sub(self.start_item)? < 2 {
            return None;
        }
        let members = items.get(self.start_item..self.end_item)?;
        let (tail, heads) = members.split_last()?;
        (members
            .iter()
            .all(|item| item.terms.iter().flatten().any(readable_inline))
            && blocks_have_readable_content(&tail.description)
            && heads
                .iter()
                .all(|head| !blocks_have_readable_content(&head.description)))
        .then_some(members)
    }

    /// Retain and rebase a complete group inside a sliced list. Partial groups
    /// are dropped rather than borrowing content outside the excerpt.
    #[must_use]
    pub fn within(self, start: usize, end: usize) -> Option<Self> {
        (start <= self.start_item && self.start_item < self.end_item && self.end_item <= end).then(
            || Self {
                start_item: self.start_item - start,
                end_item: self.end_item - start,
            },
        )
    }
}

fn readable_inline(inline: &Inline) -> bool {
    match inline {
        Inline::Text { value } | Inline::Code { value } => !value.trim().is_empty(),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => children.iter().any(readable_inline),
        Inline::LineBreak | Inline::Anchor { .. } => false,
    }
}

/// Whether blocks contain readable content rather than just spacing/anchors.
/// Used by producer grouping and shared structural validation alike.
#[must_use]
pub fn blocks_have_readable_content(blocks: &[Block]) -> bool {
    struct Readable(bool);
    impl<'a> Visit<'a> for Readable {
        fn visit_block(&mut self, block: &'a Block) {
            if self.0 {
                return;
            }
            match block {
                Block::Equation { value, .. } => self.0 |= !value.trim().is_empty(),
                Block::Unsupported { text, .. } => self.0 |= !text.trim().is_empty(),
                _ => visit::walk_block(self, block),
            }
        }
        fn visit_inline(&mut self, inline: &'a Inline) {
            if self.0 {
                return;
            }
            match inline {
                Inline::Text { value } | Inline::Code { value } => {
                    self.0 |= !value.trim().is_empty();
                }
                _ => visit::walk_inline(self, inline),
            }
        }
    }
    let mut readable = Readable(false);
    for block in blocks {
        readable.visit_block(block);
    }
    readable.0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(text: &str) -> DefinitionItem {
        DefinitionItem {
            source: None,
            entry: None,
            terms: vec![vec![Inline::Text {
                value: "--name".into(),
            }]],
            description: vec![Block::Paragraph {
                children: vec![Inline::Text { value: text.into() }],
                layout: crate::LayoutHint::default(),
                source: None,
            }],
            layout: crate::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
            },
        }
    }
    #[test]
    fn ranges_are_checked_and_partial_excerpts_cannot_borrow_context() {
        let items = vec![item(""), item(""), item("body"), item("next")];
        let group = DeclarationGroup {
            start_item: 0,
            end_item: 3,
        };
        assert_eq!(group.resolve(&items).unwrap().len(), 3);
        for bad in [
            DeclarationGroup {
                start_item: 1,
                end_item: 1,
            },
            DeclarationGroup {
                start_item: 0,
                end_item: usize::MAX,
            },
            DeclarationGroup {
                start_item: 3,
                end_item: 2,
            },
            DeclarationGroup {
                start_item: 0,
                end_item: 2,
            },
            DeclarationGroup {
                start_item: 2,
                end_item: 4,
            },
        ] {
            assert!(bad.resolve(&items).is_none(), "{bad:?}");
        }
        assert_eq!(group.within(0, 3), Some(group));
        assert_eq!(group.within(1, 3), None);
        assert_eq!(group.within(0, 2), None);
        assert_eq!(
            DeclarationGroup {
                start_item: 2,
                end_item: 4
            }
            .within(2, 5),
            Some(DeclarationGroup {
                start_item: 0,
                end_item: 2
            })
        );
    }
    #[test]
    fn anchors_and_spacing_do_not_count_as_readable_descriptions() {
        let blocks = vec![
            Block::VerticalSpace {
                lines: 3,
                source: None,
            },
            Block::Paragraph {
                children: vec![Inline::anchor("target")],
                layout: crate::LayoutHint::default(),
                source: None,
            },
        ];
        assert!(!blocks_have_readable_content(&blocks));
        assert!(blocks_have_readable_content(&[Block::Equation {
            value: "x + y".into(),
            display: true,
            layout: crate::LayoutHint::default(),
            source: None
        }]));
    }
    #[test]
    fn group_wire_is_closed_and_round_trip_keeps_original_items() {
        let block = Block::DefinitionList {
            items: vec![item(""), item("body")],
            declaration_groups: vec![DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }],
            compact: false,
            layout: crate::LayoutHint::default(),
            source: None,
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(
            serde_json::from_value::<Block>(json.clone()).unwrap(),
            block
        );
        let mut malformed = json;
        malformed["declarationGroups"][0]["endItme"] = 2.into();
        assert!(serde_json::from_value::<Block>(malformed).is_err());
    }
}
