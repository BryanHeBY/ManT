//! Closed object decoding of column preferences and table-cell owners.
use super::{
    Block, ColumnPreferences, TableAlignment, TableBorderStyle, TableBorders, TableCell,
    TableCellKind, TableColumnBorder, TableRow, TableRowKind,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(
    remote = "ColumnPreferences",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ColumnPreferencesWire {
    #[serde(default)]
    widths: Vec<u16>,
    #[serde(default = "super::default_column_gap")]
    gap_columns: u16,
    #[serde(default)]
    max_alignment_padding_columns: Option<u16>,
    #[serde(default)]
    extra_width_columns: Option<u16>,
}

struct ColumnPreferencesVisitor;

impl<'de> serde::de::Visitor<'de> for ColumnPreferencesVisitor {
    type Value = ColumnPreferences;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a column preferences object")
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        ColumnPreferencesWire::deserialize(serde::de::value::MapAccessDeserializer::new(map))
    }
}

impl<'de> Deserialize<'de> for ColumnPreferences {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Deriving a struct decoder also accepts positional sequences. The
        // public wire and schema require an object, including for defaults.
        deserializer.deserialize_map(ColumnPreferencesVisitor)
    }
}

#[derive(Deserialize)]
#[serde(remote = "TableCell", rename_all = "camelCase", deny_unknown_fields)]
struct TableCellWire {
    #[serde(default)]
    kind: TableCellKind,
    blocks: Vec<Block>,
    #[serde(default)]
    break_after: bool,
    #[serde(default = "super::one_u32")]
    column_span: u32,
    #[serde(default = "super::one_u32")]
    row_span: u32,
    alignment: Option<TableAlignment>,
    #[serde(default)]
    preferred_width_columns: Option<u16>,
}

struct TableCellVisitor;

impl<'de> serde::de::Visitor<'de> for TableCellVisitor {
    type Value = TableCell;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a table cell object")
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        let cell = TableCellWire::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
        if cell.preferred_width_columns == Some(0) {
            return Err(serde::de::Error::custom(
                "table cell preferredWidthColumns must be positive",
            ));
        }
        if cell.column_span == 0 || cell.row_span == 0 {
            return Err(serde::de::Error::custom(
                "table row and column spans must be positive",
            ));
        }
        Ok(cell)
    }
}

#[derive(Deserialize)]
#[serde(remote = "TableBorders", rename_all = "camelCase", deny_unknown_fields)]
struct TableBordersWire {
    #[serde(default)]
    outer: Option<TableBorderStyle>,
    #[serde(default)]
    inner_rows: Option<TableBorderStyle>,
    #[serde(default)]
    inner_columns: Option<TableBorderStyle>,
}

struct TableBordersVisitor;

impl<'de> serde::de::Visitor<'de> for TableBordersVisitor {
    type Value = TableBorders;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a table borders object")
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        TableBordersWire::deserialize(serde::de::value::MapAccessDeserializer::new(map))
    }
}

impl<'de> Deserialize<'de> for TableBorders {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(TableBordersVisitor)
    }
}

#[derive(Deserialize)]
#[serde(
    remote = "TableColumnBorder",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct TableColumnBorderWire {
    column: u32,
    style: TableBorderStyle,
}

struct TableColumnBorderVisitor;

impl<'de> serde::de::Visitor<'de> for TableColumnBorderVisitor {
    type Value = TableColumnBorder;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a table column border object")
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        TableColumnBorderWire::deserialize(serde::de::value::MapAccessDeserializer::new(map))
    }
}

impl<'de> Deserialize<'de> for TableColumnBorder {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(TableColumnBorderVisitor)
    }
}

pub(super) fn column_borders<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TableColumnBorder>, D::Error> {
    let borders = Vec::<TableColumnBorder>::deserialize(deserializer)?;
    if borders
        .windows(2)
        .any(|pair| pair[0].column >= pair[1].column)
    {
        return Err(serde::de::Error::custom(
            "table column borders must be strictly ordered and unique",
        ));
    }
    Ok(borders)
}

#[derive(Deserialize)]
#[serde(remote = "TableRow", rename_all = "camelCase", deny_unknown_fields)]
struct TableRowWire {
    #[serde(default)]
    kind: TableRowKind,
    cells: Vec<TableCell>,
    #[serde(default, deserialize_with = "column_borders")]
    column_borders: Vec<TableColumnBorder>,
}

struct TableRowVisitor;

impl<'de> serde::de::Visitor<'de> for TableRowVisitor {
    type Value = TableRow;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a table row object")
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        TableRowWire::deserialize(serde::de::value::MapAccessDeserializer::new(map))
    }
}

impl<'de> Deserialize<'de> for TableRow {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(TableRowVisitor)
    }
}

impl<'de> Deserialize<'de> for TableCell {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // An owner is an object; positional arrays bypass the named contract.
        deserializer.deserialize_map(TableCellVisitor)
    }
}
