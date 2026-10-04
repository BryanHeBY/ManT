//! Borrow the effective BODY without moving its content or interpreting source.
use super::{Block, DefinitionItem, HeadBodyRelation, Inline, LayoutHint};
use crate::InlineContentRef;

/// The first effective description block and its already resolved boundary.
///
/// Empty prose, navigation-only inline roots and zero-row spacing remain at
/// their original addresses but do not create physical rows. Authored literal
/// rows, hard breaks, whitespace and structural blocks are effective content.
/// This view does not execute a formatter or own a second copy of the body.
#[derive(Debug, Clone, Copy)]
pub struct DefinitionBodyRef<'a> {
    /// Original index in [`DefinitionItem::description`].
    pub block_index: usize,
    /// Original transparent/spacing blocks preceding the effective block.
    /// Their navigation targets and positive spacing must still be consumed.
    pub leading_blocks: &'a [Block],
    /// Original effective block; later blocks are outside this shared fragment.
    pub block: &'a Block,
    /// Positive spacing before or on this block prevents HEAD/BODY sharing.
    pub has_leading_spacing: bool,
}

impl<'a> DefinitionBodyRef<'a> {
    /// Whether the declared relation admits this inline block on a HEAD row.
    /// Consumers must still have a physical HEAD row to join to.
    #[must_use]
    pub const fn can_share(self, relation: HeadBodyRelation) -> bool {
        !relation.is_default() && !self.has_leading_spacing && self.inline_content().is_some()
    }

    /// Borrow inline content and owner layout when this is prose or literal.
    #[must_use]
    pub const fn inline_content(self) -> Option<(InlineContentRef<'a>, &'a LayoutHint)> {
        match self.block {
            Block::Paragraph {
                children,
                inline_layout,
                layout,
                ..
            }
            | Block::Preformatted {
                children,
                inline_layout,
                layout,
                ..
            } => Some((
                InlineContentRef {
                    content: children.as_slice(),
                    layout: inline_layout,
                },
                layout,
            )),
            _ => None,
        }
    }
}

impl DefinitionItem {
    /// Select the first effective BODY and its leading boundary once.
    ///
    /// Selection reads source-neutral IR facts. In particular, an empty prose
    /// node is not an upstream empty TEXT request: any executed source spacing
    /// has already been retained in a layout or `VerticalSpace` block.
    /// A structural block stops selection even when a format later simplifies
    /// its rendering. No future BODY block can change this boundary.
    #[must_use]
    pub fn description_start(&self) -> Option<DefinitionBodyRef<'_>> {
        let mut has_leading_spacing = false;
        for (block_index, block) in self.description.iter().enumerate() {
            has_leading_spacing |= crate::geometry::block_gap(block) > 0;
            let effective = match block {
                Block::Paragraph { children, .. } => {
                    crate::first_visible_character(children).is_some()
                }
                Block::Preformatted { children, .. } => crate::geometry::has_literal_rows(children),
                Block::VerticalSpace { .. } => false,
                _ => true,
            };
            if effective {
                return Some(DefinitionBodyRef {
                    block_index,
                    leading_blocks: &self.description[..block_index],
                    block,
                    has_leading_spacing,
                });
            }
        }
        None
    }

    /// Borrow the effective inline BODY admitted by the declared shared relation.
    ///
    /// Positive leading spacing or a structural first BODY prevents sharing.
    /// Consumers also require a physical HEAD row before joining. The prefix
    /// remains borrowed for navigation, and all blocks after `block_index` keep
    /// their original independent origins and addresses.
    #[must_use]
    pub fn shared_description(&self) -> Option<DefinitionBodyRef<'_>> {
        if !self.inline_term() {
            return None;
        }
        let body = self.description_start()?;
        body.can_share(self.head_body_relation).then_some(body)
    }

    /// The first effective content row that can share the final term row.
    ///
    /// This compatibility view omits the original block index. Consumers that
    /// process the remaining BODY should use [`Self::shared_description`] so
    /// zero-output prefixes do not shift their slice or drop navigation targets.
    #[must_use]
    pub fn inline_description(&self) -> Option<(&[Inline], &LayoutHint)> {
        let (content, layout) = self.inline_description_content()?;
        Some((content.content, layout))
    }

    /// Borrow the first effective shared row and its original owner layout.
    #[must_use]
    pub fn inline_description_content(&self) -> Option<(InlineContentRef<'_>, &LayoutHint)> {
        self.shared_description()?.inline_content()
    }
}

#[cfg(test)]
mod tests;
