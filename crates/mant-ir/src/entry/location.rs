//! Source-order semantic coordinates over unchanged native and Markdown owners.
use super::walk::{owner_child_step, visit_child_entry_locations};
use crate::{Block, ContentBlockStep, EntryOwner, SourceSpan};

/// One identified owner, retaining enough context to excerpt its original item.
pub struct ContentEntry<'a> {
    item: EntryOwner<'a>,
    /// Validated once for this immutable location snapshot, never raw facts.
    names: &'a [String],
    source: Option<SourceSpan>,
    indices: Vec<usize>,
    ancestors: Vec<EntryOwner<'a>>,
    container: &'a Block,
    item_index: usize,
    block_path: Vec<ContentBlockStep>,
}

impl<'a> ContentEntry<'a> {
    /// Original semantic owner borrowed from the supplied immutable content.
    #[must_use]
    pub const fn owner(&self) -> EntryOwner<'a> {
        self.item
    }
    /// Validated semantic names, or empty when only locations were requested.
    #[must_use]
    pub const fn names(&self) -> &'a [String] {
        self.names
    }
    /// Original owner source span, when supplied by the producer.
    #[must_use]
    pub const fn source(&self) -> Option<SourceSpan> {
        self.source
    }
    /// One-based semantic child indices relative to the containing section/root.
    #[must_use]
    pub fn indices(&self) -> &[usize] {
        &self.indices
    }
    /// Semantic ancestor owners, excluding transparent structural containers.
    #[must_use]
    pub fn ancestors(&self) -> &[EntryOwner<'a>] {
        &self.ancestors
    }
    /// Zero-based item index within the original containing list block.
    #[must_use]
    pub const fn item_index(&self) -> usize {
        self.item_index
    }
    /// Physical path ending at the original containing list block.
    #[must_use]
    pub fn block_path(&self) -> &[ContentBlockStep] {
        &self.block_path
    }
    /// Copy only the selected owner, not its siblings or a synthetic definition.
    #[must_use]
    pub fn content(&self) -> Block {
        match self.container {
            Block::List {
                kind,
                compact,
                items,
                layout,
                source,
            } => Block::List {
                kind: kind.for_excerpt(self.item_index),
                compact: *compact,
                items: vec![items[self.item_index].clone()],
                layout: *layout,
                source: *source,
            },
            Block::DefinitionList {
                declaration_groups: _,
                items,
                compact,
                layout,
                source,
            } => Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![items[self.item_index].clone()],
                compact: *compact,
                layout: *layout,
                source: *source,
            },
            _ => unreachable!("entry containers are lists"),
        }
    }
}

/// Borrowed serialization of exactly the same single-owner block as `content()`.
/// Copy-budget checks can measure it without first cloning a large description.
impl serde::Serialize for ContentEntry<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;
        let mut output = serializer.serialize_map(None)?;
        let (layout, source) = match self.container {
            Block::List {
                kind,
                compact,
                items,
                layout,
                source,
            } => {
                output.serialize_entry("type", "list")?;
                output.serialize_entry("kind", &kind.for_excerpt(self.item_index))?;
                if *compact {
                    output.serialize_entry("compact", compact)?;
                }
                output.serialize_entry("items", std::slice::from_ref(&items[self.item_index]))?;
                (layout, source)
            }
            Block::DefinitionList {
                declaration_groups: _,
                items,
                compact,
                layout,
                source,
            } => {
                output.serialize_entry("type", "definition-list")?;
                output.serialize_entry("items", std::slice::from_ref(&items[self.item_index]))?;
                if *compact {
                    output.serialize_entry("compact", compact)?;
                }
                (layout, source)
            }
            _ => unreachable!("entry containers are lists"),
        };
        if !layout.is_empty() {
            output.serialize_entry("layout", layout)?;
        }
        if let Some(source) = source {
            output.serialize_entry("source", source)?;
        }
        output.end()
    }
}

/// Locate every semantic owner without validating names or materializing its body.
/// Empty/invalid names and forms never remove an addressable owner. Transparent
/// containers retain physical coordinates without consuming semantic ordinals.
#[must_use]
pub fn content_entry_locations(blocks: &[Block]) -> Vec<ContentEntry<'_>> {
    collect::<false>(blocks)
}

/// Locate semantic owners and validate their names once for this borrowed scan.
/// This reads finalized facts; it never discovers entries or infers names.
#[must_use]
pub fn content_entries(blocks: &[Block]) -> Vec<ContentEntry<'_>> {
    collect::<true>(blocks)
}

fn collect<const NAMES: bool>(blocks: &[Block]) -> Vec<ContentEntry<'_>> {
    let mut output = Vec::new();
    collect_scope::<NAMES>(blocks, &[], &mut Vec::new(), &[], &mut output);
    output
}

fn collect_scope<'a, const NAMES: bool>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<EntryOwner<'a>>,
    prefix: &[ContentBlockStep],
    output: &mut Vec<ContentEntry<'a>>,
) {
    let mut ordinal = 0;
    visit_child_entry_locations(blocks, prefix, &mut |item, container, path, item_index| {
        ordinal += 1;
        let mut indices = parent_indices.to_vec();
        indices.push(ordinal);
        output.push(ContentEntry {
            item,
            names: if NAMES {
                item.validated_names().unwrap_or_default()
            } else {
                &[]
            },
            source: item.source(),
            indices: indices.clone(),
            ancestors: ancestors.clone(),
            container,
            item_index: item_index as usize,
            block_path: path.to_vec(),
        });
        ancestors.push(item);
        let mut child_path = path.to_vec();
        child_path.push(owner_child_step(item, item_index));
        collect_scope::<NAMES>(item.blocks(), &indices, ancestors, &child_path, output);
        ancestors.pop();
    });
}

#[cfg(test)]
mod tests;
