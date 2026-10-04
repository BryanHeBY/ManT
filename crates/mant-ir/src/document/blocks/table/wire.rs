//! Closed object decoding of column preferences and table-cell owners.
use super::{Block, ColumnPreferences, TableAlignment, TableCell, TableCellKind};
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
    advance_limit_columns: Option<u16>,
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
    #[serde(default = "super::one_u16")]
    column_span: u16,
    #[serde(default = "super::one_u16")]
    row_span: u16,
    alignment: Option<TableAlignment>,
}

struct TableCellVisitor;

impl<'de> serde::de::Visitor<'de> for TableCellVisitor {
    type Value = TableCell;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a table cell object")
    }

    fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        TableCellWire::deserialize(serde::de::value::MapAccessDeserializer::new(map))
    }
}

impl<'de> Deserialize<'de> for TableCell {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // An owner is an object; positional arrays bypass the named contract.
        deserializer.deserialize_map(TableCellVisitor)
    }
}
