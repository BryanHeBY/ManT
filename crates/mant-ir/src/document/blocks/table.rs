//! Source-neutral table structure, column preferences and cell boundaries.
use super::super::is_false;
use super::Block;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod borders;
mod wire;
pub use borders::{TableBorderStyle, TableBorders, TableColumnBorder};

/// Source-neutral preferences for measured table columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColumnPreferences {
    /// Preferred content widths in display cells. Empty uses measured content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widths: Vec<u16>,
    /// Whitespace between columns, excluding any renderer's structural glyph.
    #[serde(
        default = "default_column_gap",
        skip_serializing_if = "is_default_column_gap"
    )]
    pub gap_columns: u16,
    /// Optional maximum alignment-padding region after the preceding field's
    /// content extent, excluding ordinary trailing spaces from that extent.
    /// Preserved trailing output occupies this region and is never clipped.
    /// Zero adds no alignment padding; absence leaves this preference unbounded.
    /// This is a resolved layout preference, not a consumer work budget.
    /// Placement takes the greater of the full output end and the content end
    /// plus the capped distance toward the preferred origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_alignment_padding_columns: Option<u16>,
    /// Resolved optional content capacity of fields beyond the declared widths.
    /// Retained content is never clipped. These fields share the end of all
    /// declarations as their preferred origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_width_columns: Option<u16>,
}

const fn default_column_gap() -> u16 {
    2
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
fn is_default_column_gap(value: &u16) -> bool {
    *value == default_column_gap()
}

impl Default for ColumnPreferences {
    fn default() -> Self {
        Self {
            widths: Vec::new(),
            gap_columns: default_column_gap(),
            max_alignment_padding_columns: None,
            extra_width_columns: None,
        }
    }
}

impl ColumnPreferences {
    /// Whether omitting this value preserves all column preferences.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// One logical table row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableRow {
    /// Structural row role retained independently from cell contents.
    #[serde(default, skip_serializing_if = "TableRowKind::is_data")]
    pub kind: TableRowKind,
    /// Cells in column order. Horizontal spans omit covered cells; vertical
    /// continuations retain empty cells at their logical positions. Use
    /// [`crate::TableGrid`] to place cells without losing horizontal spans.
    pub cells: Vec<TableCell>,
    /// Additional vertical edges at logical column boundaries in this row.
    /// Entries are strictly ordered and unique; 0 and the table's column
    /// count denote its outside edges. Spanned interiors remain unpainted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub column_borders: Vec<TableColumnBorder>,
}

/// Structural role of one logical table row.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum TableRowKind {
    /// A data row, including an intentionally empty source row.
    #[default]
    Data,
    /// A whole-row single horizontal rule.
    HorizontalRule,
    /// A whole-row double horizontal rule.
    DoubleHorizontalRule,
    /// A rule row authored in the tbl layout, retaining the strength of each
    /// logical column rather than collapsing a mixed `_`/`=` row.
    LayoutRule {
        /// Rule strengths in logical column order.
        cells: Vec<TableRuleCellKind>,
    },
}

impl TableRowKind {
    // Serde's `skip_serializing_if` predicate receives a shared reference.
    const fn is_data(value: &Self) -> bool {
        matches!(value, Self::Data)
    }
}

/// Horizontal rule strength for one logical column of a layout-only row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TableRuleCellKind {
    /// A single horizontal rule.
    Horizontal,
    /// A double horizontal rule.
    DoubleHorizontal,
}

/// Block-capable table cell with optional layout information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableCell {
    /// Effective cell content after the producer resolves source rule precedence.
    #[serde(default, skip_serializing_if = "TableCellKind::is_text")]
    pub kind: TableCellKind,
    /// Block content contained in the cell.
    pub blocks: Vec<Block>,
    /// The current physical data row ends after this cell, before the next.
    /// The row can be occupied by an earlier cell or already exist as an empty
    /// structural row. This adds neither body scalars nor an additional row.
    /// This is a resolved topology fact, not a line-break instruction.
    #[serde(default, skip_serializing_if = "is_false")]
    pub break_after: bool,
    /// Number of logical columns occupied by the cell.
    #[serde(default = "one_u32", skip_serializing_if = "is_one_u32")]
    #[schemars(range(min = 1, max = 4_294_967_295_u32))]
    pub column_span: u32,
    /// Number of logical rows occupied by the cell.
    #[serde(default = "one_u32", skip_serializing_if = "is_one_u32")]
    #[schemars(range(min = 1, max = 4_294_967_295_u32))]
    pub row_span: u32,
    /// Requested horizontal alignment, if explicitly known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<TableAlignment>,
    /// Positive soft preferred content width in display cells. Readers may
    /// shrink or wrap the cell at their viewport without changing its body.
    /// This does not select declared-field placement or imply a fixed width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65535))]
    pub preferred_width_columns: Option<u16>,
}

/// Effective content role of one table cell.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TableCellKind {
    /// Printable data or an ordinary empty cell.
    #[default]
    Text,
    /// A connecting single horizontal rule.
    HorizontalRule,
    /// A connecting double horizontal rule.
    DoubleHorizontalRule,
    /// An isolated single horizontal rule (`\_`).
    IsolatedHorizontalRule,
    /// An isolated double horizontal rule (`\=`).
    IsolatedDoubleHorizontalRule,
}

impl TableCellKind {
    #[allow(clippy::trivially_copy_pass_by_ref)] // serde skip predicates borrow the field
    const fn is_text(value: &Self) -> bool {
        matches!(value, Self::Text)
    }
}

/// Horizontal placement of a complete table frame or content within a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TableAlignment {
    /// Align content to the left edge.
    Left,
    /// Center content horizontally.
    Center,
    /// Align content to the right edge.
    Right,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_one_u32(value: &u32) -> bool {
    *value == 1
}

const fn one_u32() -> u32 {
    1
}
