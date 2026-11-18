//! Bind a sibling stream's tbl rows to shared native execution and source blocks.
use super::super::execution::TableExecution;
use crate::mandoc::{LoweringContext, TableTextBlock};
use libmandoc_rs::{Node, NodeKind, TableCellKind};
use std::rc::Rc;

pub(in crate::mandoc::blocks) struct TableEmbedding<'a> {
    pub(in crate::mandoc::blocks::tables) execution: Rc<TableExecution<'a>>,
}

fn table_embeddings(nodes: &[Node]) -> TableEmbeddingPlan<'_> {
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
                execution: Rc::clone(&execution),
            });
        }
    }
    TableEmbeddingPlan { embeddings }
}

/// Copy source evidence only when the row executes. An earlier nested
/// HEAD must use the page allowance before a later sibling table.
pub(in crate::mandoc::blocks::tables) fn recover_text_blocks(
    node: &Node,
    context: &LoweringContext<'_>,
) -> Vec<Option<TableTextBlock>> {
    let eligible = |cell: &libmandoc_rs::TableCell| {
        cell.source_recovery_safe && cell.kind == TableCellKind::Text && !cell.vertical_continuation
    };
    // Whole-cell execution proof precedes optional source copying.
    // The row-wide summary cannot disable a proven safe sibling.
    if !node
        .table_cells
        .iter()
        .any(|cell| cell.text_block && eligible(cell))
    {
        return Vec::new();
    }
    context.table_text_blocks(
        node.line,
        node.table_cells
            .iter()
            .filter(|cell| cell.text_block)
            .map(eligible),
        node.table_escape,
    )
}

/// One sibling stream owns its table width calculations and execution budget.
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
    pub(in crate::mandoc::blocks) fn new(nodes: &'a [Node]) -> Self {
        table_embeddings(nodes)
    }
    pub(in crate::mandoc::blocks) fn embedding(&self, index: usize) -> Option<&TableEmbedding<'a>> {
        self.embeddings[index].as_ref()
    }
}
