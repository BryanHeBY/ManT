//! Stable manual-document nodes independent from roff and HTML renderers.

use serde::{Deserialize, Serialize};

/// Exact schema marker for a normalized manual document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentSchema {
    /// First stable renderer-neutral document model.
    #[serde(rename = "mant.document/v1")]
    V1,
}

/// A normalized manual page ready for interactive or textual rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MantDocument {
    pub schema: DocumentSchema,
    pub producer: Producer,
    pub source: DocumentSource,
    pub meta: DocumentMeta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    pub sections: Vec<Section>,
}

/// Identifies Mant and the parsing engine used to build the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Producer {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
}

/// Pinned parser implementation behind the stable Mant contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub name: String,
    pub version: String,
}

/// Source format consumed by the normalization engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceFormat {
    Man,
    Mdoc,
    GroffHtml,
    MandocHtml,
}

/// Original source identity; temporary decompression paths must not appear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSource {
    pub format: SourceFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renderer: Option<String>,
}

/// Metadata normalized from TH, Dt, and the validated libmandoc result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
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

/// Recoverable parser finding attached to the returned best-effort document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticLevel {
    Style,
    Warning,
    Error,
    Unsupported,
}

/// One-based location in the original source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSpan {
    pub line: u32,
    pub column: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
}

/// Hierarchical manual section. Depth is derived from tree position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// Unique within one document; consumers must not treat it as a global ID.
    pub id: String,
    pub title: String,
    pub blocks: Vec<Block>,
    pub children: Vec<Section>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

/// Presentation hints retained from roff but optional for semantic outputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutHint {
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub indent_columns: u16,
}

/// A document block capable of preserving nested manual structures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ListKind {
    Bullet,
    Ordered,
    Plain,
}

/// A list item contains blocks so nested lists and displays remain intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

/// A term may have aliases and its description may contain arbitrary blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionItem {
    pub terms: Vec<Vec<Inline>>,
    pub description: Vec<Block>,
    /// Terminal rows requested before this item when man(7) changes `.PD`.
    /// `None` inherits the containing list's compactness policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing_before_lines: Option<u16>,
}

/// One logical table row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

/// Block-capable table cell with optional layout information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TableAlignment {
    Left,
    Center,
    Right,
}

/// Inline content shared by prose, terms, and styled preformatted runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// An external URI from mdoc `Lk`, man `UR`, or renderer-derived input.
    ///
    /// Roff section references use [`Inline::SectionReference`] instead so
    /// consumers never have to infer navigation semantics from URI syntax.
    ExternalLink {
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        children: Vec<Inline>,
    },
    /// An email address from mdoc `Mt` or man `MT` without a `mailto:` prefix.
    EmailLink {
        address: String,
        children: Vec<Inline>,
    },
    /// A typed reference to another installed manual page.
    ManualReference {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        section: Option<String>,
        children: Vec<Inline>,
    },
    /// A reference to a section in this document, normally originating at
    /// mdoc `Sx`.
    ///
    /// `target` is the document-local [`Section::id`] rather than a rendered
    /// heading slug. This keeps navigation stable across output formats.
    SectionReference {
        target: String,
        children: Vec<Inline>,
    },
    /// A zero-width, document-local navigation destination such as mdoc `Tg`.
    ///
    /// Anchor IDs and section IDs share one namespace within a document.
    Anchor {
        id: String,
    },
    LineBreak,
}

impl LayoutHint {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.indent_columns == 0
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
