//! Bounded placement of declared table fields, shared by terminal consumers.

/// Maximum generated padding in one physical row before source-order fallback.
/// This bounds output work independently of any source device advance limit.
pub const MAX_COLUMN_PADDING: usize = 4096;
/// Dense declared placement is bounded independently of inbound IR lengths.
pub const MAX_DECLARED_COLUMNS: usize = 256;

/// Extent used to decide whether the next declared field can stay on this
/// row. CVS `term_fill()` records the last graph before ordinary trailing
/// spaces; they remain pending padding in `term_field()`. Fixed spaces,
/// tabs and every other glyph retain their extent. Decoration is removed
/// before calling this helper, and the original output is kept separately.
#[must_use]
pub fn declared_field_width(text: &str) -> usize {
    super::text_width(text.trim_end_matches(' '))
}

/// Accepted graph extent decides field wrapping, complete output extent
/// keeps glyph coordinates monotonic, and completed rows cannot be reused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnFieldWidth {
    /// Last accepted content column, excluding pending separator padding.
    pub content: usize,
    /// Width of every preserved output glyph, including pending padding.
    pub output: usize,
    /// This physical row is already complete, so the next cell cannot
    /// contribute content to it. Resolved trailing gap rows carry this
    /// fact independently of their zero glyph width.
    pub completed: bool,
}

impl ColumnFieldWidth {
    /// Measure one undecorated row without discarding its source text.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self {
            content: declared_field_width(text),
            output: super::text_width(text),
            completed: false,
        }
    }
}

/// Measured declaration widths, excluding the inter-column gap. These are
/// display cells of the producer's reading device, not fixed viewport widths.
#[derive(Clone, Debug)]
pub struct DeclaredColumns {
    starts: Vec<usize>,
    undeclared_start: usize,
    ends: Vec<usize>,
    extra_width: Option<usize>,
    advance_limit: Option<usize>,
}

/// One cell-local physical row and its final column in the reading projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnPiece {
    /// Source-order cell index.
    pub cell: usize,
    /// Cell-local hard row index.
    pub line: usize,
    /// Display-cell start, independent of decoration and source coordinates.
    pub column: usize,
}

impl DeclaredColumns {
    /// Build bounded preferred starts from explicit source-neutral preferences.
    #[must_use]
    pub fn new(preferences: &crate::ColumnPreferences) -> Option<Self> {
        let widths = &preferences.widths;
        if widths.is_empty() || widths.len() > MAX_DECLARED_COLUMNS {
            return None;
        }
        let gap = usize::from(preferences.gap_columns);
        let mut offset = 0_usize;
        let mut ends = Vec::with_capacity(widths.len());
        let starts = widths
            .iter()
            .map(|width| {
                let start = offset;
                offset = offset
                    .saturating_add(usize::from(*width))
                    .saturating_add(gap);
                ends.push(offset);
                start
            })
            .collect();
        Some(Self {
            starts,
            ends,
            undeclared_start: offset,
            extra_width: preferences.extra_width_columns.map(usize::from),
            advance_limit: preferences.advance_limit_columns.map(usize::from),
        })
    }

    /// Declared origin for an actual field, including excess source cells.
    #[must_use]
    pub fn start(&self, column: usize) -> usize {
        self.starts
            .get(column)
            .copied()
            .unwrap_or(self.undeclared_start)
    }

    /// Configured end of a field, independent of its actual printed origin.
    /// Excess fields may have no capacity constraint.
    #[must_use]
    pub fn field_end(&self, column: usize) -> Option<usize> {
        self.ends.get(column).copied().or_else(|| {
            self.extra_width
                .map(|width| self.undeclared_start.saturating_add(width))
        })
    }

    /// Lay out complete cells in execution order. A subsequent cell follows
    /// the last physical row of its predecessor, never its first row. Widths
    /// are measured before ANSI/style wrappers. Long fields retain content
    /// and move the next field to a distinct physical row.
    #[must_use]
    pub fn place(&self, cells: &[Vec<ColumnFieldWidth>]) -> Option<Vec<Vec<ColumnPiece>>> {
        self.place_at(cells, 0)
    }

    /// Place cells relative to the actual table origin. Generated field
    /// origins must fit the shared display bounds after parent composition.
    #[must_use]
    pub fn place_at(
        &self,
        cells: &[Vec<ColumnFieldWidth>],
        origin: i32,
    ) -> Option<Vec<Vec<ColumnPiece>>> {
        if cells.len() > MAX_DECLARED_COLUMNS {
            return None;
        }
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut content_end = 0_usize;
        let mut output_end = 0_usize;
        let mut completed = false;
        let mut previous_end = None;
        let mut generated_padding = 0_usize;
        for (cell, lines) in cells.iter().enumerate() {
            for (line, width) in lines.iter().enumerate() {
                let start = self.start(cell);
                if width.content > width.output {
                    return None;
                }
                if line > 0
                    || completed
                    || (!row.is_empty() && previous_end.is_some_and(|end| content_end >= end))
                {
                    rows.push(std::mem::take(&mut row));
                    content_end = 0;
                    output_end = 0;
                    generated_padding = 0;
                }
                // Advance from the last graph while retaining all pending
                // author output. A source cap and the output-work bound are
                // distinct: neither discards glyphs or clips author padding.
                let advance = start.saturating_sub(content_end);
                let advance = self
                    .advance_limit
                    .map_or(advance, |limit| advance.min(limit));
                let column = content_end.saturating_add(advance).max(output_end);
                generated_padding =
                    generated_padding.saturating_add(column.saturating_sub(output_end));
                if generated_padding > MAX_COLUMN_PADDING
                    || super::padding(origin).saturating_add(column) > MAX_COLUMN_PADDING
                {
                    return None;
                }
                row.push(ColumnPiece { cell, line, column });
                content_end = column.saturating_add(width.content);
                output_end = column.saturating_add(width.output);
                completed = width.completed;
                previous_end = self.field_end(cell);
            }
        }
        if !row.is_empty() {
            rows.push(row);
        }
        Some(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preferences(values: &[u16]) -> crate::ColumnPreferences {
        crate::ColumnPreferences {
            widths: values.to_vec(),
            gap_columns: 4,
            advance_limit_columns: Some(256),
            extra_width_columns: Some(10),
        }
    }

    fn widths(values: &[usize]) -> Vec<ColumnFieldWidth> {
        values
            .iter()
            .map(|value| ColumnFieldWidth {
                content: *value,
                output: *value,
                completed: false,
            })
            .collect()
    }

    #[test]
    fn starts_do_not_add_default_widths_for_excess_cells() {
        let columns = DeclaredColumns::new(&preferences(&[8, 8])).unwrap();
        assert_eq!(
            (0..4).map(|i| columns.start(i)).collect::<Vec<_>>(),
            [0, 12, 24, 24]
        );
        assert_eq!(
            DeclaredColumns::new(&crate::ColumnPreferences {
                gap_columns: 3,
                ..preferences(&[2; 5])
            })
            .unwrap()
            .start(4),
            20
        );
        assert_eq!(
            DeclaredColumns::new(&crate::ColumnPreferences {
                gap_columns: 1,
                ..preferences(&[2; 6])
            })
            .unwrap()
            .start(5),
            15
        );
    }

    #[test]
    fn complete_cells_keep_order_and_preserve_empty_physical_rows() {
        let columns = DeclaredColumns::new(&preferences(&[1, 1])).unwrap();
        let placed = columns.place(&[widths(&[1, 0, 1]), widths(&[1])]).unwrap();
        assert_eq!(
            placed,
            [
                vec![ColumnPiece {
                    cell: 0,
                    line: 0,
                    column: 0
                }],
                vec![ColumnPiece {
                    cell: 0,
                    line: 1,
                    column: 0
                }],
                vec![
                    ColumnPiece {
                        cell: 0,
                        line: 2,
                        column: 0
                    },
                    ColumnPiece {
                        cell: 1,
                        line: 0,
                        column: 5
                    }
                ],
            ]
        );
        let overrun = columns.place(&[widths(&[13]), widths(&[6])]).unwrap();
        assert_eq!(overrun.len(), 2);
        assert_eq!(overrun[1][0].column, 5);
    }

    #[test]
    fn inbound_extremes_have_bounded_work_and_padding() {
        let columns = DeclaredColumns::new(&preferences(&[u16::MAX; 256])).unwrap();
        let fields = vec![widths(&[1]); 256];
        // Capped advances still fall back once cumulative generated padding
        // exceeds the independent row-work budget; all fields remain input.
        assert!(columns.place(&fields).is_none());
        assert!(DeclaredColumns::new(&preferences(&[1; 257])).is_none());
        assert!(columns.place(&vec![widths(&[1]); 257]).is_none());
    }

    #[test]
    fn only_breakable_trailing_padding_is_excluded_from_field_extent() {
        assert_eq!(declared_field_width("AAAA B "), 6);
        assert_eq!(declared_field_width("AAAA BB"), 7);
        assert_eq!(declared_field_width("AAAA B\u{a0}"), 7);
        assert_eq!(declared_field_width("中中 B "), 6);
        assert_eq!(declared_field_width("😀😀 B "), 6);
    }

    #[test]
    fn preserved_output_padding_never_moves_the_following_cursor_backwards() {
        let columns = DeclaredColumns::new(&preferences(&[3, 3])).unwrap();
        for (first, next_start) in [("AAAA B ", 7), ("X          ", 11)] {
            let placed = columns
                .place(&[
                    vec![ColumnFieldWidth::from_text(first)],
                    vec![ColumnFieldWidth::from_text("CLICK")],
                ])
                .unwrap();
            assert_eq!(placed.len(), 1);
            assert_eq!(placed[0][1].column, next_start);
        }
    }

    #[test]
    fn device_advance_is_capped_after_the_graph_before_preserved_tail_padding() {
        // A supplied device limit applies after graph measurement; preserved
        // trailing author padding remains independent of the capped advance.
        let columns = DeclaredColumns::new(&preferences(&[u16::MAX; 3])).unwrap();
        let placed = columns
            .place(&[
                vec![ColumnFieldWidth::from_text("A ")],
                vec![ColumnFieldWidth::from_text("B ")],
                vec![ColumnFieldWidth::from_text("C")],
            ])
            .unwrap();
        assert_eq!(
            placed[0]
                .iter()
                .map(|piece| piece.column)
                .collect::<Vec<_>>(),
            [0, 257, 514]
        );
    }
    #[test]
    fn completed_gap_rows_cannot_be_reused_by_the_following_cell() {
        let columns = DeclaredColumns::new(&preferences(&[1, 1])).unwrap();
        let open = vec![
            ColumnFieldWidth::from_text("X"),
            ColumnFieldWidth::from_text(""),
        ];
        let mut complete = open.clone();
        complete.last_mut().unwrap().completed = true;
        let next = vec![ColumnFieldWidth::from_text("C")];
        let open = columns.place(&[open, next.clone()]).unwrap();
        let complete = columns.place(&[complete, next]).unwrap();
        assert_eq!(open.len(), 2);
        assert_eq!(open[1].len(), 2);
        assert_eq!(complete.len(), 3);
        assert_eq!(complete[1].len(), 1);
        assert_eq!(complete[2][0].cell, 1);
    }
    #[test]
    fn generic_gaps_and_source_advance_limits_are_independent() {
        for (limit, expected) in [(None, 600), (Some(0), 1), (Some(256), 257)] {
            let preferences = crate::ColumnPreferences {
                widths: vec![598, 1],
                advance_limit_columns: limit,
                ..Default::default()
            };
            let columns = DeclaredColumns::new(&preferences).unwrap();
            assert_eq!(columns.start(1), 600);
            let placed = columns.place(&[widths(&[1]), widths(&[1])]).unwrap();
            assert_eq!(placed[0][1].column, expected);
        }
        for gap in [0, 1, 2, 6] {
            let preferences = crate::ColumnPreferences {
                widths: vec![8, 8],
                gap_columns: gap,
                ..Default::default()
            };
            let columns = DeclaredColumns::new(&preferences).unwrap();
            assert_eq!(columns.start(1), 8 + usize::from(gap));
            assert_eq!(columns.field_end(0), Some(columns.start(1)));
        }
    }

    #[test]
    fn excess_capacity_uses_the_previous_configured_end() {
        for capacity in [None, Some(0), Some(10)] {
            let preferences = crate::ColumnPreferences {
                widths: vec![8],
                gap_columns: 4,
                extra_width_columns: capacity,
                ..Default::default()
            };
            let columns = DeclaredColumns::new(&preferences).unwrap();
            assert_eq!(columns.start(1), 12);
            assert_eq!(columns.start(2), 12);
            for content in [0, 1, 9, 10, 11] {
                let placed = columns
                    .place(&[widths(&[1]), widths(&[content]), widths(&[1])])
                    .unwrap();
                let closes = capacity.is_some_and(|width| content >= usize::from(width));
                assert_eq!(
                    placed.len(),
                    if closes { 2 } else { 1 },
                    "capacity={capacity:?}, content={content}"
                );
                let last = placed.last().unwrap().last().unwrap();
                assert_eq!(last.cell, 2);
                assert_eq!(last.column, if closes { 12 } else { 12 + content });
            }
        }
    }

    #[test]
    fn generated_origins_are_bounded_after_parent_composition() {
        let fields = [widths(&[1]), widths(&[1])];
        for (first_width, valid) in [(4, true), (5, false)] {
            let columns = DeclaredColumns::new(&crate::ColumnPreferences {
                widths: vec![first_width, 1],
                ..Default::default()
            })
            .unwrap();
            assert_eq!(columns.place_at(&fields, 4090).is_some(), valid);
        }
        let columns = DeclaredColumns::new(&crate::ColumnPreferences {
            widths: vec![u16::MAX, 1],
            ..Default::default()
        })
        .unwrap();
        assert!(columns.place(&fields).is_none());
        // Authored extent is retained; it is not charged as generated padding.
        let author = columns.place(&[widths(&[8000])]).unwrap();
        assert_eq!(author[0][0].column, 0);
        let zero = DeclaredColumns::new(&crate::ColumnPreferences {
            widths: vec![u16::MAX; 256],
            advance_limit_columns: Some(0),
            ..Default::default()
        })
        .unwrap();
        let all = zero.place(&vec![widths(&[1]); 256]).unwrap();
        assert_eq!(all[0].len(), 256);
        assert_eq!(all[0].last().unwrap().column, 255);
    }
}
