//! Definition walk policy; coordinated by the parent discovery passes.
use mant_ir::{Block, DefinitionItem, SourceSpan};

/// One identified definition together with its semantic coordinates.
///
/// `indices` contains the one-based position at every entry nesting level.
/// Keeping these coordinates beside the borrowed item gives outline,
/// excerpt, and addressable-Markdown projections one topology instead of
/// independently flattening definition descriptions.
pub(crate) struct DefinitionEntry<'a> {
    pub(crate) item: &'a DefinitionItem,
    pub(crate) source: Option<SourceSpan>,
    pub(crate) indices: Vec<usize>,
    pub(crate) ancestors: Vec<&'a DefinitionItem>,
}

/// Return identified definition items in semantic pre-order.
///
/// Definitions nested inside lists or table cells remain direct entries of
/// the surrounding scope. Definitions inside an entry description become
/// children of that entry, matching [`mant_ir::SemanticIndex`].
pub(crate) fn definition_entries(blocks: &[Block]) -> Vec<DefinitionEntry<'_>> {
    let mut entries = Vec::new();
    collect_definition_scope(blocks, &[], &mut Vec::new(), &mut entries);
    entries
}

fn collect_definition_scope<'a>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<&'a DefinitionItem>,
    output: &mut Vec<DefinitionEntry<'a>>,
) {
    let mut direct_index = 0;
    collect_direct_definitions(blocks, parent_indices, ancestors, &mut direct_index, output);
}

fn collect_direct_definitions<'a>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<&'a DefinitionItem>,
    direct_index: &mut usize,
    output: &mut Vec<DefinitionEntry<'a>>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    collect_direct_definitions(
                        &item.blocks,
                        parent_indices,
                        ancestors,
                        direct_index,
                        output,
                    );
                }
            }
            Block::DefinitionList { items, source, .. } => {
                for item in items {
                    if item.identity.is_none() {
                        continue;
                    }
                    *direct_index += 1;
                    let mut indices = parent_indices.to_vec();
                    indices.push(*direct_index);
                    output.push(DefinitionEntry {
                        item,
                        source: *source,
                        indices: indices.clone(),
                        ancestors: ancestors.clone(),
                    });
                    ancestors.push(item);
                    collect_definition_scope(&item.description, &indices, ancestors, output);
                    ancestors.pop();
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_direct_definitions(
                            &cell.blocks,
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    }
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
