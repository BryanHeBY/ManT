//! Collect normalized shapes from the owned native execution tree.
use libmandoc_rs::{Node, NodeKind, NormalizedListKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{
    AstEquationTopology, AstListTopology, AstTableCellTopology, AstTableRowTopology, AstTopology,
    EquationCell, EquationContext, ListTopologyKind, SourceOccurrence,
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
        SourceOccurrence::default(),
        &mut topology,
        spacing,
        &mut TopologyOwners::default(),
    );
    topology.lists.sort_by_key(|list| list.source_line);
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

fn mdoc_column_rows(
    node: &Node,
    table_occurrence: usize,
) -> impl Iterator<Item = AstTableRowTopology> {
    column_data_items(node)
        .enumerate()
        .map(move |(row_index, item)| AstTableRowTopology {
            table_source_line: node.line,
            table_source_column: node.column,
            table_occurrence,
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
struct TopologyOwners {
    occurrences: BTreeMap<(u32, u32), usize>,
    addresses: HashMap<u32, EquationCell>,
    sections: BTreeMap<u32, usize>,
    paragraphs: HashMap<u32, SourceOccurrence>,
    unproved_paragraphs: BTreeSet<u32>,
}

impl TopologyOwners {
    fn observe_table(&mut self, table: &Node) -> usize {
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
        occurrence
    }
}

/// Local provenance for simple prose directly inside a section.
struct ParagraphWitness {
    simple: bool,
    owner: Option<SourceOccurrence>,
    occurrences: BTreeMap<u32, usize>,
    unproved_lines: BTreeSet<u32>,
    uncertain_start: Option<u32>,
}

impl ParagraphWitness {
    fn observe(&mut self, children: &[Node], topology: &mut TopologyOwners) {
        for child in children {
            let token = child.macro_token.as_deref();
            if matches!(token, Some("Pp" | "PP" | "P" | "LP")) {
                if self.uncertain_start.take() == Some(child.line) {
                    // An earlier, unproved paragraph at this same source
                    // coordinate would shift the IR occurrence ordinal.
                    self.unproved_lines.insert(child.line);
                }
                self.simple = true;
                self.owner = None;
                // man_term.c::pre_PP enters the actual BODY. Its first
                // printed node supplies the same source identity as prose.
                if child.kind == NodeKind::Block
                    && let Some(body) = child
                        .children
                        .iter()
                        .find(|part| part.kind == NodeKind::Body)
                {
                    self.observe(&body.children, topology);
                    self.owner = None;
                }
                continue;
            }
            let prose = !child.flags.no_fill
                && (child.kind == NodeKind::Text && starts_prose(child)
                    || child.kind == NodeKind::Element
                        && matches!(
                            token,
                            Some("No" | "Em" | "Sy" | "Li" | "B" | "I" | "SB" | "SM")
                        )
                        && matches!(child.children.as_slice(), [text] if starts_prose(text)));
            let has_equation = child.equation.as_ref().is_some_and(equation_has_payload);
            if prose && self.simple {
                self.start(child.line);
            } else if child.kind == NodeKind::Equation {
                match child.equation_inline {
                    Some(false) if has_equation => {
                        self.simple = true;
                        self.owner = None;
                    }
                    Some(true) if has_equation && !child.flags.no_fill => {
                        if self.simple {
                            self.start(child.line);
                        }
                        if let Some(owner) = self
                            .owner
                            .filter(|owner| !self.unproved_lines.contains(&owner.line))
                        {
                            topology.paragraphs.insert(child.id, owner);
                        } else if self.unproved_lines.contains(&child.line) {
                            topology.unproved_paragraphs.insert(child.id);
                        }
                    }
                    _ => {}
                }
            } else if child.kind != NodeKind::Text || child.flags.line_start {
                if self.owner.is_none() && child.flags.line_start {
                    self.uncertain_start = Some(child.line);
                }
                self.simple = false;
                self.owner = None;
            }
        }
    }

    fn start(&mut self, line: u32) {
        self.owner
            .get_or_insert_with(|| SourceOccurrence::next(&mut self.occurrences, line));
    }
}

fn starts_prose(node: &Node) -> bool {
    node.kind == NodeKind::Text
        && node.text.as_deref().is_some_and(|text| {
            !text.trim_start().is_empty() && !text.trim_start().starts_with('\\')
        })
}

fn collect_ast_topology(
    node: &Node,
    inside_table: bool,
    cell: Option<EquationCell>,
    section_owner: SourceOccurrence,
    topology: &mut AstTopology,
    spacing: &ordinal_labels::SourceContext,
    owners: &mut TopologyOwners,
) {
    let Some(destination) = spacing.destination(node) else {
        return;
    };
    let blocks = destination == SourceDestination::Blocks;
    let cell = owners.addresses.remove(&node.id).or(cell);
    let section = matches!(node.macro_token.as_deref(), Some("Sh" | "Ss" | "SH" | "SS"));
    let paragraph = owners.paragraphs.remove(&node.id);
    let section_owner = if section && node.kind == NodeKind::Block {
        SourceOccurrence::next(&mut owners.sections, node.line)
    } else {
        section_owner
    };
    record_equation(
        node,
        inside_table,
        blocks,
        cell,
        (section_owner, paragraph),
        owners.unproved_paragraphs.remove(&node.id),
        &mut topology.equations,
    );
    if blocks && node.kind == NodeKind::Block && node.line > 0 {
        record_block_shape(node, topology, spacing, owners);
    }

    let mut table_origin = None;
    let mut row_index = 0;
    if section && node.kind == NodeKind::Body && blocks {
        ParagraphWitness {
            simple: true,
            owner: None,
            occurrences: BTreeMap::new(),
            unproved_lines: BTreeSet::new(),
            uncertain_start: None,
        }
        .observe(&node.children, owners);
    }
    for child in &node.children {
        if blocks
            && child.kind == NodeKind::Table
            && spacing.destination(child) == Some(SourceDestination::Blocks)
            && let Some(kind) = native_table_kind(child)
        {
            if child.flags.table_start || table_origin.is_none() {
                let occurrence =
                    next_table_occurrence(&mut owners.occurrences, child.line, child.column);
                table_origin = Some((child.line, child.column, occurrence));
                row_index = 0;
            }
            let (table_source_line, table_source_column, table_occurrence) =
                table_origin.expect("native tbl start");
            topology.table_rows.push(AstTableRowTopology {
                table_source_line,
                table_source_column,
                table_occurrence,
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
            collect_ast_topology(child, true, cell, section_owner, topology, spacing, owners);
            continue;
        }
        collect_ast_topology(
            child,
            inside_table || blocks && is_table_container(node),
            cell,
            section_owner,
            topology,
            spacing,
            owners,
        );
    }
}

fn record_block_shape(
    node: &Node,
    topology: &mut AstTopology,
    spacing: &ordinal_labels::SourceContext,
    owners: &mut TopologyOwners,
) {
    if mdoc_list_topology_kind(node) == Some(MdocContainerKind::Table) {
        let occurrence = owners.observe_table(node);
        topology
            .table_rows
            .extend(mdoc_column_rows(node, occurrence));
    } else if let Some(kind) = mdoc_list_topology_kind(node) {
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
}

fn record_equation(
    node: &Node,
    inside_table: bool,
    blocks: bool,
    cell: Option<EquationCell>,
    (section_owner, paragraph): (SourceOccurrence, Option<SourceOccurrence>),
    unproved_paragraph: bool,
    equations: &mut Vec<AstEquationTopology>,
) {
    // CVS roff_parseln bypasses equation delimiters while tbl consumes a
    // cell. Only an owned Equation node establishes an obligation.
    if node.kind == NodeKind::Equation
        && let Some(equation) = node
            .equation
            .as_ref()
            .filter(|equation| equation_has_payload(equation))
    {
        equations.push(AstEquationTopology {
            source_line: node.line,
            context: equation_context(node, inside_table, !blocks),
            value: equation_visible_text(&equation.readable_text()),
            cell,
            unproved_cell: inside_table && cell.is_none(),
            unproved_paragraph,
            section: section_owner,
            paragraph,
        });
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
