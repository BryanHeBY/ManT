//! Source-order semantic coordinates over unchanged native and Markdown owners.
use mant_ir::{Block, EntryOwner, SourceSpan};

/// One identified owner, retaining enough context to excerpt its original item.
pub(crate) struct ContentEntry<'a> {
    pub(crate) item: EntryOwner<'a>,
    /// Validated once for this immutable location snapshot, never raw facts.
    pub(crate) names: &'a [String],
    pub(crate) source: Option<SourceSpan>,
    pub(crate) indices: Vec<usize>,
    pub(crate) ancestors: Vec<EntryOwner<'a>>,
    container: &'a Block,
    item_index: usize,
}

impl ContentEntry<'_> {
    /// Copy only the selected owner, not its siblings or a synthetic definition.
    pub(crate) fn content(&self) -> Block {
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
                items,
                compact,
                layout,
                source,
            } => Block::DefinitionList {
                items: vec![items[self.item_index].clone()],
                compact: *compact,
                layout: *layout,
                source: *source,
            },
            _ => unreachable!("entry containers are lists"),
        }
    }
}

/// Same semantic pre-order as `SemanticIndex`, with source presentation retained.
pub(crate) fn content_entries(blocks: &[Block]) -> Vec<ContentEntry<'_>> {
    let mut entries = Vec::new();
    collect_scope(blocks, &[], &mut Vec::new(), &mut entries);
    entries
}

fn collect_scope<'a>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<EntryOwner<'a>>,
    output: &mut Vec<ContentEntry<'a>>,
) {
    collect_direct(blocks, parent_indices, ancestors, &mut 0, output);
}

fn collect_direct<'a>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<EntryOwner<'a>>,
    direct_index: &mut usize,
    output: &mut Vec<ContentEntry<'a>>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    if item.entry.is_some() {
                        collect_owner(
                            block,
                            index,
                            EntryOwner::List(item),
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    } else {
                        collect_direct(
                            &item.blocks,
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    }
                }
            }
            Block::DefinitionList { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    if item.entry.is_some() {
                        collect_owner(
                            block,
                            index,
                            EntryOwner::Definition(item),
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    } else {
                        collect_direct(
                            &item.description,
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    }
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_direct(
                        &cell.blocks,
                        parent_indices,
                        ancestors,
                        direct_index,
                        output,
                    );
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_owner<'a>(
    container: &'a Block,
    item_index: usize,
    item: EntryOwner<'a>,
    parent_indices: &[usize],
    ancestors: &mut Vec<EntryOwner<'a>>,
    direct_index: &mut usize,
    output: &mut Vec<ContentEntry<'a>>,
) {
    *direct_index += 1;
    let mut indices = parent_indices.to_vec();
    indices.push(*direct_index);
    output.push(ContentEntry {
        item,
        names: item.validated_names().unwrap_or_default(),
        source: item.source(),
        indices: indices.clone(),
        ancestors: ancestors.clone(),
        container,
        item_index,
    });
    ancestors.push(item);
    collect_scope(item.blocks(), &indices, ancestors, output);
    ancestors.pop();
}
