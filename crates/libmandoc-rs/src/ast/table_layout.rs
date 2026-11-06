//! Shared native tbl layout facts, without copied terminal frame glyphs.

/// Requested number of strokes for a table edge.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableBorderStyle {
    /// One stroke.
    Single,
    /// Two strokes.
    Double,
}

/// One native layout row, shared by every data row using that declaration.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableLayout {
    /// Executed decimal point shared by this table's layouts.
    pub decimal_point: u8,
    /// Final shared left/right edge allowance used by native expansion.
    /// Boxed tables reserve two en; otherwise each outer vertical edge one.
    pub width_edge_columns: u8,
    /// Authored vertical edge before the first logical column.
    pub left_border: Option<TableBorderStyle>,
    /// Declared slots, including unoccupied trailing and spanning slots.
    pub cells: Vec<TableLayoutCell>,
}

/// Source layout facts for one zero-based logical tbl column.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableLayoutCell {
    /// Logical column number, independent of printable data cells.
    pub column: u32,
    /// Native layout operation, including slots with no data owner.
    pub kind: TableLayoutKind,
    /// Authored minimum width in mandoc basic units (24 units per en).
    /// Zero means no explicit minimum; this is not a clipping width.
    pub minimum_width_basic_units: u64,
    /// Requested spacing to the right, in en; `None` selects the tbl default.
    pub spacing_columns: Option<u32>,
    /// Exclude this cell from automatic width calculation (`z`).
    pub ignore_width: bool,
    /// Equalize this column with the other `e` columns.
    pub equal_width: bool,
    /// Expand this column into the available source line (`x`).
    pub maximize_width: bool,
    /// Authored vertical edge after this slot.
    pub right_border: Option<TableBorderStyle>,
}

/// The operation selected by a native tbl layout slot.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableLayoutKind {
    /// Centered text (`c`).
    Center,
    /// Right aligned text (`r`).
    Right,
    /// Left aligned text (`l`).
    Left,
    /// Decimal aligned text (`n`).
    Number,
    /// Continuation of a horizontal span (`s`).
    Span,
    /// Alphabetic text with an additional en of offset (`a`).
    Long,
    /// Continuation of a vertical span (`^`).
    Down,
    /// A single horizontal rule (`_`).
    HorizontalRule,
    /// A double horizontal rule (`=`).
    DoubleHorizontalRule,
}
