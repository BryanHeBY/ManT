//! Shared access to fields carried by most renderer-neutral block variants.
//!
//! Keeping these exhaustive matches in one module prevents Markdown and roff
//! normalization from drifting when the IR gains another block variant. They
//! remain internal so the engine continues to compile against every compatible
//! `mant-ir` patch release in its declared dependency range.

use mant_ir::{Block, LayoutHint, SourceSpan};

/// Return a block's layout hint when its representation carries one.
pub(crate) const fn block_layout(block: &Block) -> Option<&LayoutHint> {
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
pub(crate) const fn block_layout_mut(block: &mut Block) -> Option<&mut LayoutHint> {
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
pub(crate) const fn block_source(block: &Block) -> Option<SourceSpan> {
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
pub(crate) fn rebase_roots(blocks: &mut [mant_ir::Block], old_parent: i32, new_parent: i32) {
    for block in blocks {
        if let Some(layout) = block_layout_mut(block) {
            layout.indent_columns = mant_protocol::geometry::rebase_origin(
                layout.indent_columns,
                old_parent,
                new_parent,
            );
        }
    }
}
