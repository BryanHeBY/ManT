//! Bound derived row-edge copies without limiting table bodies or topology.
use std::{collections::HashMap, sync::Arc};

use libmandoc_rs::Node;
use mant_ir::{TableBorderStyle, TableBorders, TableColumnBorder};

const MAX_DERIVED_COLUMN_EDGES: usize = 65_536;

#[derive(Default)]
pub(super) struct TableLayoutBudget {
    layouts: HashMap<usize, Arc<[TableColumnBorder]>>,
    emitted_edges: usize,
    exhausted: bool,
}

impl TableLayoutBudget {
    pub(super) fn exhausted(&self) -> bool {
        self.exhausted
    }

    pub(super) fn column_borders(&mut self, node: &Node) -> Vec<TableColumnBorder> {
        let Some(layout) = &node.table_layout else {
            return Vec::new();
        };
        if self.exhausted {
            return Vec::new();
        }
        let key = Arc::as_ptr(layout) as usize;
        let remaining = MAX_DERIVED_COLUMN_EDGES.saturating_sub(self.emitted_edges);
        let edges = if let Some(edges) = self.layouts.get(&key) {
            Arc::clone(edges)
        } else {
            let defaults = table_borders(node);
            let count = node.table_column_count.unwrap_or_default() as usize;
            // Native row.vert and tbl_cell.vert describe logical boundaries,
            // not printed characters. Cache each declaration once, including
            // declarations with no edges, instead of rescanning N slots for
            // every one of M data rows sharing that native layout identity.
            let mut edges = Vec::new();
            for (column, style) in layout
                .left_border
                .map(|style| (0, style))
                .into_iter()
                .chain(layout.cells.iter().filter_map(|cell| {
                    cell.right_border
                        .map(|style| (cell.column.saturating_add(1), style))
                }))
            {
                let style = border_style(style);
                if defaults
                    .column_style(column as usize, count)
                    .is_some_and(|default| default.stronger(style) == default)
                {
                    continue;
                }
                if edges.len() == remaining {
                    self.exhausted = true;
                    return Vec::new();
                }
                edges.push(TableColumnBorder { column, style });
            }
            let edges: Arc<[TableColumnBorder]> = edges.into();
            self.layouts.insert(key, Arc::clone(&edges));
            edges
        };
        if edges.len() > remaining {
            self.exhausted = true;
            return Vec::new();
        }
        self.emitted_edges += edges.len();
        edges.to_vec()
    }
}

pub(super) const fn border_style(style: libmandoc_rs::TableBorderStyle) -> TableBorderStyle {
    match style {
        libmandoc_rs::TableBorderStyle::Single => TableBorderStyle::Single,
        libmandoc_rs::TableBorderStyle::Double => TableBorderStyle::Double,
    }
}

pub(super) fn table_borders(node: &Node) -> TableBorders {
    TableBorders {
        outer: node.table_frame.map(border_style),
        inner_rows: node.table_grid.map(border_style),
        inner_columns: node.table_grid.map(border_style),
    }
}
