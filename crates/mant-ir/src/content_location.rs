//! Checked, snapshot-local addresses into authoritative document content.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Block, Document, EntryContentSlice, EntryInlineRoot, EntryOwner, Inline, Section};

/// Maximum structural or inline nesting accepted by content addressing.
pub const MAX_CONTENT_DEPTH: usize = 256;
/// Maximum compact JSON bytes for a materialized content position.
pub const MAX_CONTENT_LOCATION_BYTES: usize = 8 * 1024;

/// A checked transition through block arrays and their containing items/cells.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ContentBlockStep {
    /// One ordinary list item's block array.
    ListItem {
        /// Zero-based item index.
        index: u32,
    },
    /// One definition item's description block array.
    DefinitionItem {
        /// Zero-based item index.
        index: u32,
    },
    /// One original table cell, independent of visual spans.
    TableCell {
        /// Zero-based original row.
        row: u32,
        /// Zero-based original cell.
        column: u32,
    },
    /// One block in the current block array.
    Block {
        /// Zero-based block index.
        index: u32,
    },
}

/// Inline root within the addressed final block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ContentInlineRoot {
    /// Paragraph or preformatted content.
    Inlines,
    /// A term in a definition list, whether or not it has semantic facts.
    DefinitionTerm {
        /// Zero-based item in the addressed definition list.
        item_index: u32,
        /// Zero-based term in the selected item.
        term_index: u32,
    },
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ClosedInlineRoot {
    Inlines {},
    DefinitionTerm { item_index: u32, term_index: u32 },
}

impl<'de> Deserialize<'de> for ContentInlineRoot {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ClosedInlineRoot::deserialize(deserializer)? {
            ClosedInlineRoot::Inlines {} => Self::Inlines,
            ClosedInlineRoot::DefinitionTerm {
                item_index,
                term_index,
            } => Self::DefinitionTerm {
                item_index,
                term_index,
            },
        })
    }
}

/// Exact final-IR inline position, valid only in the document being addressed.
///
/// An empty inline path denotes the whole inline root; a link occurrence always
/// uses a nonempty path to its actual `Inline::Link`. Source bytes, visible
/// scalar ranges and terminal cells are not represented by this type.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ContentLocation {
    /// Original visible document heading, not native bibliographic metadata.
    DocumentHeading {
        /// Zero-based inline child indices.
        path: Vec<u32>,
    },
    /// Original visible heading of a structurally addressed section.
    SectionHeading {
        /// Zero-based nested section indices, nonempty for a section heading.
        sections: Vec<u32>,
        /// Zero-based inline child indices.
        path: Vec<u32>,
    },
    /// Content before sections or within one section.
    Content {
        /// Zero-based nested section indices; empty selects root content.
        sections: Vec<u32>,
        /// Typed path beginning with a block step.
        blocks: Vec<ContentBlockStep>,
        /// Inline container within the addressed block.
        root: ContentInlineRoot,
        /// Zero-based inline child indices.
        path: Vec<u32>,
    },
}

/// Borrowed position over a traversal's reusable path stacks.
/// This view must be copied explicitly to retain it beyond the callback.
#[derive(Debug, Clone, Copy)]
pub enum ContentLocationRef<'a> {
    /// Visible document heading.
    DocumentHeading {
        /// Borrowed inline child indices.
        path: &'a [u32],
    },
    /// Visible section heading.
    SectionHeading {
        /// Borrowed nested section indices.
        sections: &'a [u32],
        /// Borrowed inline child indices.
        path: &'a [u32],
    },
    /// Root/section block content.
    Content {
        /// Borrowed nested section indices.
        sections: &'a [u32],
        /// Borrowed block/item/cell steps.
        blocks: &'a [ContentBlockStep],
        /// Addressed block's inline root.
        root: ContentInlineRoot,
        /// Borrowed inline child indices.
        path: &'a [u32],
    },
}

impl ContentLocation {
    /// Borrow this position without allocating.
    #[must_use]
    pub fn as_ref(&self) -> ContentLocationRef<'_> {
        match self {
            Self::DocumentHeading { path } => ContentLocationRef::DocumentHeading { path },
            Self::SectionHeading { sections, path } => {
                ContentLocationRef::SectionHeading { sections, path }
            }
            Self::Content {
                sections,
                blocks,
                root,
                path,
            } => ContentLocationRef::Content {
                sections,
                blocks,
                root: *root,
                path,
            },
        }
    }

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
    /// Number of section/block/inline path coordinates inspected by encoding
    /// or copying this position. Budgeted consumers charge this work separately
    /// when retaining a position after scanning it.
    #[must_use]
    pub fn depth(self) -> usize {
        match self {
            Self::DocumentHeading { path } => path.len(),
            Self::SectionHeading { sections, path } => sections.len().saturating_add(path.len()),
            Self::Content {
                sections,
                blocks,
                path,
                ..
            } => sections
                .len()
                .saturating_add(blocks.len())
                .saturating_add(path.len()),
        }
    }

    /// Size of this position's compact JSON encoding, checked before copying.
    /// No path, text or intermediate JSON string is allocated.
    #[must_use]
    pub fn encoded_len(self) -> usize {
        fn indices(values: &[u32]) -> usize {
            2 + values.iter().map(|n| digits(*n)).sum::<usize>() + values.len().saturating_sub(1)
        }
        fn steps(values: &[ContentBlockStep]) -> usize {
            2 + values
                .iter()
                .map(|step| match step {
                    ContentBlockStep::Block { index } => {
                        "{\"kind\":\"block\",\"index\":}".len() + digits(*index)
                    }
                    ContentBlockStep::ListItem { index } => {
                        "{\"kind\":\"list-item\",\"index\":}".len() + digits(*index)
                    }
                    ContentBlockStep::DefinitionItem { index } => {
                        "{\"kind\":\"definition-item\",\"index\":}".len() + digits(*index)
                    }
                    ContentBlockStep::TableCell { row, column } => {
                        "{\"kind\":\"table-cell\",\"row\":,\"column\":}".len()
                            + digits(*row)
                            + digits(*column)
                    }
                })
                .sum::<usize>()
                + values.len().saturating_sub(1)
        }
        match self {
            Self::DocumentHeading { path } => {
                "{\"kind\":\"document-heading\",\"path\":}".len() + indices(path)
            }
            Self::SectionHeading { sections, path } => {
                "{\"kind\":\"section-heading\",\"sections\":,\"path\":}".len()
                    + indices(sections)
                    + indices(path)
            }
            Self::Content {
                sections,
                blocks,
                root,
                path,
            } => {
                let root = match root {
                    ContentInlineRoot::Inlines => "{\"kind\":\"inlines\"}".len(),
                    ContentInlineRoot::DefinitionTerm {
                        item_index,
                        term_index,
                    } => {
                        "{\"kind\":\"definition-term\",\"itemIndex\":,\"termIndex\":}".len()
                            + digits(item_index)
                            + digits(term_index)
                    }
                };
                "{\"kind\":\"content\",\"sections\":,\"blocks\":,\"root\":,\"path\":}".len()
                    + indices(sections)
                    + steps(blocks)
                    + root
                    + indices(path)
            }
        }
    }

    /// Retain a bounded position. Oversized/deep paths are never partially copied.
    #[must_use]
    pub fn to_owned(self) -> Option<ContentLocation> {
        if !self.within_limits() {
            return None;
        }
        Some(match self {
            Self::DocumentHeading { path } => ContentLocation::DocumentHeading {
                path: path.to_vec(),
            },
            Self::SectionHeading { sections, path } => ContentLocation::SectionHeading {
                sections: sections.to_vec(),
                path: path.to_vec(),
            },
            Self::Content {
                sections,
                blocks,
                root,
                path,
            } => ContentLocation::Content {
                sections: sections.to_vec(),
                blocks: blocks.to_vec(),
                root,
                path: path.to_vec(),
            },
        })
    }

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

    fn within_limits(self) -> bool {
        self.depth() <= MAX_CONTENT_DEPTH && self.encoded_len() <= MAX_CONTENT_LOCATION_BYTES
    }
}

fn digits(value: u32) -> usize {
    value.checked_ilog10().unwrap_or(0) as usize + 1
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

/// An entry owner's actual item address, independent of its names or IDs.
#[derive(Debug, Clone, Copy)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document() -> Document {
        serde_json::from_value(json!({
            "parser":null,"source":{"format":"markdown"},"meta":{},
            "heading":{"content":[{"type":"link","target":{"kind":"document","name":"index"},"children":[]}]},
            "blocks":[{"type":"definition-list","items":[{
                "entry":null,"terms":[[{"type":"link","target":{"kind":"document","name":"term"},"children":[{"type":"code","value":"é名"}]}]],
                "description":[{"type":"paragraph","children":[{"type":"text","value":"body"}]}]
            }]}],"sections":[{"id":"part","heading":{"content":[{"type":"text","value":"Part"}]},"blocks":[],"children":[]}]
        })).unwrap()
    }

    #[test]
    fn typed_addresses_resolve_empty_labels_terms_and_owner_slices() {
        let document = document();
        let heading = ContentLocation::DocumentHeading { path: vec![0] };
        assert!(
            matches!(heading.resolve_link(&document), Some(Inline::Link { children, .. }) if children.is_empty())
        );
        assert!(
            ContentLocation::DocumentHeading { path: vec![] }
                .resolve_link(&document)
                .is_none()
        );
        let steps = [ContentBlockStep::Block { index: 0 }];
        let owner = EntryOwnerLocationRef {
            sections: &[],
            blocks: &steps,
            item_index: 0,
        };
        let term = owner
            .map_slice(
                &document,
                &EntryContentSlice {
                    root: EntryInlineRoot::Term { index: 0 },
                    path: vec![0, 0],
                    bytes: Some(0..2),
                },
            )
            .unwrap();
        assert!(
            matches!(term.resolve(&document), Some([Inline::Code { value }]) if value == "é名")
        );
        assert!(
            owner
                .map_slice(
                    &document,
                    &EntryContentSlice {
                        root: EntryInlineRoot::Term { index: 0 },
                        path: vec![0, 0],
                        bytes: Some(0..1)
                    }
                )
                .is_none()
        );
        let body = owner
            .map_slice(
                &document,
                &EntryContentSlice {
                    root: EntryInlineRoot::Block { index: 0 },
                    path: vec![0],
                    bytes: None,
                },
            )
            .unwrap();
        assert!(
            matches!(body.resolve(&document), Some([Inline::Text { value }]) if value == "body")
        );
    }

    #[test]
    fn compact_encoding_budget_is_exact_and_checked_before_materialization() {
        for location in [
            ContentLocation::DocumentHeading {
                path: vec![0, u32::MAX],
            },
            ContentLocation::SectionHeading {
                sections: vec![3, 14],
                path: vec![2],
            },
            ContentLocation::Content {
                sections: vec![],
                blocks: vec![
                    ContentBlockStep::Block { index: 123 },
                    ContentBlockStep::ListItem { index: 4 },
                    ContentBlockStep::Block { index: 5 },
                    ContentBlockStep::TableCell { row: 6, column: 7 },
                    ContentBlockStep::Block { index: 8 },
                    ContentBlockStep::DefinitionItem { index: 9 },
                    ContentBlockStep::Block { index: 10 },
                ],
                root: ContentInlineRoot::DefinitionTerm {
                    item_index: 123,
                    term_index: 12,
                },
                path: vec![4, 3],
            },
            ContentLocation::Content {
                sections: vec![0],
                blocks: vec![ContentBlockStep::Block { index: 0 }],
                root: ContentInlineRoot::Inlines,
                path: vec![],
            },
        ] {
            assert_eq!(
                location.as_ref().encoded_len(),
                serde_json::to_vec(&location).unwrap().len()
            );
            assert_eq!(location.as_ref().to_owned(), Some(location));
        }
        let huge = vec![u32::MAX; MAX_CONTENT_DEPTH + 1];
        assert!(
            ContentLocationRef::DocumentHeading { path: &huge }
                .to_owned()
                .is_none()
        );
        let huge = vec![ContentBlockStep::DefinitionItem { index: u32::MAX }; MAX_CONTENT_DEPTH];
        let oversized = ContentLocationRef::Content {
            sections: &[],
            blocks: &huge,
            root: ContentInlineRoot::Inlines,
            path: &[],
        };
        assert!(oversized.encoded_len() > MAX_CONTENT_LOCATION_BYTES);
        assert!(oversized.to_owned().is_none());
    }

    #[test]
    fn wrong_container_and_out_of_bounds_addresses_never_fall_back() {
        let document = document();
        let good = ContentLocation::Content {
            sections: vec![],
            blocks: vec![ContentBlockStep::Block { index: 0 }],
            root: ContentInlineRoot::DefinitionTerm {
                item_index: 0,
                term_index: 0,
            },
            path: vec![0],
        };
        assert!(good.resolve_link(&document).is_some());
        for (blocks, root, path) in [
            (
                vec![ContentBlockStep::ListItem { index: 0 }],
                ContentInlineRoot::Inlines,
                vec![0],
            ),
            (
                vec![ContentBlockStep::Block { index: 0 }],
                ContentInlineRoot::Inlines,
                vec![0],
            ),
            (
                vec![
                    ContentBlockStep::Block { index: 0 },
                    ContentBlockStep::ListItem { index: 0 },
                    ContentBlockStep::Block { index: 0 },
                ],
                ContentInlineRoot::Inlines,
                vec![0],
            ),
            (
                vec![ContentBlockStep::Block { index: 0 }],
                ContentInlineRoot::DefinitionTerm {
                    item_index: 1,
                    term_index: 0,
                },
                vec![0],
            ),
            (
                vec![ContentBlockStep::Block { index: 0 }],
                ContentInlineRoot::DefinitionTerm {
                    item_index: 0,
                    term_index: 0,
                },
                vec![0, 0, 0],
            ),
        ] {
            assert!(
                ContentLocation::Content {
                    sections: vec![],
                    blocks,
                    root,
                    path
                }
                .resolve(&document)
                .is_none()
            );
        }
        assert!(
            ContentLocation::SectionHeading {
                sections: vec![],
                path: vec![0]
            }
            .resolve(&document)
            .is_none()
        );
        assert!(
            ContentLocation::SectionHeading {
                sections: vec![99],
                path: vec![0]
            }
            .resolve(&document)
            .is_none()
        );
        let encoded = serde_json::to_value(good).unwrap();
        let mut unknown = encoded.clone();
        unknown["guess"] = json!(true);
        assert!(serde_json::from_value::<ContentLocation>(unknown).is_err());
        let mut negative = encoded;
        negative["path"] = json!([-1]);
        assert!(serde_json::from_value::<ContentLocation>(negative).is_err());
        assert!(
            serde_json::from_value::<ContentInlineRoot>(json!({"kind":"inlines","unknown":true}))
                .is_err()
        );
    }

    #[test]
    fn shared_block_resolver_preserves_response_pair_depth_contract() {
        let mut block = Block::Paragraph {
            children: vec![],
            layout: crate::LayoutHint::default(),
            source: None,
        };
        let mut path = Vec::new();
        for _ in 0..=MAX_CONTENT_DEPTH {
            block = Block::List {
                kind: crate::ListKind::Bullet,
                compact: true,
                layout: crate::LayoutHint::default(),
                source: None,
                items: vec![crate::ListItem {
                    layout: crate::ListItemLayout::default(),
                    source: None,
                    entry: None,
                    blocks: vec![block],
                }],
            };
            path.extend([
                ContentBlockStep::ListItem { index: 0 },
                ContentBlockStep::Block { index: 0 },
            ]);
        }
        assert!(matches!(
            resolve_block_descendant(&block, &path),
            Some(Block::Paragraph { .. })
        ));
        path.extend([
            ContentBlockStep::ListItem { index: 0 },
            ContentBlockStep::Block { index: 0 },
        ]);
        assert!(resolve_block_descendant(&block, &path).is_none());
    }
}
