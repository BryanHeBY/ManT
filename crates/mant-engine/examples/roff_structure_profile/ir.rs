//! IR observations using the same structure counters.
use super::{
    Block, DefinitionOccurrence, Document, EquationCell, EquationContext, EquationOwner,
    FlowOccurrence, Inline, IrEquationTopology, IrListTopology, IrStructure, IrTableCellTopology,
    IrTableRowTopology, IrTopology, LinkTarget, ListTopologyKind, Section, SourceOccurrence,
};

pub(super) fn ir_profile(document: &Document) -> (IrStructure, IrTopology) {
    let mut profile = IrStructure::default();
    for diagnostic in &document.diagnostics {
        match diagnostic.code.as_deref() {
            Some("unresolved-section-reference") => {
                profile.unresolved_section_references += 1;
                profile.section_outcomes.record_diagnostic(diagnostic);
            }
            Some("ir.invalid-manual-reference") => profile.invalid_manual_diagnostics += 1,
            _ => {}
        }
    }
    let mut topology = IrTopology::default();
    collect_blocks(&document.blocks, None, None, &mut profile, &mut topology);
    for section in &document.sections {
        collect_section(section, &mut profile, &mut topology);
    }
    profile.literal_origins = super::literal_owners::record_ir_displays(document);
    topology.lists.sort_by_key(|list| list.source_line);
    (profile, topology)
}

fn collect_section(section: &Section, profile: &mut IrStructure, topology: &mut IrTopology) {
    topology.section = SourceOccurrence::next(
        &mut topology.section_occurrences,
        section.source.map_or(0, |source| source.line),
    );
    profile.section_outcomes.record_heading(section);
    collect_inlines(
        &section.heading.content,
        section.source.map_or(0, |source| source.line),
        None,
        None,
        profile,
        topology,
    );
    collect_blocks(&section.blocks, None, None, profile, topology);
    for child in &section.children {
        collect_section(child, profile, topology);
    }
}

fn collect_blocks(
    blocks: &[Block],
    cell: Option<EquationCell>,
    definition: Option<DefinitionOccurrence>,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    let mut paragraphs = std::collections::BTreeMap::new();
    let mut literals = std::collections::BTreeMap::new();
    for block in blocks {
        match block {
            Block::Paragraph {
                children,
                layout,
                source,
                ..
            } => {
                profile.paragraph_blocks += 1;
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                let owner = source.map(|span| {
                    EquationOwner::Paragraph(FlowOccurrence {
                        definition,
                        source: SourceOccurrence::next(&mut paragraphs, span.line),
                    })
                });
                collect_inlines(
                    children,
                    source.map_or(0, |span| span.line),
                    cell,
                    owner,
                    profile,
                    topology,
                );
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
                collect_display_equation(value, *display, layout, *source, cell, profile, topology);
            }
            Block::Preformatted {
                children,
                layout,
                source,
                ..
            } => {
                profile.preformatted_blocks += 1;
                profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
                if has_visible_inline(children) {
                    profile.preformatted_lines += 1;
                }
                profile.preformatted_lines += line_break_count(children);
                collect_inlines(
                    children,
                    source.map_or(0, |span| span.line),
                    cell,
                    Some(EquationOwner::Literal(source.map(|span| FlowOccurrence {
                        definition,
                        source: SourceOccurrence::next(&mut literals, span.line),
                    }))),
                    profile,
                    topology,
                );
            }
            Block::List {
                kind,
                items,
                layout,
                source,
                ..
            } => {
                collect_list(
                    kind,
                    items,
                    layout,
                    *source,
                    (cell, definition),
                    profile,
                    topology,
                );
            }
            Block::DefinitionList {
                items,
                layout,
                source,
                ..
            } => {
                collect_definitions(items, layout, *source, cell, profile, topology);
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

fn collect_list(
    kind: &mant_ir::ListKind,
    items: &[mant_ir::ListItem],
    layout: &mant_ir::LayoutHint,
    source: Option<mant_ir::SourceSpan>,
    (cell, definition): (Option<EquationCell>, Option<DefinitionOccurrence>),
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    profile.generic_list_items += items.len();
    record_list_layout(
        items.len(),
        layout.indent_columns,
        source,
        ListTopologyKind::Generic,
        matches!(kind, mant_ir::ListKind::Ordered { .. }),
        profile,
        topology,
    );
    for item in items {
        collect_blocks(&item.blocks, cell, definition, profile, topology);
    }
}

fn collect_definitions(
    items: &[mant_ir::DefinitionItem],
    layout: &mant_ir::LayoutHint,
    source: Option<mant_ir::SourceSpan>,
    cell: Option<EquationCell>,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    profile.definition_items += items.len();
    record_list_layout(
        items.len(),
        layout.indent_columns,
        source,
        ListTopologyKind::Definition,
        false,
        profile,
        topology,
    );
    for item in items {
        for term in &item.terms {
            collect_inlines(term, 0, cell, None, profile, topology);
        }
        let definition = item.source.map(|span| {
            DefinitionOccurrence::next(&mut topology.definition_occurrences, span.line, span.column)
        });
        collect_blocks(&item.description, cell, definition, profile, topology);
    }
}

fn collect_display_equation(
    value: &str,
    display: bool,
    layout: &mant_ir::LayoutHint,
    source: Option<mant_ir::SourceSpan>,
    cell: Option<EquationCell>,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
    if display {
        profile.display_equations += 1;
        topology.equations.push(IrEquationTopology {
            source_line: source.map_or(0, |span| span.line),
            context: EquationContext::Display,
            value: value.to_owned(),
            cell,
            section: topology.section,
            owner: None,
        });
    }
}

pub(super) fn has_visible_inline(inlines: &[Inline]) -> bool {
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
    cell: Option<EquationCell>,
    owner: Option<EquationOwner>,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    for inline in inlines {
        match inline {
            Inline::Strong { children } | Inline::Emphasis { children } => {
                collect_inlines(children, source_line, cell, owner, profile, topology);
            }
            Inline::Link {
                target, children, ..
            } => {
                match target {
                    LinkTarget::Manual { .. } if target.to_uri().is_none() => {
                        profile.invalid_manual_links.record_target(target);
                    }
                    LinkTarget::Manual { .. } => {
                        profile.manual_links += 1;
                        profile.known_links.record_target(target);
                    }
                    LinkTarget::External { .. } => profile.external_links += 1,
                    LinkTarget::Email { .. } => profile.email_links += 1,
                    LinkTarget::Section { id } => {
                        profile.section_links += 1;
                        profile.section_outcomes.record_resolved(id.as_str());
                    }
                    LinkTarget::Document { .. } => {}
                }
                if matches!(
                    target,
                    LinkTarget::Email { .. } | LinkTarget::External { .. }
                ) {
                    profile.known_links.record_target(target);
                }
                collect_inlines(children, source_line, cell, owner, profile, topology);
            }
            Inline::LineBreak { .. } => profile.hard_breaks += 1,
            Inline::Equation { value, .. } => {
                if cell.is_some() {
                    profile.table_equation_candidates += 1;
                } else {
                    profile.inline_equation_candidates += 1;
                }
                topology.equations.push(IrEquationTopology {
                    source_line,
                    context: if cell.is_some() {
                        EquationContext::TableCell
                    } else {
                        EquationContext::Inline
                    },
                    value: value.clone(),
                    cell,
                    section: topology.section,
                    owner,
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

fn collect_table(
    rows: &[mant_ir::TableRow],
    layout: &mant_ir::LayoutHint,
    source: Option<mant_ir::SourceSpan>,
    profile: &mut IrStructure,
    topology: &mut IrTopology,
) {
    profile.max_indent_columns = profile.max_indent_columns.max(layout.indent_columns);
    profile.table_rows += rows.len();
    let (line, column) = source.map_or((0, 0), |span| (span.line, span.column));
    let occurrence = super::next_table_occurrence(&mut topology.table_occurrences, line, column);
    topology
        .table_rows
        .extend(rows.iter().enumerate().map(|(row_index, row)| {
            IrTableRowTopology {
                table_source_line: line,
                table_source_column: column,
                table_occurrence: occurrence,
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
    for (row_index, row) in rows.iter().enumerate() {
        profile.table_spanning_cells += row
            .cells
            .iter()
            .filter(|cell| cell.column_span > 1 || cell.row_span > 1)
            .count();
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let address = EquationCell {
                table_source_line: line,
                table_source_column: column,
                table_occurrence: occurrence,
                row_index,
                cell_index,
            };
            collect_blocks(&cell.blocks, Some(address), None, profile, topology);
        }
    }
}

fn record_list_layout(
    items: usize,
    indent_columns: i32,
    source: Option<mant_ir::SourceSpan>,
    kind: ListTopologyKind,
    ordered: bool,
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
            source_column: source.column,
            kind,
            items,
            ordered,
        });
    }
}
