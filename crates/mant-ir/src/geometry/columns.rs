//! Bounded placement of declared table fields, shared by terminal consumers.

/// Maximum one-device advance, from pinned `term_ascii.c::ascii_advance()`.
pub const MAX_COLUMN_ADVANCE: usize = 256;
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
    /// Build bounded source-order starts. CVS `mdoc_term.c::termp_it_pre()`
    /// adds every earlier declared width plus 4/3/1 cells. Extra BODY parts
    /// share the sum of all declarations; their own default width of ten
    /// does not contribute to the preceding-offset loop.
    #[must_use]
    pub fn new(widths: &[u16]) -> Option<Self> {
        if widths.is_empty() || widths.len() > MAX_DECLARED_COLUMNS {
            return None;
        }
        let gap = match widths.len() {
            count if count < 5 => 4,
            5 => 3,
            _ => 1,
        };
        let mut offset = 0_usize;
        let starts = widths
            .iter()
            .map(|width| {
                let start = offset;
                offset = offset
                    .saturating_add(usize::from(*width))
                    .saturating_add(gap);
                start
            })
            .collect();
        Some(Self {
            starts,
            undeclared_start: offset,
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

    /// Lay out complete cells in execution order. A subsequent cell follows
    /// the last physical row of its predecessor, never its first row. Widths
    /// are measured before ANSI/style wrappers. Long fields retain content
    /// and move the next field to a distinct physical row.
    #[must_use]
    pub fn place(&self, cells: &[Vec<ColumnFieldWidth>]) -> Option<Vec<Vec<ColumnPiece>>> {
        if cells.len() > MAX_DECLARED_COLUMNS {
            return None;
        }
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut content_end = 0_usize;
        let mut output_end = 0_usize;
        let mut completed = false;
        for (cell, lines) in cells.iter().enumerate() {
            for (line, width) in lines.iter().enumerate() {
                let start = self.start(cell);
                if width.content > width.output {
                    return None;
                }
                if line > 0 || completed || (!row.is_empty() && content_end >= start) {
                    rows.push(std::mem::take(&mut row));
                    content_end = 0;
                    output_end = 0;
                }
                // term_field delays trailing padding: the native advance
                // starts at the last printed graph, not after that padding.
                // Preserve source-neutral output in full without counting its
                // tail a second time when applying the device advance cap.
                let column = content_end
                    .saturating_add(start.saturating_sub(content_end).min(MAX_COLUMN_ADVANCE))
                    .max(output_end);
                row.push(ColumnPiece { cell, line, column });
                content_end = column.saturating_add(width.content);
                output_end = column.saturating_add(width.output);
                completed = width.completed;
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
        let columns = DeclaredColumns::new(&[8, 8]).unwrap();
        assert_eq!(
            (0..4).map(|i| columns.start(i)).collect::<Vec<_>>(),
            [0, 12, 24, 24]
        );
        assert_eq!(DeclaredColumns::new(&[2; 5]).unwrap().start(4), 20);
        assert_eq!(DeclaredColumns::new(&[2; 6]).unwrap().start(5), 15);
    }

    #[test]
    fn complete_cells_keep_order_and_preserve_empty_physical_rows() {
        let columns = DeclaredColumns::new(&[1, 1]).unwrap();
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
        let columns = DeclaredColumns::new(&[u16::MAX; 256]).unwrap();
        let fields = vec![widths(&[1]); 256];
        let placed = columns.place(&fields).unwrap();
        assert_eq!(placed.iter().map(Vec::len).sum::<usize>(), 256);
        for row in placed {
            let mut end = 0;
            for piece in row {
                assert!(piece.column.saturating_sub(end) <= MAX_COLUMN_ADVANCE);
                end = piece.column.saturating_add(1);
            }
        }
        assert!(DeclaredColumns::new(&[1; 257]).is_none());
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
        let columns = DeclaredColumns::new(&[3, 3]).unwrap();
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
        // Exact A/Ta/B sources ran pristine CVS before this assertion:
        // term_field defers the trailing cell and ascii/locale_advance cap
        // a single advance from A's printed end to 256 display columns.
        let columns = DeclaredColumns::new(&[u16::MAX; 3]).unwrap();
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
        let columns = DeclaredColumns::new(&[1, 1]).unwrap();
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
}
