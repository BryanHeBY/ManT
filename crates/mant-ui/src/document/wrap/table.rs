//! Dispatch table rows to declared, measured, rule, or stacked cell layout.
use super::{Line, LogicalTableRow, RowCopyMap, WrappedLine};

mod cells;
mod content;
mod declared;
mod rules;

use cells::stack_table_cells;
use content::{render_table_columns, table_column_widths};
use declared::render_declared_columns;
use rules::render_layout_rule;

pub(super) fn render_table_row_with_links(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
) -> Vec<WrappedLine> {
    if let Some(rules) = &table.rules {
        return render_layout_rule(indent, rules, &table.layout, width);
    }
    if table.cells.is_empty() {
        return vec![WrappedLine {
            source_end: None,
            anchors: Vec::new(),
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
            copy_map: RowCopyMap::default(),
        }];
    }
    let indent = super::readable_origins(indent, indent, width).0;
    let available = width.saturating_sub(indent).max(1);
    if table.layout.requires_stack(&table.cells) {
        return stack_table_cells(indent, table, width);
    }
    if !table.layout.column_preferences.widths.is_empty() {
        return render_declared_columns(indent, table, width, available);
    }
    let Some(column_widths) = table_column_widths(
        &table.layout.preferred_widths,
        available,
        usize::from(table.layout.column_preferences.gap_columns),
    ) else {
        return stack_table_cells(indent, table, width);
    };
    render_table_columns(
        indent,
        table,
        &column_widths,
        usize::from(table.layout.column_preferences.gap_columns),
        width,
    )
}
