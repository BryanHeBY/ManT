//! Block, item and table models with resolved source layout facts.
use super::{Inline, SourceSpan, is_zero_u16};
use crate::EntryFacts;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Presentation hints retained from roff but optional for semantic outputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutHint {
    /// Signed terminal-cell displacement from the actual parent's content
    /// origin. Compose once before clamping a final display position; negative
    /// children can outdent without erasing their parent's source geometry.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub indent_columns: i32,
    /// Additional displacement for paragraph continuation lines, relative to
    /// the first-line origin. Applies to hard breaks and visual wraps, not to
    /// later sibling blocks; zero preserves ordinary filled flow.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub continuation_indent_columns: i32,
    /// Resolved blank rows owned by this block's leading boundary. Zero means
    /// tight spacing, not inheritance. Producers resolve source/Markdown
    /// defaults and assign each request exactly one consumption point.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub spacing_before_lines: u16,
}

/// A document block capable of preserving nested manual structures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ListKind {
    /// Unordered list with bullets.
    Bullet,
    /// Ordered list with ordinal markers.
    Ordered {
        /// First ordinal, or unknown. Renderers use one for an unknown start
        /// without changing the source fact. Zero and `u64::MAX` are valid.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<u64>,
    },
    /// Marker-free list.
    Plain,
}

// Empty struct variants enforce closure even for bullet/plain during real
// Serde decoding. Unit variants alone can discard unknown tagged fields.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ClosedListKind {
    Bullet {},
    Ordered {
        #[serde(default)]
        start: Option<u64>,
    },
    Plain {},
}

impl<'de> Deserialize<'de> for ListKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ClosedListKind::deserialize(deserializer)? {
            ClosedListKind::Bullet {} => Self::Bullet,
            ClosedListKind::Ordered { start } => Self::Ordered { start },
            ClosedListKind::Plain {} => Self::Plain,
        })
    }
}

impl ListKind {
    /// Displayed ordinal for a zero-based item, saturating at `u64::MAX`.
    /// Non-ordered lists have no ordinal. Unknown starts display from one.
    #[must_use]
    pub fn ordinal(self, index: usize) -> Option<u64> {
        match self {
            Self::Ordered { start } => Some(
                start
                    .unwrap_or(1)
                    .saturating_add(u64::try_from(index).unwrap_or(u64::MAX)),
            ),
            Self::Bullet | Self::Plain => None,
        }
    }

    /// Kind of an excerpt starting at a zero-based original item. The returned
    /// ordered kind records its effective ordinal; the source remains unchanged.
    #[must_use]
    pub fn for_excerpt(self, index: usize) -> Self {
        self.ordinal(index)
            .map_or(self, |start| Self::Ordered { start: Some(start) })
    }
}

/// A list item contains blocks so nested lists and displays remain intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListItem {
    /// Item boundary geometry, independent of content and semantic facts.
    #[serde(default, skip_serializing_if = "ListItemLayout::is_empty")]
    pub layout: ListItemLayout,
    /// Original item span, independent of any removed declaration or first
    /// visible block. Unknown for synthetic content; never an identity key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
    /// Optional semantic facts; these never replace or render the item body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryFacts>,
    /// Arbitrary item content in source order.
    pub blocks: Vec<Block>,
}

/// Resolved list-item boundary. Absent spacing inherits list compactness;
/// explicit zero is tight, while larger requests precede the whole marker and
/// body, including items beginning with displays or other non-paragraph blocks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListItemLayout {
    /// Blank rows before this item; missing/null inherits list compactness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_before_lines: Option<u16>,
}

impl ListItemLayout {
    /// Whether the layout only inherits the containing list's defaults.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.spacing_before_lines.is_none()
    }
}

/// Displayed terms share a description containing arbitrary blocks.
/// Sharing that content does not establish behavioral equivalence of the terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionItem {
    /// Original term-and-description owner span, not its containing list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
    /// Optional source-neutral semantic facts. The original terms and
    /// description remain authoritative regardless of fact validity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryFacts>,
    /// One or more displayed terms sharing this description, not necessarily
    /// equivalent names or interchangeable invocation forms.
    pub terms: Vec<Vec<Inline>>,
    /// Block content describing the terms.
    pub description: Vec<Block>,
    /// Item presentation, independent of any attached semantic facts.
    #[serde(default, skip_serializing_if = "DefinitionLayout::is_empty")]
    pub layout: DefinitionLayout,
}

/// Definition-item presentation. Missing spacing inherits list compactness;
/// explicit zero spacing is a distinct, preserved source request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionLayout {
    /// Render the term on the same line as the first description line (a man(7)
    /// hanging tag that fits the indent) instead of on its own line. Decided
    /// once during lowering so every renderer lays the item out identically.
    #[serde(default, skip_serializing_if = "is_false")]
    pub inline_term: bool,
    /// Description content origin relative to the label origin. Hard and
    /// wrapped continuation lines use this origin even if a long run-in head
    /// forces the first description text further right. The generic default
    /// is four cells; source producers resolve their own widths explicitly.
    #[serde(
        default = "default_definition_indent",
        skip_serializing_if = "is_default_definition_indent"
    )]
    pub body_indent_columns: i32,
    /// Minimum separation after a run-in term, independent of body origin.
    #[serde(
        default = "default_term_gap",
        skip_serializing_if = "is_default_term_gap"
    )]
    pub min_term_gap_columns: u16,
    /// Terminal rows requested before this item when man(7) changes `.PD`.
    /// `None` inherits the containing list's compactness policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing_before_lines: Option<u16>,
}

impl DefinitionLayout {
    /// Whether all presentation choices inherit their existing defaults.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.inline_term
            && self.spacing_before_lines.is_none()
            && self.body_indent_columns == default_definition_indent()
            && self.min_term_gap_columns == default_term_gap()
    }
}

impl Default for DefinitionLayout {
    fn default() -> Self {
        Self {
            inline_term: false,
            body_indent_columns: default_definition_indent(),
            min_term_gap_columns: default_term_gap(),
            spacing_before_lines: None,
        }
    }
}

const fn default_definition_indent() -> i32 {
    4
}
const fn default_term_gap() -> u16 {
    1
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
const fn is_default_definition_indent(value: &i32) -> bool {
    *value == default_definition_indent()
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
const fn is_default_term_gap(value: &u16) -> bool {
    *value == default_term_gap()
}

impl DefinitionItem {
    /// Generic default for authored definitions. Source-specific producers
    /// set [`DefinitionLayout::body_indent_columns`]; consumers must read that
    /// resolved field rather than applying this default a second time.
    pub const DESCRIPTION_INDENT_COLUMNS: u16 = 4;

    /// The first paragraph that can share the term's displayed line.
    ///
    /// Only this paragraph's wrapped lines hang from its first-line text.
    /// Every later block uses [`DefinitionLayout::body_indent_columns`] from the
    /// definition container, independently of label width and inline mode.
    /// Explicit leading spacing or a non-paragraph first block prevents the
    /// inline presentation; it must not be consumed by joining the term.
    #[must_use]
    pub fn inline_description(&self) -> Option<(&[Inline], &LayoutHint)> {
        if !self.layout.inline_term {
            return None;
        }
        match self.description.first()? {
            Block::Paragraph {
                children, layout, ..
            } if layout.spacing_before_lines == 0 => Some((children, layout)),
            _ => None,
        }
    }
}

/// One logical table row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableRow {
    /// Cells in column order. Horizontal spans omit covered cells; vertical
    /// continuations retain empty cells at their logical positions. Use
    /// [`crate::TableGrid`] to place cells without losing horizontal spans.
    pub cells: Vec<TableCell>,
}

/// Block-capable table cell with optional layout information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

impl LayoutHint {
    /// Return whether the hint requests no additional layout behavior.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.indent_columns == 0
            && self.continuation_indent_columns == 0
            && self.spacing_before_lines == 0
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde skip predicates receive a reference.
const fn is_zero_i32(value: &i32) -> bool {
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
