//! One bounded set of rail slots shared by rules, data and fallback boxes.
use crate::table_border::{crossing, frame_corner, horizontal_glyph, junction, vertical_glyph};
use mant_ir::{
    TableBorderStyle as Stroke, TableBorders, TableRow, TableRowKind, TableRuleCellKind,
};
#[derive(Clone, Copy)]
pub(super) struct Frame<'a> {
    borders: &'a TableBorders,
    rows: &'a [TableRow],
    count: usize,
    pub(super) gap: usize,
    outer: bool,
    internal: bool,
    legacy: bool,
}
impl<'a> Frame<'a> {
    pub(super) fn new(
        rows: &'a [TableRow],
        borders: &'a TableBorders,
        count: usize,
        gap: usize,
    ) -> Self {
        let outer = borders.outer.is_some()
            || rows
                .iter()
                .flat_map(|row| &row.column_borders)
                .any(|edge| edge.column == 0 || usize::try_from(edge.column).ok() == Some(count));
        let internal = borders.inner_columns.is_some()
            || rows.iter().flat_map(|row| &row.column_borders).any(|edge| {
                edge.column > 0 && usize::try_from(edge.column).is_ok_and(|column| column < count)
            });
        let legacy = borders.is_empty() && rows.iter().all(|row| row.column_borders.is_empty());
        Self {
            borders,
            rows,
            count,
            gap: gap.saturating_add(usize::from(internal || legacy)),
            outer,
            internal,
            legacy,
        }
    }
    pub(super) fn has_edges(&self) -> bool {
        !self.legacy
    }
    pub(super) fn has_outer(&self) -> bool {
        self.outer
    }
    pub(super) fn stacked_spans(&self) -> Option<super::spans::Spans> {
        // Wide sparse tables deliberately simplify covered-column geometry;
        // never allocate a slot for every declared column during fallback.
        (self.count <= mant_ir::MAX_DENSE_TABLE_COLUMNS)
            .then(|| super::spans::Spans::new(self.count))
    }
    pub(super) fn stacked_boundary(
        &self,
        spans: Option<&super::spans::Spans>,
        next: &TableRow,
    ) -> Option<TableRuleCellKind> {
        if !next.cells.is_empty() && spans.is_some_and(super::spans::Spans::fully_continued) {
            return None;
        }
        self.borders.inner_rows.map(|stroke| match stroke {
            Stroke::Single => TableRuleCellKind::Horizontal,
            Stroke::Double => TableRuleCellKind::DoubleHorizontal,
        })
    }
    pub(super) fn outside(&self) -> usize {
        if self.outer { 4 } else { 0 }
    }
    pub(super) fn left(&self) -> usize {
        if self.outer { 2 } else { 0 }
    }
    pub(super) fn left_rail(&self, row: &TableRow) -> String {
        if !self.outer {
            return String::new();
        }
        format!(
            "{} ",
            vertical(row.column_border_style(self.borders, 0, self.count))
        )
    }
    pub(super) fn right_rail(&self, row: &TableRow) -> String {
        if !self.outer {
            return String::new();
        }
        format!(
            " {}",
            vertical(row.column_border_style(self.borders, self.count, self.count))
        )
    }
    pub(super) fn divider(&self, row: &TableRow, column: usize, covered: bool) -> String {
        if covered {
            return " ".repeat(self.gap);
        }
        let edge = row.column_border_style(self.borders, column, self.count);
        self.gap_text(if self.legacy { '|' } else { vertical(edge) })
    }
    fn gap_text(&self, center: char) -> String {
        if self.internal || self.legacy {
            let blank = self.gap.saturating_sub(1);
            format!(
                "{}{center}{}",
                " ".repeat(blank / 2),
                " ".repeat(blank - blank / 2)
            )
        } else {
            " ".repeat(self.gap)
        }
    }
    pub(super) fn stack_left(&self, source: Option<&TableRow>) -> String {
        format!("{} ", vertical(self.column_style(source, 0)))
    }
    pub(super) fn stack_right(&self, source: Option<&TableRow>) -> String {
        format!(" {}", vertical(self.column_style(source, self.count)))
    }
    fn column_style(&self, source: Option<&TableRow>, column: usize) -> Option<Stroke> {
        source.map_or_else(
            || self.borders.column_style(column, self.count),
            |row| row.column_border_style(self.borders, column, self.count),
        )
    }
    pub(super) fn horizontal(
        &self,
        columns: &[usize],
        stroke: Option<Stroke>,
        top: bool,
        bottom: bool,
        source: Option<&TableRow>,
    ) -> Option<String> {
        let stroke = stroke.or(self.borders.outer)?;
        let mut physical = self
            .rows
            .iter()
            .filter(|row| !mant_ir::table_row_is_navigation_only(row));
        let source = source.or(if top {
            physical.next()
        } else if bottom {
            physical.next_back()
        } else {
            None
        });
        let horizontal = if self.legacy {
            match stroke {
                Stroke::Single => '-',
                Stroke::Double => '=',
            }
        } else {
            horizontal_glyph(stroke)
        };
        let mut output = String::new();
        if self.outer {
            let edge = self.column_style(source, 0);
            output.push(edge.map_or(' ', |edge| frame_corner(stroke, edge, true, top, bottom)));
            output.push(if edge.is_some() { horizontal } else { ' ' });
        }
        for (index, width) in columns.iter().enumerate() {
            output.extend(std::iter::repeat_n(horizontal, *width));
            if index + 1 < columns.len() {
                let edge = source
                    .or(if top {
                        self.rows.first()
                    } else if bottom {
                        self.rows.last()
                    } else {
                        None
                    })
                    .map_or(self.borders.inner_columns, |row| {
                        row.column_border_style(self.borders, index + 1, self.count)
                    });
                let covered = source.is_some_and(|row| covers(row, index + 1));
                if self.internal
                    && !covered
                    && let Some(edge) = edge
                {
                    let blank = self.gap.saturating_sub(1);
                    output.extend(std::iter::repeat_n(horizontal, blank / 2));
                    output.push(crossing(stroke, edge, top, bottom));
                    output.extend(std::iter::repeat_n(horizontal, blank - blank / 2));
                } else {
                    output.extend(std::iter::repeat_n(horizontal, self.gap));
                }
            }
        }
        if self.outer {
            let edge = self.column_style(source, self.count);
            output.push(if edge.is_some() { horizontal } else { ' ' });
            output.push(edge.map_or(' ', |edge| frame_corner(stroke, edge, false, top, bottom)));
        }
        Some(output)
    }
    pub(super) fn bottom(&self, columns: &[usize], spans: &super::spans::Spans) -> Option<String> {
        let mut chars = self
            .horizontal(columns, None, false, true, None)?
            .chars()
            .collect::<Vec<_>>();
        let mut offset = self.left();
        for (column, width) in columns.iter().enumerate() {
            offset += width;
            if column + 1 < columns.len() {
                if spans.interior(column + 1) && self.internal {
                    chars[offset + self.gap.saturating_sub(1) / 2] =
                        self.borders.outer.map_or(' ', horizontal_glyph);
                }
                offset += self.gap;
            }
        }
        Some(chars.into_iter().collect())
    }
    pub(super) fn between(
        &self,
        columns: &[usize],
        spans: &super::spans::Spans,
        source: &TableRow,
        next: &TableRow,
    ) -> Option<String> {
        if !next.cells.is_empty() && spans.fully_continued() {
            return None;
        }
        let stroke = self.borders.inner_rows?;
        let value = self.horizontal(columns, Some(stroke), false, false, Some(source))?;
        // An empty data row is an authored physical row. Vertical
        // continuation instead uses explicit empty cells in the next row.
        if next.cells.is_empty() {
            return Some(value);
        }
        Some(self.mask_rule(&value, columns, &spans.rules(), spans, Some(source)))
    }
    pub(super) fn mask_rule(
        &self,
        value: &str,
        columns: &[usize],
        covered: &[bool],
        spans: &super::spans::Spans,
        source: Option<&TableRow>,
    ) -> String {
        let mut chars = value.chars().collect::<Vec<_>>();
        let mut at = self.left();
        let strokes = columns
            .iter()
            .map(|width| {
                let stroke = match chars.get(at) {
                    Some('═' | '=') => Some(Stroke::Double),
                    Some('─' | '-') => Some(Stroke::Single),
                    _ => None,
                };
                at += width + self.gap;
                stroke
            })
            .collect::<Vec<_>>();
        if self.outer && covered.first() == Some(&true) {
            chars[0] = vertical(self.column_style(source, 0));
            chars[1] = ' ';
        }
        if self.outer && covered.last() == Some(&true) {
            let length = chars.len();
            chars[length - 1] = vertical(self.column_style(source, self.count));
            chars[length - 2] = ' ';
        }
        let mut offset = self.left();
        for (column, width) in columns.iter().enumerate() {
            if covered[column] {
                chars[offset..offset + width].fill(' ');
            }
            offset += width;
            if column + 1 < columns.len() {
                let left = (!covered[column]).then_some(strokes[column]).flatten();
                let right = (!covered[column + 1])
                    .then_some(strokes[column + 1])
                    .flatten();
                let center = self
                    .gap
                    .saturating_sub(usize::from(self.internal || self.legacy))
                    / 2;
                for (index, target) in chars[offset..offset + self.gap].iter_mut().enumerate() {
                    if index < center && covered[column] || index >= center && covered[column + 1] {
                        *target = ' ';
                    }
                }
                if self.internal {
                    let vertical = (!spans.interior(column + 1))
                        .then(|| {
                            source.map_or(self.borders.inner_columns, |row| {
                                row.column_border_style(self.borders, column + 1, columns.len())
                            })
                        })
                        .flatten();
                    chars[offset + center] = junction(left, right, vertical, false, false);
                }
                offset += self.gap;
            }
        }
        chars.into_iter().collect()
    }
    pub(super) fn fallback_rule(
        &self,
        cells: &[TableRuleCellKind],
        width: usize,
        source: Option<&TableRow>,
    ) -> String {
        let style = |cell: &TableRuleCellKind| match cell {
            TableRuleCellKind::Horizontal => Stroke::Single,
            TableRuleCellKind::DoubleHorizontal => Stroke::Double,
        };
        let mut value = String::new();
        let Some(first) = cells.first().map(style) else {
            return value;
        };
        if self.outer {
            value.push(
                self.column_style(source, 0)
                    .map_or(' ', |outer| frame_corner(first, outer, true, false, false)),
            );
            value.push(horizontal_glyph(first));
        }
        for (index, cell) in cells.iter().enumerate() {
            value.extend(std::iter::repeat_n(
                horizontal_glyph(style(cell)),
                width / cells.len() + usize::from(index < width % cells.len()),
            ));
        }
        if self.outer {
            let last = cells.last().map_or(first, style);
            value.push(horizontal_glyph(last));
            value.push(
                self.column_style(source, self.count)
                    .map_or(' ', |outer| frame_corner(last, outer, false, false, false)),
            );
        }
        value
    }
    pub(super) fn rule(
        &self,
        columns: &[usize],
        kind: &TableRowKind,
        source: Option<&TableRow>,
    ) -> Option<String> {
        match kind {
            TableRowKind::HorizontalRule => {
                self.horizontal(columns, Some(Stroke::Single), false, false, source)
            }
            TableRowKind::DoubleHorizontalRule => {
                self.horizontal(columns, Some(Stroke::Double), false, false, source)
            }
            TableRowKind::LayoutRule { cells } => {
                let mut output = String::new();
                if self.outer {
                    output.push(vertical(self.column_style(source, 0)));
                    output.push(' ');
                }
                for (column, width) in columns.iter().enumerate() {
                    let glyph = match cells.get(column) {
                        Some(TableRuleCellKind::Horizontal) => {
                            if self.legacy {
                                '-'
                            } else {
                                '─'
                            }
                        }
                        Some(TableRuleCellKind::DoubleHorizontal) => {
                            if self.legacy {
                                '='
                            } else {
                                '═'
                            }
                        }
                        None => ' ',
                    };
                    output.extend(std::iter::repeat_n(glyph, *width));
                    if column + 1 < columns.len() {
                        output.push_str(&self.gap_text(if self.legacy {
                            '|'
                        } else {
                            vertical(source.map_or(self.borders.inner_columns, |row| {
                                row.column_border_style(self.borders, column + 1, columns.len())
                            }))
                        }));
                    }
                }
                if self.outer {
                    output.push(' ');
                    output.push(vertical(self.column_style(source, self.count)));
                }
                Some(output)
            }
            TableRowKind::Data => None,
        }
    }
}
fn vertical(style: Option<Stroke>) -> char {
    style.map_or(' ', vertical_glyph)
}
fn covers(row: &TableRow, boundary: usize) -> bool {
    let mut column = 0_usize;
    row.cells.iter().any(|cell| {
        let end = column.saturating_add(usize::try_from(cell.column_span).unwrap_or(usize::MAX));
        let covered = column < boundary && boundary < end;
        column = end;
        covered
    })
}
