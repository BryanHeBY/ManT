//! Executed man HP/headless-IP pairs, retaining their original block geometry.
//!
//! Source positions select no owner here: expanded macros can share them. The
//! real sibling dispatch records two parse-local markers, and only the actual
//! IP execution completes the witness. Preparation consumes that pair once.

use std::collections::HashMap;

use mant_ir::{Block, DefinitionItem, Inline, LayoutHint, SourceSpan};

use super::{evidence::head_content, groups::native_inline_owner};

struct HeadWitness {
    key: usize,
    literal: bool,
    source: Option<SourceSpan>,
    children: Vec<Inline>,
    layout: LayoutHint,
    operands: Vec<super::NativeOperand>,
}

struct PairWitness {
    head: HeadWitness,
    body_key: usize,
    body_source: Option<SourceSpan>,
}

/// Optional source evidence for semantic ownership, never formatter state.
#[derive(Default)]
pub(crate) struct HangingOwnerEvidence {
    #[cfg(feature = "roff")]
    pending: HashMap<usize, HeadWitness>,
    pairs: HashMap<usize, PairWitness>,
}

impl HangingOwnerEvidence {
    /// Called after HP actually produced one paragraph, with the driver's
    /// immediate next sibling. A predicted IP alone is not a completed pair.
    #[cfg(feature = "roff")]
    pub(crate) fn head(
        &mut self,
        block: &mut Block,
        key: usize,
        next_key: usize,
        captured: super::CapturedHeadOperands,
    ) {
        let literal = matches!(block, Block::Preformatted { .. });
        let (Block::Paragraph {
            children,
            layout,
            source,
        }
        | Block::Preformatted {
            children,
            layout,
            source,
            ..
        }) = block
        else {
            return;
        };
        self.pending.insert(
            next_key,
            HeadWitness {
                key,
                literal,
                source: *source,
                children: head_content(std::slice::from_ref(children))
                    .pop()
                    .unwrap_or_default(),
                layout: *layout,
                operands: if captured.text == mant_ir::inline_plain_text(children) {
                    captured.operands
                } else {
                    Vec::new()
                },
            },
        );
        super::groups::mark_native_inline_owner(children, key);
    }

    /// Used only after `matches()` has proved this exact head/body pair.
    pub(super) fn operands(&self, head: &Block) -> &[super::NativeOperand] {
        let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = head
        else {
            return &[];
        };
        native_inline_owner(children)
            .and_then(|key| self.pairs.get(&key))
            .map_or(&[], |witness| witness.head.operands.as_slice())
    }

    /// Called after the proven IP really executed. Keep its entire description
    /// in place; only an existing direct inline carrier holds the private mark.
    #[cfg(feature = "roff")]
    pub(crate) fn body(&mut self, item: &mut DefinitionItem, key: usize) {
        let Some(head) = self.pending.remove(&key) else {
            return;
        };
        if head_content(&item.terms)
            .iter()
            .any(|term| !term.is_empty())
        {
            return;
        }
        let source = item.source;
        let Some(children) = body_carrier_mut(&mut item.description) else {
            return;
        };
        if mant_ir::inline_plain_text(children).trim().is_empty() {
            return;
        }
        super::groups::mark_native_inline_owner(children, key);
        self.pairs.insert(
            head.key,
            PairWitness {
                head,
                body_key: key,
                body_source: source,
            },
        );
    }

    pub(super) fn matches(&self, head: &Block, body: &DefinitionItem) -> bool {
        let (Block::Paragraph {
            children,
            layout,
            source,
        }
        | Block::Preformatted {
            children,
            layout,
            source,
            ..
        }) = head
        else {
            return false;
        };
        let Some(key) = native_inline_owner(children) else {
            return false;
        };
        let Some(witness) = self.pairs.get(&key) else {
            return false;
        };
        key == witness.head.key
            && matches!(head, Block::Preformatted { .. }) == witness.head.literal
            && *source == witness.head.source
            && *layout == witness.head.layout
            && head_content(std::slice::from_ref(children))[0] == witness.head.children
            && body.source == witness.body_source
            && body_carrier(&body.description).and_then(native_inline_owner)
                == Some(witness.body_key)
    }

    /// Drop only this handoff's markers before public form paths are captured.
    /// Native declaration-group markers inside descriptions remain available.
    pub(super) fn consume(head: &mut Block, body: &mut DefinitionItem) {
        if let Block::Paragraph { children, .. } | Block::Preformatted { children, .. } = head {
            remove_marker(children);
        }
        if let Some(children) = body_carrier_mut(&mut body.description) {
            remove_marker(children);
        }
    }
}

fn remove_marker(children: &mut Vec<Inline>) {
    let Some(key) = native_inline_owner(children) else {
        return;
    };
    children.retain(|inline| native_inline_owner(std::slice::from_ref(inline)) != Some(key));
}

/// Spacing can precede the direct prose carrier, but a table, another owner or
/// any structural block cannot be searched through to obtain a convenient mark.
fn body_carrier(blocks: &[Block]) -> Option<&[Inline]> {
    match blocks
        .iter()
        .find(|block| !matches!(block, Block::VerticalSpace { .. }))?
    {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => Some(children),
        _ => None,
    }
}

fn body_carrier_mut(blocks: &mut [Block]) -> Option<&mut Vec<Inline>> {
    match blocks
        .iter_mut()
        .find(|block| !matches!(block, Block::VerticalSpace { .. }))?
    {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => Some(children),
        _ => None,
    }
}

#[cfg(all(test, feature = "roff"))]
mod tests {
    use super::*;

    fn head() -> Block {
        Block::Paragraph {
            children: vec![Inline::Code {
                value: "--one".into(),
            }],
            layout: LayoutHint {
                continuation_indent_columns: 7,
                ..LayoutHint::default()
            },
            source: Some(source(4)),
        }
    }

    fn body() -> DefinitionItem {
        DefinitionItem {
            terms: Vec::new(),
            description: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: "Description".into(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            entry: None,
            layout: mant_ir::DefinitionLayout::default(),
            source: Some(source(5)),
        }
    }

    fn source(line: u32) -> SourceSpan {
        SourceSpan {
            line,
            column: 2,
            byte_range: None,
            end_line: None,
            end_column: None,
        }
    }

    #[test]
    fn same_coordinates_cannot_substitute_a_different_executed_pair() {
        let mut proof = HangingOwnerEvidence::default();
        let mut left = head();
        let mut left_body = body();
        let mut right = head();
        let mut right_body = body();
        proof.head(
            &mut left,
            10,
            11,
            super::super::CapturedHeadOperands::default(),
        );
        proof.head(
            &mut right,
            20,
            21,
            super::super::CapturedHeadOperands::default(),
        );
        assert!(
            !proof.matches(&left, &left_body),
            "future IP has not executed"
        );
        proof.body(&mut left_body, 11);
        proof.body(&mut right_body, 21);
        assert!(proof.matches(&left, &left_body));
        assert!(proof.matches(&right, &right_body));
        assert!(!proof.matches(&left, &right_body));
        assert!(!proof.matches(&right, &left_body));
        if let Block::Paragraph { layout, .. } = &mut right {
            layout.continuation_indent_columns += 1;
        }
        assert!(
            !proof.matches(&right, &right_body),
            "complete native geometry witness"
        );
    }

    #[test]
    fn consuming_one_pair_does_not_remove_other_native_owner_markers() {
        let mut proof = HangingOwnerEvidence::default();
        let mut head = head();
        let mut body = body();
        proof.head(
            &mut head,
            10,
            11,
            super::super::CapturedHeadOperands::default(),
        );
        proof.body(&mut body, 11);
        let carrier = body_carrier_mut(&mut body.description).unwrap();
        super::super::groups::mark_native_inline_owner(carrier, 99);
        // Another owner cannot take over the handoff's direct carrier.
        assert!(!proof.matches(&head, &body));
        carrier_remove(&mut body.description, 99);
        assert!(proof.matches(&head, &body));
        HangingOwnerEvidence::consume(&mut head, &mut body);
        assert!(
            !proof.matches(&head, &body),
            "one consumed pair cannot wrap recursively"
        );
    }

    fn carrier_remove(blocks: &mut [Block], key: usize) {
        body_carrier_mut(blocks)
            .unwrap()
            .retain(|inline| native_inline_owner(std::slice::from_ref(inline)) != Some(key));
    }
}
