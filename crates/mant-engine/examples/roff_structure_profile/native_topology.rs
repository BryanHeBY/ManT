//! Collect normalized shapes from the owned native execution tree.
use libmandoc_rs::{Node, NodeKind, NormalizedListKind};

use super::{
    AstEquationTopology, AstListTopology, AstTableCellTopology, AstTableRowTopology, AstTopology,
    EquationContext, ListTopologyKind, equation_context_order,
    head_payloads::HeadPayloads,
    native::{equation_has_payload, equation_visible_text},
    ordinal_labels,
};
use ordinal_labels::OrdinalDisposition;

pub(super) fn collect(root: &Node, head_payloads: &HeadPayloads) -> AstTopology {
    let mut topology = AstTopology::default();
    let spacing = ordinal_labels::HeadSpacing::collect(root);
    collect_ast_topology(root, false, &mut topology, head_payloads, &spacing);
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

fn collect_ast_topology(
    node: &Node,
    inside_table: bool,
    topology: &mut AstTopology,
    head_payloads: &HeadPayloads,
    spacing: &ordinal_labels::HeadSpacing,
) {
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
            context: if inside_table {
                EquationContext::TableCell
            } else if equation_is_display(node, head_payloads) {
                EquationContext::Display
            } else {
                EquationContext::Inline
            },
            value: equation_visible_text(&equation.readable_text()),
        });
    }
    if node.kind == NodeKind::Block
        && mdoc_list_topology_kind(node) == Some(MdocContainerKind::Table)
        && node.line > 0
    {
        topology.table_rows.extend(mdoc_column_rows(node));
    } else if node.kind == NodeKind::Block
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
        if child.kind == NodeKind::Table
            && !head_payloads.row_ids.contains(&child.id)
            && let Some(kind) = native_table_kind(child)
        {
            if child.flags.table_start || table_origin.is_none() {
                table_origin = Some((child.line, child.column));
                row_index = 0;
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
            collect_ast_topology(child, true, topology, head_payloads, spacing);
            continue;
        }
        collect_ast_topology(
            child,
            inside_table || node.kind == NodeKind::Table,
            topology,
            head_payloads,
            spacing,
        );
    }
}

pub(super) fn equation_is_display(node: &Node, head_payloads: &HeadPayloads) -> bool {
    // roff_eqndelim's executed origin is independent from roff_EQ's NODE_LINE.
    // Irrespective of the eqn delimiter mode, an executed definition HEAD
    // stores its value in the term and cannot own a stand-alone block.
    !head_payloads.contains_equation(node)
        && node
            .equation_inline
            .map_or(node.flags.line_start, |inline| !inline)
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
