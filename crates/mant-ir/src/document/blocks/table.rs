//! Source-neutral table structure, column preferences and cell boundaries.
use super::super::is_false;
use super::Block;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod wire;

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
    /// Optional maximum advance toward a preferred field origin. Zero disables
    /// that advance; absence leaves it unrestricted by the source device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advance_limit_columns: Option<u16>,
    /// Optional capacity of fields beyond the declared widths. These fields
    /// share the end of all declarations as their preferred origin.
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
            advance_limit_columns: None,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableRow {
    /// Structural row role retained independently from cell contents.
    #[serde(default, skip_serializing_if = "TableRowKind::is_data")]
    pub kind: TableRowKind,
    /// Cells in column order. Horizontal spans omit covered cells; vertical
    /// continuations retain empty cells at their logical positions. Use
    /// [`crate::TableGrid`] to place cells without losing horizontal spans.
    pub cells: Vec<TableCell>,
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
    /// Effective native cell content after tbl layout-rule precedence.
    #[serde(default, skip_serializing_if = "TableCellKind::is_text")]
    pub kind: TableCellKind,
    /// Block content contained in the cell.
    pub blocks: Vec<Block>,
    /// Close the current physical data row after this cell, before the next.
    /// The row can be occupied by an earlier cell or already exist as an empty
    /// structural row. This adds neither body scalars nor an additional row.
    #[serde(default, skip_serializing_if = "is_false")]
    pub break_after: bool,
    /// Number of logical columns occupied by the cell.
    #[serde(default = "one_u16", skip_serializing_if = "is_one_u16")]
    pub column_span: u16,
    /// Number of logical rows occupied by the cell.
    #[serde(default = "one_u16", skip_serializing_if = "is_one_u16")]
    pub row_span: u16,
    /// Requested horizontal alignment, if explicitly known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<TableAlignment>,
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

/// Horizontal alignment requested by a source table.
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
const fn is_one_u16(value: &u16) -> bool {
    *value == 1
}

const fn one_u16() -> u16 {
    1
}
