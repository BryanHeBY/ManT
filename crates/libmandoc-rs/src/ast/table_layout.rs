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
    /// Authored minimum width in mandoc basic units (24 units per en).
    /// Zero means no explicit minimum; this is not a clipping width.
    pub minimum_width_basic_units: u64,
    /// Authored vertical edge after this slot.
    pub right_border: Option<TableBorderStyle>,
}
