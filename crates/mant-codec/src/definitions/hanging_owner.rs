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
    /// Preserve only an already matching execution proof through a producer
    /// reading transform. A changed/mismatching head cannot acquire evidence
    /// merely because its normalized text resembles a captured head.
    #[cfg(feature = "roff")]
    pub(crate) fn transform_literal_head(
        &mut self,
        block: &mut Block,
        transform: impl FnOnce(&mut Block) -> usize,
    ) {
        let key = match block {
            Block::Preformatted { children, .. } => native_inline_owner(children),
            _ => None,
        };
        let matches = key
            .and_then(|key| self.pairs.get(&key))
            .is_some_and(|witness| witness.head.matches(block));
        let removed_bytes = transform(block);
        if removed_bytes == 0 {
            return;
        }
        // A transform cannot rehabilitate an already invalid witness even if
        // the resulting content happens to equal its old snapshot.
        if !matches {
            if let Some(key) = key {
                self.pairs.remove(&key);
            }
            return;
        }
        if let Some(witness) = key.and_then(|key| self.pairs.get_mut(&key))
            && let Block::Preformatted {
                children, layout, ..
            } = block
        {
            witness.head.children = head_content(std::slice::from_ref(children))
                .pop()
                .unwrap_or_default();
            witness.head.layout = *layout;
            witness.head.operands.retain_mut(|operand| {
                operand.bytes.start = operand.bytes.start.saturating_sub(removed_bytes);
                operand.bytes.end = operand.bytes.end.saturating_sub(removed_bytes);
                !operand.bytes.is_empty()
            });
        }
    }

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
            ..
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
        let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = head
        else {
            return false;
        };
        let Some(key) = native_inline_owner(children) else {
            return false;
        };
        let Some(witness) = self.pairs.get(&key) else {
            return false;
        };
        witness.head.matches(head)
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

impl HeadWitness {
    fn matches(&self, block: &Block) -> bool {
        let (Block::Paragraph {
            children,
            layout,
            source,
            ..
        }
        | Block::Preformatted {
            children,
            layout,
            source,
            ..
        }) = block
        else {
            return false;
        };
        native_inline_owner(children) == Some(self.key)
            && matches!(block, Block::Preformatted { .. }) == self.literal
            && *source == self.source
            && *layout == self.layout
            && head_content(std::slice::from_ref(children))[0] == self.children
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
mod tests;
