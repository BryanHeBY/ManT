//! Bind a sibling stream's tbl rows to shared native execution and source blocks.
use super::super::execution::TableExecution;
use crate::mandoc::{LoweringContext, TableTextBlock};
use libmandoc_rs::{Node, NodeKind, TableCellKind};
use std::rc::Rc;

pub(in crate::mandoc::blocks) struct TableEmbedding<'a> {
    pub(in crate::mandoc::blocks::tables) blocks: Vec<Option<TableTextBlock>>,
    pub(in crate::mandoc::blocks::tables) execution: Rc<TableExecution<'a>>,
}

fn table_embeddings<'a>(
    nodes: &'a [Node],
    context: &LoweringContext<'_>,
) -> TableEmbeddingPlan<'a> {
    let mut embeddings = (0..nodes.len()).map(|_| None).collect::<Vec<_>>();
    let mut groups: Vec<Vec<(usize, &Node)>> = Vec::new();
    for (index, node) in nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.kind == NodeKind::Table)
    {
        if node.flags.table_start || groups.is_empty() {
            groups.push(Vec::new());
        }
        groups.last_mut().expect("table group").push((index, node));
    }
    for group in groups {
        let execution = Rc::new(TableExecution::new(
            group.iter().map(|(_, node)| *node).collect(),
        ));
        for (index, _) in group {
            embeddings[index] = Some(TableEmbedding {
                blocks: Vec::new(),
                execution: Rc::clone(&execution),
            });
        }
    }
    for (index, node) in nodes.iter().enumerate() {
        if node.kind != NodeKind::Table {
            continue;
        }
        // Whole-cell execution proof precedes optional source copying.
        // The row-wide summary cannot disable a proven safe sibling.
        if !node.table_cells.iter().any(|cell| {
            cell.text_block
                && cell.source_recovery_safe
                && cell.kind == TableCellKind::Text
                && !cell.vertical_continuation
        }) {
            continue;
        }
        let blocks = context.table_text_blocks(
            node.line,
            node.table_cells
                .iter()
                .filter(|cell| cell.text_block)
                .map(|cell| {
                    cell.source_recovery_safe
                        && cell.kind == TableCellKind::Text
                        && !cell.vertical_continuation
                }),
            node.table_escape,
        );
        if blocks.is_empty() {
            continue;
        }
        embeddings[index].as_mut().expect("table embedding").blocks = blocks;
    }
    TableEmbeddingPlan { embeddings }
}

/// One sibling stream owns source coordinates for embedded table text.
///
/// Native macro expansion can reuse the source line of a table cell while
/// emitting ordinary AST siblings after the table. Those siblings remain in
/// the normal block stream unless a native execution witness proves that a
/// source recovery transaction consumed them; line-number overlap alone is
/// never ownership evidence.
pub(in crate::mandoc::blocks) struct TableEmbeddingPlan<'a> {
    embeddings: Vec<Option<TableEmbedding<'a>>>,
}
impl<'a> TableEmbeddingPlan<'a> {
    pub(in crate::mandoc::blocks) fn new(nodes: &'a [Node], context: &LoweringContext<'_>) -> Self {
        table_embeddings(nodes, context)
    }
    pub(in crate::mandoc::blocks) fn embedding(&self, index: usize) -> Option<&TableEmbedding<'a>> {
        self.embeddings[index].as_ref()
    }
}
