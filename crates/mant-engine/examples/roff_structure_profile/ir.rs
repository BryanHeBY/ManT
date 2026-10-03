//! IR observations using the same structure counters.
use super::{
    Block, Document, EquationContext, Inline, IrEquationTopology, IrListTopology, IrStructure,
    IrTableCellTopology, IrTableRowTopology, IrTopology, LinkTarget, ListTopologyKind, Section,
    equation_context_order,
};

pub(super) fn ir_profile(document: &Document) -> (IrStructure, IrTopology) {
    let mut profile = IrStructure {
        unresolved_section_references: document
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code.as_deref() == Some("unresolved-section-reference"))
            .count(),
        ..IrStructure::default()
    };
    let mut topology = IrTopology::default();
    collect_blocks(&document.blocks, false, &mut profile, &mut topology);
    for section in &document.sections {
        collect_section(section, &mut profile, &mut topology);
    }
    topology.lists.sort_by_key(|list| list.source_line);
    topology.equations.sort_by_key(|equation| {
        (
            equation.source_line,
            equation_context_order(equation.context),
        )
    });
    (profile, topology)
}

fn collect_section(section: &Section, profile: &mut IrStructure, topology: &mut IrTopology) {
    collect_inlines(
        &section.heading.content,
        section.source.map_or(0, |source| source.line),
        false,
        profile,
        topology,
    );
    collect_blocks(&section.blocks, false, profile, topology);
    for child in &section.children {
        collect_section(child, profile, topology);
    }
}

fn collect_blocks(
    blocks: &[Block],
    inside_table: bool,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    for block in blocks {
        match block {
            Block::Paragraph {
                children, layout, ..
            } => {
                profile.paragraph_blocks += 1;
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                collect_flow_inlines(block, children, inside_table, profile, topology);
            }
            Block::Unsupported { layout, .. } => {
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
            }
            Block::Equation {
                value,
                display,
                layout,
                source,
                ..
            } => {
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                if *display {
                    profile.display_equations += 1;
                    topology.equations.push(IrEquationTopology {
                        source_line: source.map_or(0, |span| span.line),
                        context: EquationContext::Display,
                        value: value.clone(),
                    });
                }
            }
            Block::Preformatted {
                children, layout, ..
            } => {
                profile.preformatted_blocks += 1;
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                if has_visible_inline(children) {
                    profile.preformatted_lines += 1;
                }
                profile.preformatted_lines += line_break_count(children);
                collect_flow_inlines(block, children, inside_table, profile, topology);
            }
            Block::List {
                items,
                layout,
                source,
                ..
            } => {
                profile.generic_list_items += items.len();
                record_list_layout(
                    items.len(),
                    layout.indent_columns,
                    *source,
                    ListTopologyKind::Generic,
                    profile,
                    topology,
                );
                for item in items {
                    collect_blocks(&item.blocks, inside_table, profile, topology);
                }
            }
            Block::DefinitionList {
                items,
                layout,
                source,
                ..
            } => {
                profile.definition_items += items.len();
                record_list_layout(
                    items.len(),
                    layout.indent_columns,
                    *source,
                    ListTopologyKind::Definition,
                    profile,
                    topology,
                );
                for item in items {
                    for term in &item.terms {
                        collect_inlines(term, 0, inside_table, profile, topology);
                    }
                    collect_blocks(&item.description, inside_table, profile, topology);
                }
            }
            Block::Table {
                rows,
                layout,
                source,
                ..
            } => {
                collect_table(rows, layout, *source, profile, topology);
            }
            Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => {}
        }
    }
}

fn has_visible_inline(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
            !value.is_empty()
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => has_visible_inline(children),
        Inline::Anchor { .. } | Inline::LineBreak { .. } => false,
    })
}

fn collect_inlines(
    inlines: &[Inline],
    source_line: u32,
    inside_table: bool,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    for inline in inlines {
        match inline {
            Inline::Strong { children } | Inline::Emphasis { children } => {
                collect_inlines(children, source_line, inside_table, profile, topology);
            }
            Inline::Link {
                target, children, ..
            } => {
                match target {
                    LinkTarget::Manual { .. } => profile.manual_links += 1,
                    LinkTarget::External { .. } => profile.external_links += 1,
                    LinkTarget::Email { .. } => profile.email_links += 1,
                    LinkTarget::Section { .. } => profile.section_links += 1,
                    LinkTarget::Document { .. } => {}
                }
                collect_inlines(children, source_line, inside_table, profile, topology);
            }
            Inline::LineBreak { .. } => profile.hard_breaks += 1,
            Inline::Equation { value, .. } => {
                if inside_table {
                    profile.table_equation_candidates += 1;
                } else {
                    profile.inline_equation_candidates += 1;
                }
                topology.equations.push(IrEquationTopology {
                    source_line,
                    context: if inside_table {
                        EquationContext::TableCell
                    } else {
                        EquationContext::Inline
                    },
                    value: value.clone(),
                });
            }
            Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } => {}
        }
    }
}

fn line_break_count(inlines: &[Inline]) -> usize {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => line_break_count(children),
            Inline::LineBreak { .. } => 1,
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::Anchor { .. } => 0,
        })
        .sum()
}

fn source_line(block: &Block) -> u32 {
    match block {
        Block::Paragraph { source, .. }
        | Block::Preformatted { source, .. }
        | Block::List { source, .. }
        | Block::DefinitionList { source, .. }
        | Block::Table { source, .. }
        | Block::Equation { source, .. }
        | Block::VerticalSpace { source, .. }
        | Block::ThematicBreak { source, .. }
        | Block::Unsupported { source, .. } => source.map_or(0, |span| span.line),
    }
}

fn collect_table(
    rows: &[mant_ir::TableRow],
    layout: &mant_ir::LayoutHint,
    source: Option<mant_ir::SourceSpan>,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
    profile.table_rows += rows.len();
    topology
        .table_rows
        .extend(rows.iter().enumerate().map(|(row_index, row)| {
            IrTableRowTopology {
                table_source_line: source.map_or(0, |origin| origin.line),
                table_source_column: source.map_or(0, |origin| origin.column),
                row_index,
                kind: row.kind.clone(),
                cells: row
                    .cells
                    .iter()
                    .map(|cell| IrTableCellTopology {
                        column_span: cell.column_span,
                        row_span: cell.row_span,
                        empty: cell.blocks.is_empty(),
                    })
                    .collect(),
            }
        }));
    for row in rows {
        profile.table_spanning_cells += row
            .cells
            .iter()
            .filter(|cell| cell.column_span > 1 || cell.row_span > 1)
            .count();
        for cell in &row.cells {
            collect_blocks(&cell.blocks, true, profile, topology);
        }
    }
}

fn record_list_layout(
    items: usize,
    indent_columns: i32,
    source: Option<mant_ir::SourceSpan>,
    kind: ListTopologyKind,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    profile.max_indent_columns = profile.max_indent_columns.max(indent_columns);
    if items != 0 {
        profile.max_indent_columns = profile.max_indent_columns.max(1);
    }
    if let Some(source) = source {
        topology.lists.push(IrListTopology {
            source_line: source.line,
            kind,
            items,
        });
    }
}

fn collect_flow_inlines(
    block: &Block,
    children: &[Inline],
    inside_table: bool,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    collect_inlines(
        children,
        source_line(block),
        inside_table,
        profile,
        topology,
    );
}
