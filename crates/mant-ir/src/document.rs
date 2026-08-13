//! Stable document nodes independent from their source parser.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::NodeId;

/// A normalized document ready for interactive or textual rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// Parser provenance retained independently from process-protocol metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parser: Option<ParserInfo>,
    pub source: DocumentSource,
    pub meta: DocumentMeta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Content preceding the first section heading.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    pub sections: Vec<Section>,
}

/// Parser implementation that produced this normalized document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParserInfo {
    pub name: String,
    pub version: String,
}

/// Source format consumed by the normalization engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SourceFormat {
    Man,
    Mdoc,
    Markdown,
}

/// Original source identity; temporary decompression paths must not appear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSource {
    pub format: SourceFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Metadata normalized from TH, Dt, and the validated libmandoc result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Native manual category such as `1` or `3p`; unrelated to document headings.
    pub manual_section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_target: Option<String>,
}

/// Recoverable parser or IR validation finding attached to the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

/// Severity reported by the parser without turning useful output into failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticLevel {
    Style,
    Warning,
    Error,
    Unsupported,
}

/// Zero-based UTF-8 byte offset in the original source.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(transparent)]
pub struct TextSize(u32);

impl TextSize {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[must_use]
    pub fn from_usize_saturating(value: usize) -> Self {
        Self(u32::try_from(value).unwrap_or(u32::MAX))
    }
}

/// Half-open UTF-8 byte range (`start..end`) in the original source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextRange {
    pub start: TextSize,
    pub end: TextSize,
}

impl TextRange {
    /// Construct a half-open range.
    ///
    /// # Panics
    ///
    /// Panics when `end` precedes `start`.
    #[must_use]
    pub fn new(start: TextSize, end: TextSize) -> Self {
        assert!(start <= end, "a source range cannot end before it starts");
        Self { start, end }
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }
}

/// Location in the original source file.
///
/// Lines and columns are one-based for diagnostics. `byte_range`, when the
/// parser provides exact offsets, is the canonical machine-facing boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceSpan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_range: Option<TextRange>,
    pub line: u32,
    pub column: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
}

/// Source-neutral document section headed by Markdown, man, or mdoc content.
///
/// This is a content subtree, not the native manual category stored in
/// [`DocumentMeta::manual_section`]. Depth is derived from tree position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// Unique within one document; consumers must not treat it as a global ID.
    pub id: NodeId,
    pub title: String,
    /// Terminal rows requested before this heading by the source macro set.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub spacing_before_lines: u16,
    pub blocks: Vec<Block>,
    pub children: Vec<Section>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

/// Presentation hints retained from roff but optional for semantic outputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LayoutHint {
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub indent_columns: u16,
    /// Terminal rows requested before this block.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub spacing_before_lines: u16,
}

/// A document block capable of preserving nested manual structures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Block {
    Paragraph {
        children: Vec<Inline>,
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    Preformatted {
        children: Vec<Inline>,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    List {
        kind: ListKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<u64>,
        #[serde(default, skip_serializing_if = "is_false")]
        compact: bool,
        items: Vec<ListItem>,
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    DefinitionList {
        items: Vec<DefinitionItem>,
        #[serde(default, skip_serializing_if = "is_false")]
        compact: bool,
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    Table {
        rows: Vec<TableRow>,
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    Equation {
        value: String,
        #[serde(default, skip_serializing_if = "is_false")]
        display: bool,
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    VerticalSpace {
        lines: u16,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    ThematicBreak {
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
    Unsupported {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        text: String,
        #[serde(default, skip_serializing_if = "LayoutHint::is_empty")]
        layout: LayoutHint,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<SourceSpan>,
    },
}

/// Marker behavior of an ordinary list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ListKind {
    Bullet,
    Ordered,
    Plain,
}

/// A list item contains blocks so nested lists and displays remain intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

/// A term may have aliases and its description may contain arbitrary blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionItem {
    /// Present when the native lowering pass can identify this definition as
    /// a stable semantic entry, such as a command-line option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<DefinitionIdentity>,
    pub terms: Vec<Vec<Inline>>,
    pub description: Vec<Block>,
    /// Render the term on the same line as the first description line (a man(7)
    /// hanging tag that fits the indent) instead of on its own line. Decided
    /// once during lowering so every renderer lays the item out identically.
    #[serde(default, skip_serializing_if = "is_false")]
    pub inline_term: bool,
    /// Terminal rows requested before this item when man(7) changes `.PD`.
    /// `None` inherits the containing list's compactness policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing_before_lines: Option<u16>,
}

/// Stable, renderer-independent identity for one navigable definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionIdentity {
    /// Unique within one document and shared with the term's inline anchor.
    pub id: NodeId,
    pub role: DefinitionRole,
    /// Matching policy used for aliases in semantic entry lookup.
    pub case: DefinitionCase,
    /// Plain normalized names suitable for outlines and agent selection.
    pub names: Vec<String>,
}

/// Case policy used when matching one semantic entry's names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionCase {
    Sensitive,
    Insensitive,
}

/// Semantic role assigned before roff macro details leave the native layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionRole {
    Option,
    Command,
    EnvironmentVariable,
    Variable,
}

/// One logical table row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

/// Block-capable table cell with optional layout information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    pub blocks: Vec<Block>,
    #[serde(default = "one_u16", skip_serializing_if = "is_one_u16")]
    pub column_span: u16,
    #[serde(default = "one_u16", skip_serializing_if = "is_one_u16")]
    pub row_span: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<TableAlignment>,
}

/// Horizontal alignment requested by a source table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TableAlignment {
    Left,
    Center,
    Right,
}

/// Inline content shared by prose, terms, and styled preformatted runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Inline {
    Text {
        value: String,
    },
    Strong {
        children: Vec<Inline>,
    },
    Emphasis {
        children: Vec<Inline>,
    },
    Code {
        value: String,
    },
    /// A typed link whose navigation semantics are explicit in the IR.
    Link {
        target: LinkTarget,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        children: Vec<Inline>,
    },
    /// A zero-width, document-local navigation destination such as mdoc `Tg`.
    ///
    /// Anchor IDs and section IDs share one namespace within a document.
    Anchor {
        id: NodeId,
    },
    LineBreak,
}

/// Resolved destination kind for [`Inline::Link`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum LinkTarget {
    /// An external URI from mdoc `Lk`, man `UR`, or Markdown links.
    External { uri: String },
    /// An email address without a `mailto:` prefix.
    Email { address: String },
    /// A relative Markdown link to another document in the current source.
    Document {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        fragment: Option<String>,
    },
    /// A typed reference to another installed manual page.
    Manual {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        manual_section: Option<String>,
    },
    /// A reference to a section in this document, normally originating at
    /// mdoc `Sx`.
    ///
    /// `target` is the document-local [`Section::id`] rather than a rendered
    /// heading slug. This keeps navigation stable across output formats.
    Section { id: NodeId },
}

impl LayoutHint {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.indent_columns == 0 && self.spacing_before_lines == 0
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_one_u16(value: &u16) -> bool {
    *value == 1
}

const fn one_u16() -> u16 {
    1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(value: &bool) -> bool {
    !*value
}
