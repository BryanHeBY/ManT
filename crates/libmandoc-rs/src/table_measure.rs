//! Native tbl width calculation over owned parser facts.
use crate::{Node, TableLayoutKind};

/// Effective native widths used by tbl's input-buffer execution.
/// These are source measurements, not responsive document layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableColumnMetrics {
    /// Effective column width in basic units (24 per en).
    pub width_basic_units: usize,
    /// Effective spacing after the column, in en.
    pub spacing_columns: usize,
    /// Effective decimal position in basic units.
    pub decimal_basic_units: usize,
}

impl TableColumnMetrics {
    /// Execute the source line-width request used by the native tbl solver.
    /// Widths are basic units; an omitted/invalid argument restores `previous`.
    #[must_use]
    pub fn line_length(current: usize, previous: usize, argument: Option<&str>) -> usize {
        crate::ffi::table_line_length(current, previous, argument)
    }
    /// Whether a horizontal motion writes the first native cell of an empty
    /// tbl column. The caller must first apply BACKAFTER cancellation.
    #[must_use]
    pub fn horizontal_motion_has_input(argument: &str) -> bool {
        crate::ffi::table_hmotion_input(argument, false)
    }
    /// Whether a positive relative motion still buffers a cell after its
    /// first cell clears BACKBEFORE (`term.c::term_word`, `ESCAPE_HORIZ`).
    #[must_use]
    pub fn horizontal_motion_has_input_after_backbefore(argument: &str) -> bool {
        crate::ffi::table_hmotion_input(argument, true)
    }
    /// Classify a native horizontal line: `None` is an invalid operand,
    /// `Some(true)` needs a positive execution-column width, and
    /// `Some(false)` has its own positive length. WMAX can produce zero-width
    /// columns, including columns whose declared minimum was positive.
    #[must_use]
    pub fn horizontal_line_needs_column_width(argument: &str) -> Option<bool> {
        crate::ffi::table_hline_input(argument)
    }
    /// Measure the padding requested by `tbl_term.c::tbl_number`.
    /// A caller must still apply `tbl_fill_string`'s current-position guard.
    ///
    /// # Errors
    /// Rejects invalid text, exhausted measurement budgets and callback panics.
    pub fn number_padding(
        self,
        text: &str,
        decimal: u8,
        measure: impl FnMut(&str) -> usize,
    ) -> Result<usize, String> {
        crate::ffi::number_padding(self, text, decimal, measure)
    }
}

/// Run pinned `out.c::tblcalc()` once over one table's owned spans.
///
/// `offset` and `margin` are source basic units, independent of viewport
/// width. `measure` follows `term_strlen` and returns basic units. No parser
/// pointer, output buffer or terminal is retained. Malformed foreign table
/// facts and a panicking measurement callback return an error.
///
/// # Errors
/// Rejects malformed owned facts, excessive input/measurement costs and
/// callback panics before their values can escape through the native solver.
pub fn measure_table_columns(
    rows: &[&Node],
    offset: usize,
    margin: usize,
    measure: impl FnMut(&str) -> usize,
) -> Result<Vec<TableColumnMetrics>, String> {
    crate::ffi::measure_table_columns(rows, offset, margin, measure)
}

pub(crate) const fn kind_code(kind: TableLayoutKind) -> i32 {
    match kind {
        TableLayoutKind::Center => 0,
        TableLayoutKind::Right => 1,
        TableLayoutKind::Left => 2,
        TableLayoutKind::Number => 3,
        TableLayoutKind::Span => 4,
        TableLayoutKind::Long => 5,
        TableLayoutKind::Down => 6,
        TableLayoutKind::HorizontalRule => 7,
        TableLayoutKind::DoubleHorizontalRule => 8,
    }
}
