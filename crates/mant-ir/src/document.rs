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
    /// Original source format and stable path.
    pub source: DocumentSource,
    /// Metadata normalized across all supported source formats.
    pub meta: DocumentMeta,
    /// Exact source fragments resolving to the normalized document root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragment_aliases: Vec<crate::FragmentAlias>,
    /// Recoverable findings retained for callers that need source quality data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Content preceding the first section heading.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Top-level semantic sections in source order.
    pub sections: Vec<Section>,
}

/// Parser implementation that produced this normalized document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParserInfo {
    /// Stable parser implementation name.
    pub name: String,
    /// Parser version used to produce the document.
    pub version: String,
}

/// Source format consumed by the normalization engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SourceFormat {
    /// Traditional man(7) macros.
    Man,
    /// Semantic mdoc(7) macros.
    Mdoc,
    /// Markdown with `ManT` semantic extensions.
    Markdown,
}

/// Original source identity; temporary decompression paths must not appear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSource {
    /// Syntax family consumed by the parser.
    pub format: SourceFormat,
    /// Stable caller-facing path, when the source has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Metadata normalized from TH, Dt, and the validated libmandoc result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    /// Canonical document title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Native manual category such as `1` or `3p`; unrelated to document headings.
    pub manual_section: Option<String>,
    /// Normalized publication or revision date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Manual volume or collection label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    /// Operating-system label declared by the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Architecture qualifier declared by the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// Primary name followed by any aliases from the NAME section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// Logical target of a native `.so` alias page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_target: Option<String>,
}

/// Recoverable parser or IR validation finding attached to the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// Severity of the finding.
    pub level: DiagnosticLevel,
    /// Stable machine-readable code, when the producer defines one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Concise human-readable explanation.
    pub message: String,
    /// Original source location associated with the finding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

/// Severity reported by the parser without turning useful output into failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticLevel {
    /// Non-semantic source style issue.
    Style,
    /// Recoverable source defect or portability concern.
    Warning,
    /// Invalid source that left partial output available.
    Error,
    /// Valid construct that the active parser cannot represent fully.
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
    /// Construct an offset from a zero-based UTF-8 byte count.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Return the underlying UTF-8 byte count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Convert a platform-sized offset, clamping values above `u32::MAX`.
    #[must_use]
    pub fn from_usize_saturating(value: usize) -> Self {
        Self(u32::try_from(value).unwrap_or(u32::MAX))
    }
}

/// Half-open UTF-8 byte range (`start..end`) in the original source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextRange {
    /// Inclusive start offset.
    pub start: TextSize,
    /// Exclusive end offset.
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

    /// Return whether the range contains no source bytes.
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
    /// Exact half-open byte range, when supplied by the parser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_range: Option<TextRange>,
    /// One-based starting line.
    pub line: u32,
    /// One-based starting column.
    pub column: u32,
    /// One-based inclusive ending line, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// One-based exclusive ending column, when known.
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
    /// Exact source fragments resolving to this normalized section identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragment_aliases: Vec<crate::FragmentAlias>,
    /// Visible heading text.
    pub title: String,
    /// Terminal rows requested before this heading by the source macro set.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub spacing_before_lines: u16,
    /// Content directly owned by this section.
    pub blocks: Vec<Block>,
    /// Nested subsections in source order.
    pub children: Vec<Section>,
    /// Heading location in the original source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

/// Presentation hints retained from roff but optional for semantic outputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LayoutHint {
    /// Additional terminal columns requested before the block.
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
    /// Reflowable prose.
    Paragraph {
        /// Styled inline content in source order.
        children: Vec<Inline>,
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
        /// First ordinal for an ordered list.
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<u64>,
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
    /// Explicit vertical spacing requested by the source.
    VerticalSpace {
        /// Number of terminal rows requested.
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

/// Marker behavior of an ordinary list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ListKind {
    /// Unordered list with bullets.
    Bullet,
    /// Ordered list with ordinal markers.
    Ordered,
    /// Marker-free list.
    Plain,
}

/// A list item contains blocks so nested lists and displays remain intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListItem {
    /// Arbitrary item content in source order.
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
    /// One or more equivalent displayed terms.
    pub terms: Vec<Vec<Inline>>,
    /// Block content describing the terms.
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

/// Renderer-independent identity attached to one navigable content definition.
///
/// [`crate::SemanticIndex`] derives logical entries from these source facts.
/// The identity remains part of the authoritative document tree; an outline or
/// another projection may omit the entry without removing its definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionIdentity {
    /// Unique within one document and shared with the term's inline anchor.
    pub id: NodeId,
    /// Semantic category used by lookup and presentation.
    pub role: DefinitionRole,
    /// Matching policy used for aliases in semantic entry lookup.
    pub case: DefinitionCase,
    /// Plain normalized names used as [`crate::SemanticEntry::aliases`].
    pub names: Vec<String>,
    /// Optional value space explicitly declared by the source producer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_domain: Option<crate::ValueDomain>,
}

/// Case policy used when matching one semantic entry's names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionCase {
    /// Names must match with the same Unicode scalar values and case.
    Sensitive,
    /// Names match without ASCII case distinctions.
    Insensitive,
}

/// Semantic role assigned before source-specific details leave the parser.
///
/// [`crate::SemanticIndex`] maps option, marker, and operand roles to
/// [`crate::EntryKind::Parameter`] and maps the remaining roles directly to
/// their corresponding entry categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionRole {
    /// Command-line option or switch.
    Option,
    /// End-of-options or parser-control marker such as `--` or `--%`.
    Marker,
    /// Positional or special operand documented as a definition.
    Operand,
    /// Executable subcommand or verb.
    Command,
    /// Named key accepted by a configuration language or command option.
    ConfigurationKey,
    /// Process environment variable.
    EnvironmentVariable,
    /// Other named configuration or language variable.
    Variable,
    /// One documented value accepted by a parent entry.
    Value,
    /// Addressable definition without a more specific reliable role.
    Term,
}

/// One logical table row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableRow {
    /// Logical cells in column order.
    pub cells: Vec<TableCell>,
}

/// Block-capable table cell with optional layout information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    /// Block content contained in the cell.
    pub blocks: Vec<Block>,
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

/// Inline content shared by prose, terms, and styled preformatted runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Inline {
    /// Plain visible text.
    Text {
        /// Text after source escape processing.
        value: String,
    },
    /// Strongly emphasized content.
    Strong {
        /// Nested inline content.
        children: Vec<Inline>,
    },
    /// Emphasized content.
    Emphasis {
        /// Nested inline content.
        children: Vec<Inline>,
    },
    /// Literal code or symbolic token.
    Code {
        /// Literal text value.
        value: String,
    },
    /// A typed link whose navigation semantics are explicit in the IR.
    ///
    /// Only document and manual targets form cross-document graph edges.
    /// Section targets remain local, while external and email targets require a
    /// host action and never expand a documentation scope.
    Link {
        /// Typed navigation destination.
        target: LinkTarget,
        /// Optional advisory title.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Visible linked content.
        children: Vec<Inline>,
    },
    /// A zero-width, document-local navigation destination such as mdoc `Tg`.
    ///
    /// Anchor IDs and section IDs share one namespace within a document.
    Anchor {
        /// Document-local destination identity.
        id: NodeId,
        /// Exact source fragments resolving to this normalized identity.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fragment_aliases: Vec<crate::FragmentAlias>,
        /// Source location of the addressable owner receiving this target.
        ///
        /// For a standalone target this is the target request itself; when a
        /// parser attaches the target to a paragraph, definition, list item,
        /// or table cell, it is that owning construct's location.
        #[serde(skip_serializing_if = "Option::is_none")]
        owner_source: Option<SourceSpan>,
    },
    /// Hard line break that renderers must preserve.
    LineBreak,
}

impl Inline {
    /// Construct a normalized local anchor without source-authored aliases.
    #[must_use]
    pub fn anchor(id: impl Into<NodeId>) -> Self {
        Self::Anchor {
            id: id.into(),
            fragment_aliases: Vec::new(),
            owner_source: None,
        }
    }

    /// Construct a normalized local anchor at its addressable source owner.
    #[must_use]
    pub fn anchor_at(id: impl Into<NodeId>, owner_source: Option<SourceSpan>) -> Self {
        Self::Anchor {
            id: id.into(),
            fragment_aliases: Vec::new(),
            owner_source,
        }
    }

    /// Construct a normalized local anchor with exact source fragments.
    #[must_use]
    pub fn anchor_with_aliases(
        id: impl Into<NodeId>,
        fragment_aliases: Vec<crate::FragmentAlias>,
    ) -> Self {
        Self::Anchor {
            id: id.into(),
            fragment_aliases,
            owner_source: None,
        }
    }
}

/// Typed destination kind for [`Inline::Link`].
///
/// This enum records navigation intent, not host resolution state. The query
/// engine resolves [`LinkTarget::Document`] and [`LinkTarget::Manual`] against
/// the catalog; consumers must not infer equivalent edges from rendered text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum LinkTarget {
    /// An external URI from mdoc `Lk`, man `UR`, or Markdown links.
    External {
        /// Absolute URI.
        uri: String,
    },
    /// An email address without a `mailto:` prefix.
    Email {
        /// Mailbox address without a URI scheme.
        address: String,
    },
    /// A relative Markdown link to another document in the current source.
    ///
    /// This is a logical cross-document graph edge, not a physical path.
    Document {
        /// Extension-free relative document path.
        name: String,
        /// Optional document-local destination.
        #[serde(skip_serializing_if = "Option::is_none")]
        fragment: Option<String>,
    },
    /// A typed reference to another installed manual page.
    ///
    /// This is a logical cross-document graph edge resolved through the manual
    /// catalog and its normal ambiguity rules.
    Manual {
        /// Manual topic without a section suffix.
        name: String,
        /// Native manual category, when specified by the source.
        #[serde(skip_serializing_if = "Option::is_none")]
        manual_section: Option<String>,
    },
    /// A reference to a section in this document, normally originating at
    /// mdoc `Sx`.
    ///
    /// `target` is the document-local [`Section::id`] rather than a rendered
    /// heading slug. This keeps navigation stable across output formats.
    Section {
        /// Target section identity in the current document.
        id: NodeId,
    },
}

impl LayoutHint {
    /// Return whether the hint requests no additional layout behavior.
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
