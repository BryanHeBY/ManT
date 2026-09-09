//! Map one actual item's local form slices into original document positions.
use super::{
    ContentBlockStep, ContentInlineRoot, ContentLocation, ContentLocationRef, MAX_CONTENT_DEPTH,
    content_blocks, resolve_content_block, resolve_inline_path,
};
use crate::{Block, Document, EntryContentSlice, EntryInlineRoot, EntryOwner, Inline};

/// An entry owner's actual item address, independent of its names or IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryOwnerLocationRef<'a> {
    /// Section path; empty denotes document-root content.
    pub sections: &'a [u32],
    /// Path to the list or definition-list block containing the item.
    pub blocks: &'a [ContentBlockStep],
    /// Zero-based item within that block.
    pub item_index: u32,
}

impl EntryOwnerLocationRef<'_> {
    /// Resolve the actual item, including an unannotated item.
    #[must_use]
    pub fn resolve(self, document: &Document) -> Option<EntryOwner<'_>> {
        if self.sections.len().saturating_add(self.blocks.len()) > MAX_CONTENT_DEPTH {
            return None;
        }
        match resolve_content_block(content_blocks(document, self.sections)?, self.blocks)? {
            Block::List { items, .. } => {
                Some(EntryOwner::List(items.get(self.item_index as usize)?))
            }
            Block::DefinitionList { items, .. } => {
                Some(EntryOwner::Definition(items.get(self.item_index as usize)?))
            }
            _ => None,
        }
    }

    /// Map a valid owner-local slice root/path to the original document.
    /// Byte ranges are validated but intentionally remain outside node identity.
    #[must_use]
    pub fn map_slice(
        self,
        document: &Document,
        slice: &EntryContentSlice,
    ) -> Option<ContentLocation> {
        let owner = self.resolve(document)?;
        let nodes = owner.inline_root(&slice.root)?;
        let extra = usize::from(matches!(slice.root, EntryInlineRoot::Block { .. })) * 2;
        if self
            .sections
            .len()
            .saturating_add(self.blocks.len())
            .saturating_add(extra)
            .saturating_add(slice.path.len())
            > MAX_CONTENT_DEPTH
        {
            return None;
        }
        // Fixed-size scratch avoids any heap allocation before the complete
        // position's depth, coordinates and encoded byte size are checked.
        let mut path_storage = [0_u32; MAX_CONTENT_DEPTH];
        for (out, index) in path_storage.iter_mut().zip(&slice.path) {
            *out = u32::try_from(*index).ok()?;
        }
        let path = &path_storage[..slice.path.len()];
        let selected = resolve_inline_path(nodes, path)?;
        if let Some(bytes) = &slice.bytes {
            if path.is_empty() || bytes.start >= bytes.end {
                return None;
            }
            let [Inline::Text { value } | Inline::Code { value }] = selected else {
                return None;
            };
            value.get(bytes.clone())?;
        }
        let mut block_storage = [ContentBlockStep::Block { index: 0 }; MAX_CONTENT_DEPTH];
        block_storage[..self.blocks.len()].copy_from_slice(self.blocks);
        let mut blocks_len = self.blocks.len();
        let root = match slice.root {
            EntryInlineRoot::Term { index } => ContentInlineRoot::DefinitionTerm {
                item_index: self.item_index,
                term_index: u32::try_from(index).ok()?,
            },
            EntryInlineRoot::Block { index } => {
                block_storage[blocks_len] = match owner {
                    EntryOwner::List(_) => ContentBlockStep::ListItem {
                        index: self.item_index,
                    },
                    EntryOwner::Definition(_) => ContentBlockStep::DefinitionItem {
                        index: self.item_index,
                    },
                };
                block_storage[blocks_len + 1] = ContentBlockStep::Block {
                    index: u32::try_from(index).ok()?,
                };
                blocks_len += 2;
                ContentInlineRoot::Inlines
            }
        };
        ContentLocationRef::Content {
            sections: self.sections,
            blocks: &block_storage[..blocks_len],
            root,
            path,
        }
        .to_owned()
    }
}
