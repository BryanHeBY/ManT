//! Block, item and table models with resolved source layout facts.
use super::{EquationExpression, Inline, SourceSpan, is_false, is_zero_u16};
use crate::EntryFacts;
use schemars::JsonSchema;
mod wire;
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
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
        /// Measured declaration content widths in display cells, excluding
        /// the inter-column gap. These are preferred field origins, not fixed
        /// viewport widths. The producer resolves source escapes using its
        /// reading device. Empty selects content-derived table layout.
        /// Consumers share bounded 4/3/1-cell gap placement and preserve every
        /// actual cell when declarations and cells have different lengths.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        column_widths: Vec<u16>,
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

/// Marker behavior of an ordinary list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ListKind {
    /// Unordered list with bullets.
    Bullet,
    /// Unordered list with dash markers.
    Dash,
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
    Dash {},
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
            ClosedListKind::Dash {} => Self::Dash,
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
            Self::Bullet | Self::Dash | Self::Plain => None,
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
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    pub terms: Vec<crate::DefinitionTerm>,
    /// Block content describing the terms.
    pub description: Vec<Block>,
    /// Resolved row and word relation between authoritative head and body.
    /// This source-neutral fact remains independent of presentation geometry.
    #[serde(default, skip_serializing_if = "is_default_relation")]
    pub head_body_relation: HeadBodyRelation,
    /// Item presentation, independent of any attached semantic facts.
    #[serde(default, skip_serializing_if = "DefinitionLayout::is_empty")]
    pub layout: DefinitionLayout,
}

/// Definition-item presentation. Missing spacing inherits list compactness;
/// explicit zero spacing is a distinct, preserved source request.
///
/// The semantic source of truth is [`HeadBodyRelation`]; the column-valued
/// fields are pre-resolved hints for fixed-width terminal presentation and
/// must not drive structure decisions in other renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionLayout {
    /// Preferred first-body alignment when the item shares a separated row.
    /// A joined word seam never acquires padding from this preference.
    #[serde(default, skip_serializing_if = "is_default_alignment")]
    pub body_alignment: DefinitionBodyAlignment,
    /// Description content origin relative to the label origin. Hard and
    /// wrapped continuation lines use this origin even if a long run-in head
    /// forces the first description text further right. A pre-resolved
    /// fixed-width hint (mandoc `offset`, term.c:134-136); the generic
    /// default is four cells.
    #[serde(
        default = "default_definition_indent",
        skip_serializing_if = "is_default_definition_indent"
    )]
    pub body_indent_columns: i32,
    /// Minimum separation after a run-in term, independent of body origin.
    /// A pre-resolved fixed-width hint (mandoc `trailspace`, mdoc_term.c:801-812).
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

/// Source-neutral row and word-boundary facts.
/// Column hints never override a joined word boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HeadBodyRelation {
    /// The body starts on a separate row at its own content origin.
    #[default]
    Separate,
    /// The first body row shares the final head row. Its word seam is
    /// independent from the preferred alignment and later body origins.
    Shared {
        /// Whether consumers may insert separation after the accepted head.
        word_boundary: DefinitionWordBoundary,
    },
}

// A tagged unit variant silently accepts extra fields during Serde decoding.
// Keep the public default ergonomic while closing every wire variant.
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ClosedHeadBodyRelation {
    Separate {},
    Shared {
        word_boundary: DefinitionWordBoundary,
    },
}

impl<'de> Deserialize<'de> for HeadBodyRelation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ClosedHeadBodyRelation::deserialize(deserializer)? {
            ClosedHeadBodyRelation::Separate {} => Self::Separate,
            ClosedHeadBodyRelation::Shared { word_boundary } => Self::Shared { word_boundary },
        })
    }
}

/// Word boundary at a shared definition head/body seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionWordBoundary {
    /// The accepted head and body are adjacent; add no synthetic cells.
    Joined,
    /// Apply the minimum gap and preferred first-body alignment.
    Separated,
}

/// Preferred alignment for only the first body row of a shared definition.
/// Hard/wrapped continuations and later blocks use their own body origins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionBodyAlignment {
    /// Follow the final head row, with only the resolved minimum gap.
    AfterTerm,
    /// Prefer the body content origin, without moving before the head end.
    #[default]
    Indented,
}

impl DefinitionLayout {
    /// Whether all presentation choices inherit their existing defaults.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self.body_alignment, DefinitionBodyAlignment::Indented)
            && self.spacing_before_lines.is_none()
            && self.body_indent_columns == default_definition_indent()
            && self.min_term_gap_columns == default_term_gap()
    }
}

impl HeadBodyRelation {
    /// Share the final head row behind a word separator.
    #[must_use]
    pub const fn separated() -> Self {
        Self::Shared {
            word_boundary: DefinitionWordBoundary::Separated,
        }
    }

    /// Share the final head row with no synthetic word separator or padding.
    #[must_use]
    pub const fn joined() -> Self {
        Self::Shared {
            word_boundary: DefinitionWordBoundary::Joined,
        }
    }

    /// Whether the accepted BODY follows the final HEAD without extra cells.
    #[must_use]
    pub const fn joins_without_separator(self) -> bool {
        matches!(
            self,
            Self::Shared {
                word_boundary: DefinitionWordBoundary::Joined,
                ..
            }
        )
    }

    /// Whether the relation carries the generic default.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Separate)
    }
}

impl From<bool> for HeadBodyRelation {
    fn from(shares_row: bool) -> Self {
        if shares_row {
            Self::separated()
        } else {
            Self::Separate
        }
    }
}

impl Default for DefinitionLayout {
    fn default() -> Self {
        Self {
            body_alignment: DefinitionBodyAlignment::Indented,
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
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
const fn is_default_relation(value: &HeadBodyRelation) -> bool {
    value.is_default()
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
const fn is_default_alignment(value: &DefinitionBodyAlignment) -> bool {
    matches!(value, DefinitionBodyAlignment::Indented)
}

impl DefinitionItem {
    /// Whether the resolved body shares the final term row.
    #[must_use]
    pub const fn inline_term(&self) -> bool {
        !matches!(self.head_body_relation, HeadBodyRelation::Separate)
    }
    /// Generic default for authored definitions. Source-specific producers
    /// set [`DefinitionLayout::body_indent_columns`]; consumers must read that
    /// resolved field rather than applying this default a second time.
    pub const DESCRIPTION_INDENT_COLUMNS: u16 = 4;

    /// The first content row that can share the term's displayed line.
    ///
    /// Only this block's wrapped lines hang from its first-line text.
    /// Every later block uses [`DefinitionLayout::body_indent_columns`] from the
    /// definition container, independently of label width and inline mode.
    /// The producer sets `inline_term` for a literal first block only when
    /// source execution kept the physical HEAD row open. Explicit leading
    /// spacing prevents the inline presentation.
    #[must_use]
    pub fn inline_description(&self) -> Option<(&[Inline], &LayoutHint)> {
        if !self.inline_term() {
            return None;
        }
        match self.description.first()? {
            Block::Paragraph {
                children, layout, ..
            } if layout.spacing_before_lines == 0 => Some((children, layout)),
            Block::Preformatted {
                children, layout, ..
            } if layout.spacing_before_lines == 0 => Some((children, layout)),
            _ => None,
        }
    }

    /// Borrow the first shared description row together with its owner layout.
    #[must_use]
    pub fn inline_description_content(&self) -> Option<(crate::InlineContentRef<'_>, &LayoutHint)> {
        self.inline_description()?;
        match self.description.first()? {
            Block::Paragraph {
                children,
                inline_layout,
                layout,
                ..
            }
            | Block::Preformatted {
                children,
                inline_layout,
                layout,
                ..
            } => Some((
                crate::InlineContentRef {
                    content: children,
                    layout: inline_layout,
                },
                layout,
            )),
            _ => None,
        }
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableCell {
    /// Effective native cell content after tbl layout-rule precedence.
    #[serde(default, skip_serializing_if = "TableCellKind::is_text")]
    pub kind: TableCellKind,
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
