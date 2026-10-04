//! Closed wire decoding of block owners and their local row hints.
use super::{
    Block, ColumnPreferences, DefinitionItem, EquationExpression, Inline, LayoutHint, ListItem,
    ListKind, SourceSpan, TableAlignment, TableCell, TableCellKind, TableRow,
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

#[derive(Deserialize)]
#[serde(
    remote = "Block",
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum BlockWire {
    /// Reflowable prose.
    Paragraph {
        /// Styled inline content in source order.
        children: Vec<Inline>,
        /// Exceptional row offsets in this content root, never inline text.
        #[serde(default, skip_serializing_if = "crate::InlineLayout::is_empty")]
        inline_layout: crate::InlineLayout,
        /// Source-derived indentation and vertical spacing.
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    /// Literal content that preserves line boundaries.
    Preformatted {
        /// Styled literal runs and line breaks.
        children: Vec<Inline>,
        /// Exceptional row offsets in this literal content root.
        #[serde(default, skip_serializing_if = "crate::InlineLayout::is_empty")]
        inline_layout: crate::InlineLayout,
        /// Optional language hint, primarily from fenced Markdown.
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        /// Source-derived indentation and vertical spacing.
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    /// Ordered, unordered, or marker-free block list.
    List {
        /// Marker behavior for the list.
        kind: ListKind,
        /// Whether renderers should suppress extra spacing between items.
        #[serde(default, skip_serializing_if = "is_false")]
        compact: bool,
        /// List items in source order.
        items: Vec<ListItem>,
        /// Source-derived indentation and vertical spacing.
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    /// Term-and-description list.
    DefinitionList {
        /// Definitions in source order.
        items: Vec<DefinitionItem>,
        /// Recovered consecutive declaration contexts, not alias relationships.
        /// Ranges refer to this list only and do not change item ownership/layout.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        declaration_groups: Vec<crate::DeclarationGroup>,
        /// Whether renderers should suppress extra spacing between definitions.
        #[serde(default, skip_serializing_if = "is_false")]
        compact: bool,
        /// Source-derived indentation and vertical spacing.
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    /// Block-capable table.
    Table {
        /// Logical rows in source order.
        rows: Vec<TableRow>,
        /// Preferred column geometry, independent of source macro rules.
        #[serde(default, skip_serializing_if = "ColumnPreferences::is_empty")]
        column_preferences: ColumnPreferences,
        /// Source-derived indentation and vertical spacing.
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    /// Equation retained as a normalized expression.
    Equation {
        /// Equation source after parser normalization.
        value: String,
        /// Parsed equation structure, when the source parser provides it.
        /// `value` must equal this structure's readable projection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expression: Option<EquationExpression>,
        /// Whether the equation occupies its own display block.
        #[serde(default, skip_serializing_if = "is_false")]
        display: bool,
        /// Source-derived indentation and vertical spacing.
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    /// Completed blank rows emitted by the producer.
    VerticalSpace {
        /// Number of executed blank rows.
        lines: u16,
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    /// Horizontal thematic separator.
    ThematicBreak {
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    /// Source construct retained because it has no native IR representation.
    Unsupported {
        /// Macro or construct name, when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Best-effort visible text retained for consumers.
        text: String,
        /// Source-derived indentation and vertical spacing.
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        /// Original source range.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
}

impl<'de> Deserialize<'de> for Block {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let block = BlockWire::deserialize(deserializer)?;
        if let Self::Paragraph {
            children,
            inline_layout,
            ..
        }
        | Self::Preformatted {
            children,
            inline_layout,
            ..
        } = &block
        {
            inline_layout
                .validate(children)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(block)
    }
}
