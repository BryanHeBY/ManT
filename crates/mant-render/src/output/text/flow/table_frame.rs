//! Final table placement retains physical row and gap receipts. A rule uses
//! the same chosen region as data, including after a source-order fallback.
use super::{Flow, LayoutText, Part};
use mant_ir::{TableAlignment, TableRuleCellKind, geometry};

pub(super) struct TableRule {
    cells: Vec<TableRuleCellKind>,
    gap: u16,
    widths: Option<Vec<usize>>,
    divider: bool,
}

impl TableRule {
    pub(super) fn render_default(&self) -> LayoutText {
        self.render(self.minimum_width())
    }

    fn minimum_width(&self) -> usize {
        let widths = self.widths.as_deref();
        if widths.is_none()
            && (usize::from(self.gap) + usize::from(self.divider))
                .saturating_mul(self.cells.len().saturating_sub(1))
                > geometry::MAX_COLUMN_PADDING
        {
            return if self.cells.is_empty() { 0 } else { 3 };
        }
        let columns = widths.map_or(self.cells.len(), <[usize]>::len);
        let content = widths.map_or_else(
            || 3_usize.saturating_mul(columns),
            |widths| widths.iter().sum(),
        );
        content.saturating_add(
            (usize::from(self.gap) + usize::from(self.divider))
                .saturating_mul(columns.saturating_sub(1)),
        )
    }

    fn render(&self, width: usize) -> LayoutText {
        if self.cells.is_empty() {
            return LayoutText::default();
        }
        let gap = usize::from(self.gap);
        let gap_width = gap.min(geometry::MAX_COLUMN_PADDING);
        let separator = if self.divider {
            format!(
                "{}|{}",
                " ".repeat(gap_width / 2),
                " ".repeat(gap_width - gap_width / 2)
            )
        } else {
            " ".repeat(gap_width)
        };
        if let Some(widths) = self.widths.as_deref().filter(|widths| {
            widths.len() <= mant_ir::MAX_DENSE_TABLE_COLUMNS
                && widths.len() >= self.cells.len()
                && self.minimum_width() <= width
        }) {
            let extra = width.saturating_sub(self.minimum_width());
            return LayoutText::join(
                widths.iter().enumerate().map(|(column, cells)| {
                    let cells =
                        cells.saturating_add(if column + 1 == widths.len() { extra } else { 0 });
                    let character = self.cells.get(column).map_or(' ', |kind| match kind {
                        TableRuleCellKind::Horizontal => '-',
                        TableRuleCellKind::DoubleHorizontal => '=',
                    });
                    LayoutText::from(character.to_string().repeat(cells))
                }),
                &separator,
            );
        }
        if self.cells.len() == 1 {
            return LayoutText::from(
                match self.cells[0] {
                    TableRuleCellKind::Horizontal => "-",
                    TableRuleCellKind::DoubleHorizontal => "=",
                }
                .repeat(width),
            );
        }
        // Stacked/sparse rules retain their actual strengths in source order;
        // a missing column never turns a mixed row into one whole-row rule.
        let parts = self.cells.iter().map(|kind| {
            LayoutText::from(match kind {
                TableRuleCellKind::Horizontal => "---",
                TableRuleCellKind::DoubleHorizontal => "===",
            })
        });
        LayoutText::join(
            parts,
            if (gap + usize::from(self.divider)).saturating_mul(self.cells.len().saturating_sub(1))
                > geometry::MAX_COLUMN_PADDING
            {
                "\n"
            } else {
                &separator
            },
        )
    }
}

impl Flow {
    pub(in crate::output::text) fn push_table_rule(
        &mut self,
        cells: Vec<TableRuleCellKind>,
        gap: u16,
        widths: Option<Vec<usize>>,
        divider: bool,
    ) {
        self.parts.push(Part::TableRule(TableRule {
            cells,
            gap,
            widths,
            divider,
        }));
    }

    /// Whole-table placement only translates already prepared rows. Empty
    /// rows and unresolved vertical gaps remain unchanged; it never copies or
    /// reexecutes a source cell, and generated spaces stay outside decoration.
    pub(in crate::output::text) fn position_table(
        mut self,
        alignment: Option<TableAlignment>,
        viewport: Option<usize>,
        origin: i32,
        minimum_width: Option<usize>,
    ) -> Self {
        let parent = geometry::padding(origin);
        let width = self
            .parts
            .iter()
            .fold(minimum_width.unwrap_or(0), |width, part| {
                width.max(match part {
                    Part::Text(text) | Part::CompletedText(text) | Part::Literal(text) => text
                        .visible
                        .lines()
                        .map(|row| geometry::text_width(row).saturating_sub(parent))
                        .max()
                        .unwrap_or(0),
                    Part::TableRule(rule) => rule.minimum_width(),
                    Part::Gap(_) | Part::CompleteGap => 0,
                })
            });
        let offset = viewport
            .filter(|viewport| parent.saturating_add(width) <= *viewport)
            .map_or(0, |viewport| {
                let spare = viewport.saturating_sub(parent).saturating_sub(width);
                match alignment {
                    Some(TableAlignment::Center) => spare / 2,
                    Some(TableAlignment::Right) => spare,
                    _ => 0,
                }
            });
        let offset = if parent.saturating_add(offset).saturating_add(width)
            <= geometry::MAX_COLUMN_PADDING
        {
            offset
        } else {
            0
        };
        for part in &mut self.parts {
            match part {
                Part::Text(text) | Part::CompletedText(text) | Part::Literal(text) => {
                    *text = std::mem::take(text).indented(offset);
                }
                Part::TableRule(rule) => {
                    *part = Part::Text(
                        rule.render(width.min(geometry::MAX_COLUMN_PADDING))
                            .indented(parent.saturating_add(offset)),
                    );
                }
                Part::Gap(_) | Part::CompleteGap => {}
            }
        }
        self
    }
}
