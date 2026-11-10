//! Lower mdoc column fields into table cells without changing item execution.

use super::super::super::lower_scope_with_boundary;
use super::super::{
    AstTableAlignment, AstTableCell, Block, LoweringContext, Node, NodeKind, ScopeFlow, TableRow,
    first_part_children, layout, source_span, targets,
};
use super::{MdocListItem, attach_item_targets};

/// Preserve every body sibling of an mdoc `Bl -column` item as one table cell.
///
/// libmandoc represents `Ta` separators by creating several `Body` siblings
/// below the same `It` block. The usual term/body helper intentionally returns
/// only one structural part, so treating a column list as a definition list
/// silently discarded every cell after the first.
pub(super) fn lower_mdoc_column_list(
    node: &Node,
    items: Vec<MdocListItem<'_>>,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    cell_indent: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Block {
    let column_widths: Vec<u16> = node
        .columns
        .iter()
        .map(|declared| {
            u16::try_from(crate::mandoc::roff_escape::width_sample(declared)).unwrap_or(u16::MAX)
        })
        .collect();
    // Resolve the pinned mdoc_term.c::termp_it_pre widths, gap and extra-field
    // capacity here. term_ascii.c::ascii_advance/locale_advance also caps each
    // printed alignment region at 256 cells. That executed-device result is
    // exported as a generic layout preference, not a source-authored setting
    // or a consumer allocation budget.
    let gap = match column_widths.len() {
        n if n < 5 => 4,
        5 => 3,
        _ => 1,
    };
    let column_preferences = mant_ir::ColumnPreferences {
        widths: column_widths,
        gap_columns: gap,
        max_alignment_padding_columns: Some(256),
        extra_width_columns: Some(10),
    };
    let origins = mant_ir::geometry::DeclaredColumns::new(&column_preferences);
    formatter.execution.enter_column_output_scope();
    let rows = items
        .into_iter()
        .map(|item| {
            context.lower_inline_with_spacing(
                item.leading_controls,
                formatter.spacing_enabled(),
                formatter,
            );
            context.lower_inline_with_spacing(
                first_part_children(item.node, NodeKind::Head),
                formatter.spacing_enabled(),
                formatter,
            );
            let bodies = item
                .node
                .children
                .iter()
                .filter(|part| part.kind == NodeKind::Body && part.scope_end.is_none())
                .collect::<Vec<_>>();
            let mut cells = bodies
                .iter()
                .enumerate()
                .map(|(index, body)| {
                    let width = column_preferences.widths.get(index).map_or(10, |width| {
                        width.saturating_add(column_preferences.gap_columns)
                    });
                    let origin = origins.as_ref().map_or(0, |columns| columns.start(index));
                    let (blocks, break_after) = lower_scope_with_boundary(
                        &body.children,
                        context,
                        paragraph_distance,
                        formatter,
                        ScopeFlow::column_post(
                            cell_indent.content_origin(),
                            formatter.spacing_enabled(),
                            if index + 1 == bodies.len() {
                                u16::MAX
                            } else {
                                width
                            },
                            origin,
                            index + 1 == bodies.len(),
                        )
                        .with_column_nodes(
                            (index == 0).then_some(*body),
                            bodies.get(index + 1).copied(),
                        ),
                    );
                    AstTableCell {
                        preferred_width_columns: None,
                        kind: mant_ir::TableCellKind::Text,
                        blocks,
                        column_span: 1,
                        row_span: 1,
                        alignment: Some(AstTableAlignment::Left),
                        break_after,
                    }
                })
                .collect::<Vec<_>>();
            attach_column_item_targets(&mut cells, &item, cell_indent);
            TableRow {
                column_borders: vec![],
                kind: mant_ir::TableRowKind::Data,
                cells,
            }
        })
        .filter(|row| !row.cells.is_empty())
        .collect();
    formatter.execution.exit_column_output_scope();
    Block::Table {
        borders: mant_ir::TableBorders::default(),
        alignment: None,
        column_count: None,
        column_preferences,
        rows,
        layout: layout(indent_columns),
        source: source_span(node),
    }
}

fn attach_column_item_targets(
    cells: &mut Vec<AstTableCell>,
    item: &MdocListItem<'_>,
    indent: crate::mandoc::layout::SourceIndent,
) {
    if let Some(cell) = cells.first_mut() {
        attach_item_targets(&mut cell.blocks, item, layout(indent.content_origin()));
        return;
    }
    let mut blocks = Vec::new();
    attach_item_targets(&mut blocks, item, layout(indent.content_origin()));
    if !blocks.is_empty() {
        cells.push(AstTableCell {
            preferred_width_columns: None,
            break_after: false,
            kind: mant_ir::TableCellKind::Text,
            blocks,
            column_span: 1,
            row_span: 1,
            alignment: Some(AstTableAlignment::Left),
        });
    }
}

/// Attach targets after the final item, making a cell only when necessary.
pub(super) fn append_targets(
    rows: &mut Vec<TableRow>,
    targets: Vec<String>,
    layout: mant_ir::LayoutHint,
    source: Option<mant_ir::SourceSpan>,
) {
    if rows.is_empty() {
        rows.push(TableRow {
            column_borders: vec![],
            kind: mant_ir::TableRowKind::Data,
            cells: vec![AstTableCell {
                preferred_width_columns: None,
                break_after: false,
                kind: mant_ir::TableCellKind::Text,
                blocks: Vec::new(),
                column_span: 1,
                row_span: 1,
                alignment: Some(AstTableAlignment::Left),
            }],
        });
    }
    let cell = rows
        .last_mut()
        .and_then(|row| row.cells.last_mut())
        .expect("table cell inserted");
    targets::append_targets(&mut cell.blocks, targets, layout, source);
}
