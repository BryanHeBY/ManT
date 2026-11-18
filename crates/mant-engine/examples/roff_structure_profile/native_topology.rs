//! Collect normalized shapes from the owned native execution tree.
use std::collections::HashSet;

use libmandoc_rs::{Node, NodeKind, NormalizedListKind};

use super::{
    AstEquationTopology, AstListTopology, AstTableCellTopology, AstTableRowTopology, AstTopology,
    EquationContext, ListTopologyKind, equation_context_order,
    native::{
        equation_visible_text, is_printable_no_fill_text, is_stateful_request, node_part_children,
        strip_tag_font_escapes,
    },
};

pub(super) fn collect(root: &Node, inline_table_rows: &HashSet<u32>) -> AstTopology {
    let mut topology = AstTopology::default();
    collect_ast_topology(root, false, &mut topology, inline_table_rows);
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
    inline_table_rows: &HashSet<u32>,
) {
    // CVS roff_parseln bypasses equation delimiters while tbl consumes a
    // cell. Only an owned Equation node establishes an obligation.
    if node.kind == NodeKind::Equation
        && let Some(value) = node
            .equation
            .as_ref()
            .map(libmandoc_rs::EquationBox::readable_text)
        && !value.is_empty()
    {
        topology.equations.push(AstEquationTopology {
            source_line: node.line,
            context: if inside_table {
                EquationContext::TableCell
            } else if equation_is_display(node) {
                EquationContext::Display
            } else {
                EquationContext::Inline
            },
            value: equation_visible_text(&value),
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
        let kind = if kind == MdocContainerKind::Definition
            && mdoc_definition_is_recoverable_ordinal_list(node)
        {
            // Lowering deliberately recovers explicit `1.`, `2.` tag heads
            // as an ordered generic list. The audit must compare semantic
            // topology, not the implementation-private native list subtype.
            ListTopologyKind::Generic
        } else {
            match kind {
                MdocContainerKind::Generic => ListTopologyKind::Generic,
                MdocContainerKind::Definition => ListTopologyKind::Definition,
                MdocContainerKind::Table => unreachable!("column lists are tables"),
            }
        };
        topology.lists.push(AstListTopology {
            source_line: node.line,
            kind,
            items: mdoc_list_items(node).count(),
        });
    }

    let mut table_origin = None;
    let mut row_index = 0;
    for child in &node.children {
        if child.kind == NodeKind::Table
            && !inline_table_rows.contains(&child.id)
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
            collect_ast_topology(child, true, topology, inline_table_rows);
            continue;
        }
        collect_ast_topology(
            child,
            inside_table || node.kind == NodeKind::Table,
            topology,
            inline_table_rows,
        );
    }
}

fn mdoc_definition_is_recoverable_ordinal_list(node: &Node) -> bool {
    if node.definition_list_style != Some(libmandoc_rs::DefinitionListStyle::Tag) {
        return false;
    }
    let mut previous: Option<(u64, OrdinalPunctuation)> = None;
    let mut count = 0usize;
    for item in mdoc_list_items(node) {
        if !node_part_children(item, NodeKind::Body)
            .iter()
            .any(has_ordinal_description)
        {
            return false;
        }
        let mut label = String::new();
        if !collect_ordinal_label(node_part_children(item, NodeKind::Head), &mut label) {
            return false;
        }
        let Some((number, punctuation)) = explicit_ordinal(&label) else {
            return false;
        };
        if previous.is_some_and(|(prior, mark)| {
            prior.checked_add(1) != Some(number) || mark != punctuation
        }) {
            return false;
        }
        previous = Some((number, punctuation));
        count += 1;
    }
    count > 1
}

fn has_ordinal_description(node: &Node) -> bool {
    if node.flags.no_print || node.kind == NodeKind::Comment || is_stateful_request(node) {
        return false;
    }
    // A spacing request may supply a layout-only description. Font and
    // spacing controls themselves, and a stray `\&`, supply no such owner.
    matches!(node.macro_token.as_deref(), Some("sp"))
        || matches!(node.kind, NodeKind::Equation | NodeKind::Table)
        || node.text.as_deref().is_some_and(is_printable_no_fill_text)
        || node.children.iter().any(has_ordinal_description)
}

fn collect_ordinal_label(nodes: &[Node], label: &mut String) -> bool {
    nodes.iter().all(|node| {
        if node.flags.no_print || node.kind == NodeKind::Comment {
            return true;
        }
        if node.kind == NodeKind::Text {
            label.push_str(&strip_tag_font_escapes(
                node.text.as_deref().unwrap_or_default(),
            ));
            return true;
        }
        // These pre-handlers apply presentation to their authored words.
        matches!(node.macro_token.as_deref(), Some("Sy" | "Em" | "Li" | "No"))
            && collect_ordinal_label(&node.children, label)
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OrdinalPunctuation {
    Period,
    Close,
    Paired,
}

fn explicit_ordinal(text: &str) -> Option<(u64, OrdinalPunctuation)> {
    let text = text.trim();
    let (number, punctuation) = if let Some(number) = text.strip_suffix('.') {
        (number, OrdinalPunctuation::Period)
    } else {
        let number = text.strip_suffix(')')?;
        number
            .strip_prefix('(')
            .map_or((number, OrdinalPunctuation::Close), |number| {
                (number, OrdinalPunctuation::Paired)
            })
    };
    Some((number.parse().ok()?, punctuation))
}

pub(super) fn equation_is_display(node: &Node) -> bool {
    // roff_eqndelim's executed origin is independent from roff_EQ's NODE_LINE.
    node.equation_inline
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
