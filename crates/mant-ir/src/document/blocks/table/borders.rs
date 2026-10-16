//! Author-specified table edges, independent of terminal glyphs and theme.
use super::TableRow;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Source-neutral table frame and grid preferences.
///
/// Grid edges follow logical cells: covered span interiors have no edge.
/// Explicit authored rule rows remain separate table content, rather than
/// being duplicated by a grid edge at the same boundary.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableBorders {
    /// Stroke of the complete outside frame, if requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer: Option<TableBorderStyle>,
    /// Stroke between adjacent data rows, excluding covered span interiors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_rows: Option<TableBorderStyle>,
    /// Stroke between logical columns, excluding covered span interiors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_columns: Option<TableBorderStyle>,
}

impl TableBorders {
    /// Whether omitting this value preserves every edge preference.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.outer.is_none() && self.inner_rows.is_none() && self.inner_columns.is_none()
    }

    /// Default vertical edge at a logical boundary, before row-local edges.
    /// This does not allocate a grid or decide whether a span covers the edge.
    #[must_use]
    pub const fn column_style(
        &self,
        column: usize,
        column_count: usize,
    ) -> Option<TableBorderStyle> {
        if column == 0 || column == column_count {
            self.outer
        } else if column < column_count {
            self.inner_columns
        } else {
            None
        }
    }
}

/// One or two parallel strokes; glyphs, color and occupied cells are consumer choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TableBorderStyle {
    /// A single stroke.
    Single,
    /// A double stroke.
    Double,
}

impl<'de> Deserialize<'de> for TableBorderStyle {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StyleVisitor;
        impl serde::de::Visitor<'_> for StyleVisitor {
            type Value = TableBorderStyle;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("the string single or double")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "single" => Ok(TableBorderStyle::Single),
                    "double" => Ok(TableBorderStyle::Double),
                    _ => Err(E::unknown_variant(value, &["single", "double"])),
                }
            }
        }
        deserializer.deserialize_str(StyleVisitor)
    }
}

impl TableBorderStyle {
    /// Resolve coincident author-specified edges without depending on order.
    #[must_use]
    pub const fn stronger(self, other: Self) -> Self {
        if matches!(self, Self::Double) || matches!(other, Self::Double) {
            Self::Double
        } else {
            Self::Single
        }
    }
}

/// A row-local vertical edge at a zero-based logical column boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableColumnBorder {
    /// Boundary in `0..=columnCount`, independent of display-cell widths.
    #[schemars(range(min = 0, max = 4_294_967_295_u32))]
    pub column: u32,
    /// Requested edge stroke, combined with defaults using the stronger value.
    pub style: TableBorderStyle,
}

impl TableRow {
    pub(crate) fn column_borders_valid(&self, column_count: usize) -> bool {
        self.column_borders
            .iter()
            .all(|border| usize::try_from(border.column).unwrap_or(usize::MAX) <= column_count)
            && self
                .column_borders
                .windows(2)
                .all(|pair| pair[0].column < pair[1].column)
    }

    /// Effective row-local or default edge. Call on validated, ordered rows;
    /// span suppression remains a separate topology decision.
    #[must_use]
    pub fn column_border_style(
        &self,
        borders: &TableBorders,
        column: usize,
        column_count: usize,
    ) -> Option<TableBorderStyle> {
        if column > column_count {
            return None;
        }
        let local = u32::try_from(column).ok().and_then(|column| {
            self.column_borders
                .binary_search_by_key(&column, |edge| edge.column)
                .ok()
                .map(|index| self.column_borders[index].style)
        });
        match (borders.column_style(column, column_count), local) {
            (Some(default), Some(local)) => Some(default.stronger(local)),
            (default, local) => default.or(local),
        }
    }
}
