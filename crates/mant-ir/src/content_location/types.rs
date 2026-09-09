//! Typed content roots and their exact, bounded wire representation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    pub(super) fn within_limits(self) -> bool {
        self.depth() <= MAX_CONTENT_DEPTH && self.encoded_len() <= MAX_CONTENT_LOCATION_BYTES
    }
}

fn digits(value: u32) -> usize {
    value.checked_ilog10().unwrap_or(0) as usize + 1
}
