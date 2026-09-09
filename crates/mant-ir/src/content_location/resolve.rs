//! Checked traversal from snapshot-local coordinates into borrowed content.
use super::{
    ContentBlockStep, ContentInlineRoot, ContentLocation, ContentLocationRef, MAX_CONTENT_DEPTH,
};
use crate::{Block, Document, Inline, Section};

impl ContentLocation {
    /// Resolve the checked inline container or singleton node in this snapshot.
    #[must_use]
    pub fn resolve<'a>(&self, document: &'a Document) -> Option<&'a [Inline]> {
        self.as_ref().resolve(document)
    }

    /// Resolve precisely one real link, including an empty-label link.
    #[must_use]
    pub fn resolve_link<'a>(&self, document: &'a Document) -> Option<&'a Inline> {
        self.as_ref().resolve_link(document)
    }
}

impl ContentLocationRef<'_> {
    /// Check container kinds and bounds against the exact supplied document.
    #[must_use]
    pub fn resolve(self, document: &Document) -> Option<&[Inline]> {
        if !self.within_limits() {
            return None;
        }
        let (nodes, path) = match self {
            Self::DocumentHeading { path } => (document.heading.as_ref()?.content.as_slice(), path),
            Self::SectionHeading { sections, path } => (
                resolve_content_section(document, sections)?
                    .heading
                    .content
                    .as_slice(),
                path,
            ),
            Self::Content {
                sections,
                blocks,
                root,
                path,
            } => {
                let blocks_root = content_blocks(document, sections)?;
                let block = resolve_content_block(blocks_root, blocks)?;
                (resolve_inline_root(block, root)?, path)
            }
        };
        resolve_inline_path(nodes, path)
    }

    /// A link occurrence must identify a node, not a whole one-link container.
    #[must_use]
    pub fn resolve_link(self, document: &Document) -> Option<&Inline> {
        let path = match self {
            Self::DocumentHeading { path }
            | Self::SectionHeading { path, .. }
            | Self::Content { path, .. } => path,
        };
        if path.is_empty() {
            return None;
        }
        match self.resolve(document)? {
            [link @ Inline::Link { .. }] => Some(link),
            _ => None,
        }
    }
}

/// Resolve a nonempty path of section child indices.
#[must_use]
pub fn resolve_content_section<'a>(document: &'a Document, path: &[u32]) -> Option<&'a Section> {
    if path.is_empty() || path.len() > MAX_CONTENT_DEPTH {
        return None;
    }
    let mut children = document.sections.as_slice();
    let mut selected = None;
    for index in path {
        let section = children.get(*index as usize)?;
        selected = Some(section);
        children = &section.children;
    }
    selected
}

pub(crate) fn content_blocks<'a>(document: &'a Document, sections: &[u32]) -> Option<&'a [Block]> {
    if sections.is_empty() {
        Some(&document.blocks)
    } else {
        Some(&resolve_content_section(document, sections)?.blocks)
    }
}

/// Resolve a typed path beginning with a block in the supplied array.
#[must_use]
pub fn resolve_content_block<'a>(
    blocks: &'a [Block],
    path: &[ContentBlockStep],
) -> Option<&'a Block> {
    let (ContentBlockStep::Block { index }, rest) = path.split_first()? else {
        return None;
    };
    if path.len() > MAX_CONTENT_DEPTH {
        return None;
    }
    resolve_block_descendant(blocks.get(*index as usize)?, rest)
}

/// Resolve item/cell + block pairs relative to an already selected block.
/// This is also the primitive for response-local explanation positions.
#[must_use]
pub fn resolve_block_descendant<'a>(
    mut block: &'a Block,
    path: &[ContentBlockStep],
) -> Option<&'a Block> {
    // Existing response-local explanation paths count an item/cell + block
    // pair per nesting level. Preserve that established acceptance boundary;
    // document ContentLocation applies its own stricter combined-path budget.
    if path.len() > MAX_CONTENT_DEPTH * 2 + 2 || !path.len().is_multiple_of(2) {
        return None;
    }
    for pair in path.as_chunks::<2>().0 {
        let children = block_children(block, pair[0])?;
        let ContentBlockStep::Block { index } = pair[1] else {
            return None;
        };
        block = children.get(index as usize)?;
    }
    Some(block)
}

pub(crate) fn block_children(block: &Block, step: ContentBlockStep) -> Option<&[Block]> {
    match (block, step) {
        (Block::List { items, .. }, ContentBlockStep::ListItem { index }) => {
            Some(&items.get(index as usize)?.blocks)
        }
        (Block::DefinitionList { items, .. }, ContentBlockStep::DefinitionItem { index }) => {
            Some(&items.get(index as usize)?.description)
        }
        (Block::Table { rows, .. }, ContentBlockStep::TableCell { row, column }) => {
            Some(&rows.get(row as usize)?.cells.get(column as usize)?.blocks)
        }
        _ => None,
    }
}

fn resolve_inline_root(block: &Block, root: ContentInlineRoot) -> Option<&[Inline]> {
    match (block, root) {
        (
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. },
            ContentInlineRoot::Inlines,
        ) => Some(children),
        (
            Block::DefinitionList { items, .. },
            ContentInlineRoot::DefinitionTerm {
                item_index,
                term_index,
            },
        ) => Some(
            items
                .get(item_index as usize)?
                .terms
                .get(term_index as usize)?,
        ),
        _ => None,
    }
}

/// Borrow one inline subtree, or the whole root when the path is empty.
#[must_use]
pub fn resolve_inline_path<'a>(mut nodes: &'a [Inline], path: &[u32]) -> Option<&'a [Inline]> {
    if path.len() > MAX_CONTENT_DEPTH {
        return None;
    }
    for (depth, index) in path.iter().enumerate() {
        let node = nodes.get(*index as usize)?;
        nodes = if depth + 1 == path.len() {
            std::slice::from_ref(node)
        } else {
            inline_children(node)?
        };
    }
    Some(nodes)
}

pub(crate) fn inline_children(node: &Inline) -> Option<&[Inline]> {
    match node {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => Some(children),
        _ => None,
    }
}
