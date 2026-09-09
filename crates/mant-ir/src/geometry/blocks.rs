//! Shared access to fields carried by most renderer-neutral block variants.
//!
//! Exhaustive accessors keep producers and consumers aligned as block variants
//! evolve. Reparenting mutates only moved roots, never descendant coordinates.

use crate::{Block, LayoutHint, SourceSpan};

/// Return a block's layout hint when its representation carries one.
#[must_use]
pub const fn block_layout(block: &Block) -> Option<&LayoutHint> {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(layout),
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => None,
    }
}

/// Return a mutable block layout hint when its representation carries one.
#[must_use]
pub const fn block_layout_mut(block: &mut Block) -> Option<&mut LayoutHint> {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(layout),
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => None,
    }
}

/// Return the original source location attached to a block.
#[must_use]
pub const fn block_source(block: &Block) -> Option<SourceSpan> {
    match block {
        Block::Paragraph { source, .. }
        | Block::Preformatted { source, .. }
        | Block::List { source, .. }
        | Block::DefinitionList { source, .. }
        | Block::Table { source, .. }
        | Block::Equation { source, .. }
        | Block::VerticalSpace { source, .. }
        | Block::ThematicBreak { source }
        | Block::Unsupported { source, .. } => *source,
    }
}

/// Move already-relative roots between actual content origins. Descendants
/// remain relative to those roots and must never receive this translation.
/// Non-layout blocks, source spans, spacing and continuation offsets are unchanged.
/// Arithmetic saturates at the signed coordinate limits, without wrapping.
pub fn rebase_roots(blocks: &mut [Block], old_parent: i32, new_parent: i32) {
    for block in blocks {
        if let Some(layout) = block_layout_mut(block) {
            layout.indent_columns =
                super::rebase_origin(layout.indent_columns, old_parent, new_parent);
        }
    }
}

#[cfg(test)]
mod tests;
