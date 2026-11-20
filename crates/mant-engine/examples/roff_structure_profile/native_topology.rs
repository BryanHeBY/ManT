//! Collect normalized shapes from the owned native execution tree.
use libmandoc_rs::{Node, NodeKind, NormalizedListKind};
use std::collections::{BTreeMap, HashMap};

use super::{
    AstEquationTopology, AstListTopology, AstTableCellTopology, AstTableRowTopology, AstTopology,
    EquationCell, EquationContext, ListTopologyKind, equation_context_order,
    native::{equation_has_payload, equation_visible_text},
    next_table_occurrence, ordinal_labels,
};
use ordinal_labels::{OrdinalDisposition, SourceDestination};

pub(super) fn collect(root: &Node, spacing: &ordinal_labels::SourceContext) -> AstTopology {
    let mut topology = AstTopology::default();
    collect_ast_topology(
        root,
        false,
        None,
        &mut topology,
        spacing,
        &mut ColumnBodies::default(),
    );
    topology.lists.sort_by_key(|list| list.source_line);
    topology.equations.sort_by_key(|equation| {
        (
            equation.source_line,
            equation_context_order(equation.context),
        )
    });
    topology
}

pub(super) fn mdoc_list_items(node: &Node) -> impl Iterator<Item = &Node> {
    node.children
        .iter()
        .filter(|part| part.kind == NodeKind::Body)
        .flat_map(|body| &body.children)
        .filter(|child| child.macro_token.as_deref() == Some("It"))
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum MdocContainerKind {
    Generic,
    Definition,
    Table,
}

pub(super) fn mdoc_list_topology_kind(node: &Node) -> Option<MdocContainerKind> {
    if node.macro_token.as_deref() != Some("Bl") {
        return None;
    }
    Some(match node.list_kind {
        Some(NormalizedListKind::Column) => MdocContainerKind::Table,
        Some(NormalizedListKind::Definition) => MdocContainerKind::Definition,
        None if node.children.iter().any(|child| {
            child.kind == NodeKind::Body
                && child.children.iter().any(|item| {
                    item.macro_token.as_deref() == Some("It")
                        && item
                            .children
                            .iter()
                            .any(|part| part.kind == NodeKind::Head && !part.children.is_empty())
                })
        }) =>
        {
            MdocContainerKind::Definition
        }
        Some(
            NormalizedListKind::Bullet
            | NormalizedListKind::Dash
            | NormalizedListKind::Ordered
            | NormalizedListKind::Plain,
        )
        | None => MdocContainerKind::Generic,
    })
}

pub(super) fn column_data_items(node: &Node) -> impl Iterator<Item = &Node> {
    mdoc_list_items(node)
        .filter(|item| item.children.iter().any(|part| part.kind == NodeKind::Body))
}

fn mdoc_column_rows(node: &Node) -> impl Iterator<Item = AstTableRowTopology> {
    column_data_items(node)
        .enumerate()
        .map(|(row_index, item)| AstTableRowTopology {
            table_source_line: node.line,
            table_source_column: node.column,
            row_index,
            kind: mant_ir::TableRowKind::Data,
            cells: item
                .children
                .iter()
                .filter(|part| part.kind == NodeKind::Body)
                .map(|_| AstTableCellTopology {
                    column_span: 1,
                    row_span: 1,
                    vertical_continuation: false,
                })
                .collect(),
        })
}

#[derive(Default)]
struct ColumnBodies {
    occurrences: BTreeMap<(u32, u32), usize>,
    addresses: HashMap<u32, EquationCell>,
}

impl ColumnBodies {
    fn observe_table(&mut self, table: &Node) {
        let occurrence = next_table_occurrence(&mut self.occurrences, table.line, table.column);
        // mdoc_term.c::termp_it_pre walks preceding BODY siblings to
        // obtain the column. HEAD is not an additional cell.
        for (row_index, item) in column_data_items(table).enumerate() {
            for (cell_index, body) in item
                .children
                .iter()
                .filter(|part| part.kind == NodeKind::Body)
                .enumerate()
            {
                self.addresses.insert(
                    body.id,
                    EquationCell {
                        table_source_line: table.line,
                        table_source_column: table.column,
                        table_occurrence: occurrence,
                        row_index,
                        cell_index,
                    },
                );
            }
        }
    }
}

fn collect_ast_topology(
    node: &Node,
    inside_table: bool,
    cell: Option<EquationCell>,
    topology: &mut AstTopology,
    spacing: &ordinal_labels::SourceContext,
    column_bodies: &mut ColumnBodies,
) {
    let Some(destination) = spacing.destination(node) else {
        return;
    };
    let blocks = destination == SourceDestination::Blocks;
    let cell = column_bodies.addresses.remove(&node.id).or(cell);
    // CVS roff_parseln bypasses equation delimiters while tbl consumes a
    // cell. Only an owned Equation node establishes an obligation.
    if node.kind == NodeKind::Equation
        && let Some(equation) = node
            .equation
            .as_ref()
            .filter(|equation| equation_has_payload(equation))
    {
        topology.equations.push(AstEquationTopology {
            source_line: node.line,
            context: equation_context(node, inside_table, !blocks),
            value: equation_visible_text(&equation.readable_text()),
            cell,
        });
    }
    if blocks
        && node.kind == NodeKind::Block
        && mdoc_list_topology_kind(node) == Some(MdocContainerKind::Table)
        && node.line > 0
    {
        topology.table_rows.extend(mdoc_column_rows(node));
        column_bodies.observe_table(node);
    } else if blocks
        && node.kind == NodeKind::Block
        && let Some(kind) = mdoc_list_topology_kind(node)
        && node.line > 0
    {
        let ordinal_disposition = if kind == MdocContainerKind::Definition
            && node.definition_list_style == Some(libmandoc_rs::DefinitionListStyle::Tag)
        {
            ordinal_labels::observe(mdoc_list_items(node), spacing)
                .map(|labels| labels.disposition())
        } else {
            None
        };
        let kind = match (kind, ordinal_disposition) {
            (_, Some(OrdinalDisposition::Recovered)) | (MdocContainerKind::Generic, _) => {
                ListTopologyKind::Generic
            }
            (MdocContainerKind::Definition, _) => ListTopologyKind::Definition,
            (MdocContainerKind::Table, _) => unreachable!("column lists are tables"),
        };
        topology.lists.push(AstListTopology {
            source_line: node.line,
            source_column: node.column,
            kind,
            items: mdoc_list_items(node).count(),
            ordinal_uncertain: ordinal_disposition == Some(OrdinalDisposition::Uncertain),
        });
    }

    let mut table_origin = None;
    let mut row_index = 0;
    for child in &node.children {
        if blocks
            && child.kind == NodeKind::Table
            && spacing.destination(child) == Some(SourceDestination::Blocks)
            && let Some(kind) = native_table_kind(child)
        {
            if child.flags.table_start || table_origin.is_none() {
                table_origin = Some((child.line, child.column));
                row_index = 0;
                next_table_occurrence(&mut column_bodies.occurrences, child.line, child.column);
            }
            let (table_source_line, table_source_column) = table_origin.expect("native tbl start");
            topology.table_rows.push(AstTableRowTopology {
                table_source_line,
                table_source_column,
                row_index,
                kind,
                cells: child
                    .table_cells
                    .iter()
                    .map(|cell| AstTableCellTopology {
                        column_span: cell.column_span,
                        row_span: cell.row_span,
                        vertical_continuation: cell.vertical_continuation,
                    })
                    .collect(),
            });
            row_index += 1;
            collect_ast_topology(child, true, cell, topology, spacing, column_bodies);
            continue;
        }
        collect_ast_topology(
            child,
            inside_table || blocks && is_table_container(node),
            cell,
            topology,
            spacing,
            column_bodies,
        );
    }
}

pub(super) fn is_table_container(node: &Node) -> bool {
    node.kind == NodeKind::Table
        || node.kind == NodeKind::Block
            && mdoc_list_topology_kind(node) == Some(MdocContainerKind::Table)
}

pub(super) fn equation_context(
    node: &Node,
    inside_table: bool,
    inside_term: bool,
) -> EquationContext {
    // roff_eqndelim's executed origin is independent from roff_EQ's NODE_LINE.
    // Irrespective of the eqn delimiter mode, an executed definition HEAD
    // stores its value in the term and cannot own a stand-alone block.
    let display = !inside_term
        && node
            .equation_inline
            .map_or(node.flags.line_start, |inline| !inline);
    if display {
        EquationContext::Display
    } else if inside_table {
        EquationContext::TableCell
    } else {
        EquationContext::Inline
    }
}

fn native_table_kind(node: &Node) -> Option<mant_ir::TableRowKind> {
    use libmandoc_rs::{TableRowKind, TableRuleCellKind};
    node.table_row_kind.as_ref().map(|kind| match kind {
        TableRowKind::Data => mant_ir::TableRowKind::Data,
        TableRowKind::HorizontalRule => mant_ir::TableRowKind::HorizontalRule,
        TableRowKind::DoubleHorizontalRule => mant_ir::TableRowKind::DoubleHorizontalRule,
        TableRowKind::LayoutRule { cells } => mant_ir::TableRowKind::LayoutRule {
            cells: cells
                .iter()
                .map(|cell| match cell {
                    TableRuleCellKind::Horizontal => mant_ir::TableRuleCellKind::Horizontal,
                    TableRuleCellKind::DoubleHorizontal => {
                        mant_ir::TableRuleCellKind::DoubleHorizontal
                    }
                })
                .collect(),
        },
    })
}
