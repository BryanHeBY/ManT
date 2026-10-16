//! Width-independent table owners, occupied regions and physical-tail receipts.
use super::{Arc, LogicalLine};
use mant_ir::{TableAlignment, TableRuleCellKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::document) enum TableCellOccupancy {
    Content,
    Vacant,
    Covered { owner_column: usize },
    Continuation { owner_column: usize },
}
impl TableCellOccupancy {
    pub(in crate::document) fn owner_column(self, slot: usize) -> Option<usize> {
        match self {
            Self::Content => Some(slot),
            Self::Covered { owner_column } | Self::Continuation { owner_column } => {
                Some(owner_column)
            }
            Self::Vacant => None,
        }
    }
    pub(in crate::document) fn covered(self) -> bool {
        matches!(self, Self::Continuation { .. })
    }
}

#[derive(Debug, Default)]
pub(in crate::document) struct TableRails {
    pub(in crate::document) columns: bool,
    pub(in crate::document) outer: bool,
}

#[derive(Debug, Clone)]
pub(in crate::document) struct LogicalTableCell {
    pub(in crate::document) lines: Vec<LogicalLine>,
    pub(in crate::document) alignment: TableAlignment,
    pub(in crate::document) anchors: std::collections::HashMap<String, usize>,
    pub(in crate::document) completed_tail: bool,
    pub(in crate::document) break_after: bool,
    pub(in crate::document) empty_boundary: bool,
    pub(in crate::document) origin_bounds: mant_ir::geometry::CellOriginBounds,
    pub(in crate::document) column_span: usize,
    pub(in crate::document) row_span: u32,
    pub(in crate::document) occupancy: TableCellOccupancy,
    pub(in crate::document) preferred_width: Option<usize>,
}

#[derive(Debug, Clone)]
pub(in crate::document) struct LogicalTableRow {
    pub(in crate::document) cells: Vec<LogicalTableCell>,
    pub(in crate::document) rules: Option<Vec<TableRuleCellKind>>,
    pub(in crate::document) whole_rule: Option<bool>,
    pub(in crate::document) completed_tail: bool,
    pub(in crate::document) open_tail: bool,
    pub(in crate::document) layout: Arc<LogicalTableLayout>,
    pub(in crate::document) column_borders: Vec<mant_ir::TableColumnBorder>,
}

#[derive(Debug)]
pub(in crate::document) struct LogicalTableLayout {
    pub(in crate::document) preferred_widths: Vec<usize>,
    pub(in crate::document) alignment: Option<TableAlignment>,
    pub(in crate::document) force_stack: bool,
    /// Declared or actual-origin cells retain a provisional final row;
    /// generic measured projection retires that row even on bounded fallback.
    pub(in crate::document) retain_open_tail: bool,
    /// Complete source-neutral geometry, including content-derived gaps.
    pub(in crate::document) column_preferences: mant_ir::ColumnPreferences,
    pub(in crate::document) borders: mant_ir::TableBorders,
    pub(in crate::document) rails: TableRails,
    pub(in crate::document) logical_columns: usize,
}

impl LogicalTableLayout {
    #[cfg(test)]
    pub(in crate::document) fn for_rows(rows: &[Vec<LogicalTableCell>]) -> Self {
        Self::for_rows_with_gap(rows, 2)
    }

    pub(in crate::document) fn for_rows_with_gap(
        rows: &[Vec<LogicalTableCell>],
        gap: usize,
    ) -> Self {
        let column_count = rows
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0)
            .min(mant_ir::MAX_DENSE_TABLE_COLUMNS);
        let mut preferred_widths = vec![1; column_count];
        for row in rows {
            for (column, cell) in row
                .iter()
                .enumerate()
                .filter(|(_, cell)| cell.occupancy == TableCellOccupancy::Content)
            {
                let span = cell
                    .column_span
                    .min(column_count.saturating_sub(column))
                    .max(1);
                let target = cell.preferred_width.map_or_else(
                    || cell.preferred_width(),
                    |preferred| preferred.max(cell.longest_word_width()),
                );
                let content = target.saturating_sub(gap.saturating_mul(span.saturating_sub(1)));
                for (index, width) in preferred_widths
                    .iter_mut()
                    .skip(column)
                    .take(span)
                    .enumerate()
                {
                    *width =
                        (*width).max((content / span + usize::from(index < content % span)).max(1));
                }
            }
        }
        Self {
            preferred_widths,
            alignment: None,
            force_stack: false,
            retain_open_tail: false,
            column_preferences: mant_ir::ColumnPreferences::default(),
            borders: mant_ir::TableBorders::default(),
            rails: TableRails::default(),
            logical_columns: column_count,
        }
    }

    pub(in crate::document) fn requires_stack(&self, cells: &[LogicalTableCell]) -> bool {
        self.force_stack
            || (self.column_preferences.widths.is_empty()
                && cells
                    .iter()
                    .take(cells.len().saturating_sub(1))
                    .any(|cell| cell.break_after))
    }

    pub(super) fn preferred_width(&self) -> usize {
        self.preferred_widths.iter().sum::<usize>()
            + self.preferred_widths.len().saturating_sub(1)
                * (usize::from(self.column_preferences.gap_columns)
                    + usize::from(self.rails.columns))
            + usize::from(self.rails.outer) * 4
    }
}

impl LogicalTableCell {
    pub(in crate::document) fn has_open_tail(&self) -> bool {
        !self.completed_tail
            && self.lines.last().is_some_and(|line| {
                if let Some(row) = &line.table_row {
                    row.open_tail
                } else {
                    self.lines.len() > 1 && line.spans.iter().all(|span| span.content.is_empty())
                }
            })
    }

    pub(super) fn reading_line_count(&self) -> usize {
        self.lines
            .len()
            .saturating_sub(usize::from(self.has_open_tail()))
            .max(1)
    }

    pub(super) fn reading_completed_tail(&self) -> bool {
        self.completed_tail
            || self
                .lines
                .get(self.reading_line_count().saturating_sub(1))
                .is_some_and(|line| {
                    line.table_row.is_none()
                        && line.spans.iter().all(|span| span.content.is_empty())
                })
    }

    pub(in crate::document) fn new(
        lines: Vec<LogicalLine>,
        alignment: Option<TableAlignment>,
    ) -> Self {
        Self {
            lines,
            alignment: alignment.unwrap_or(TableAlignment::Left),
            anchors: std::collections::HashMap::new(),
            completed_tail: false,
            break_after: false,
            empty_boundary: false,
            origin_bounds: mant_ir::geometry::CellOriginBounds::default(),
            column_span: 1,
            row_span: 1,
            occupancy: TableCellOccupancy::Content,
            preferred_width: None,
        }
    }

    fn longest_word_width(&self) -> usize {
        self.lines
            .iter()
            .map(|line| {
                let text = line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>();
                text.split(|character: char| {
                    character.is_whitespace()
                        && !matches!(character, '\u{a0}' | '\u{2007}' | '\u{202f}')
                })
                .map(mant_ir::geometry::text_width)
                .max()
                .unwrap_or(0)
            })
            .max()
            .unwrap_or(0)
    }

    pub(in crate::document) fn preferred_width(&self) -> usize {
        self.lines
            .iter()
            .map(LogicalLine::preferred_width)
            .max()
            .unwrap_or(1)
            .max(1)
    }
}
