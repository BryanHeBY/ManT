//! Block, item and table models with resolved source layout facts.
use super::{Inline, SourceSpan, is_zero_u16};
use crate::EntryFacts;
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Deserializer, Serialize};
use std::num::NonZeroU64;

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
#[derive(Deserialize, JsonSchema)]
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
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    try_from = "DefinitionLayoutWire"
)]
#[schemars(transform = definition_layout_schema)]
pub struct DefinitionLayout {
    /// Conditional placement of the final open term and first description
    /// paragraph. Readers resolve `Fit` with the shared geometry contract at
    /// their effective width; producers must not collapse it to a boolean.
    #[serde(default, skip_serializing_if = "DefinitionPlacement::is_stacked")]
    pub placement: DefinitionPlacement,
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
    /// Continuation origin for wrapped term rows, relative to the label
    /// origin. This is independent of the description origin: native tag and
    /// hang fields continue at the body edge, while other definitions keep
    /// term continuations at the label edge.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub term_continuation_indent_columns: i32,
    /// Exact fixed-CVS field-fit operands for [`DefinitionPlacement::Fit`],
    /// when the producer obtained them from native execution. Generic `Fit`
    /// producers omit this and use the source-neutral column geometry above.
    #[serde(
        default,
        deserialize_with = "deserialize_present_fit_constraint",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "DefinitionFitConstraint")]
    pub fit_constraint: Option<DefinitionFitConstraint>,
    /// Terminal rows requested before this item when man(7) changes `.PD`.
    /// `None` inherits the containing list's compactness policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing_before_lines: Option<u16>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DefinitionLayoutWire {
    #[serde(default)]
    placement: DefinitionPlacement,
    #[serde(default = "default_definition_indent")]
    body_indent_columns: i32,
    #[serde(default = "default_term_gap")]
    min_term_gap_columns: u16,
    #[serde(default)]
    term_continuation_indent_columns: i32,
    #[serde(default, deserialize_with = "deserialize_present_fit_constraint")]
    #[schemars(with = "DefinitionFitConstraint")]
    fit_constraint: Option<DefinitionFitConstraint>,
    spacing_before_lines: Option<u16>,
}

impl TryFrom<DefinitionLayoutWire> for DefinitionLayout {
    type Error = &'static str;

    fn try_from(value: DefinitionLayoutWire) -> Result<Self, Self::Error> {
        if value.fit_constraint.is_some() && value.placement != DefinitionPlacement::Fit {
            return Err("definition fit constraint requires fit placement");
        }
        Ok(Self {
            placement: value.placement,
            body_indent_columns: value.body_indent_columns,
            min_term_gap_columns: value.min_term_gap_columns,
            term_continuation_indent_columns: value.term_continuation_indent_columns,
            fit_constraint: value.fit_constraint,
            spacing_before_lines: value.spacing_before_lines,
        })
    }
}

fn definition_layout_schema(schema: &mut Schema) {
    schema.insert(
        "allOf".to_owned(),
        schemars::json_schema!({
            "allOf": [{
                "if": { "required": ["fitConstraint"] },
                "then": {
                    "required": ["placement"],
                    "properties": { "placement": { "const": "fit" } }
                }
            }]
        })
        .remove("allOf")
        .expect("literal allOf array"),
    );
}

/// Source-neutral placement policy for a definition label and its body.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionPlacement {
    /// Start the description on a separate physical row.
    #[default]
    Stacked,
    /// Join an eligible first paragraph to the final open label row.
    RunIn,
    /// Join only when the label field and minimum gap fit the effective width.
    Fit,
}

impl DefinitionPlacement {
    #[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
    const fn is_stacked(&self) -> bool {
        matches!(self, Self::Stacked)
    }
}

impl DefinitionLayout {
    /// Whether all presentation choices inherit their existing defaults.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.placement.is_stacked()
            && self.spacing_before_lines.is_none()
            && self.body_indent_columns == default_definition_indent()
            && self.min_term_gap_columns == default_term_gap()
            && self.term_continuation_indent_columns == 0
            && self.fit_constraint.is_none()
    }
}

impl Default for DefinitionLayout {
    fn default() -> Self {
        Self {
            placement: DefinitionPlacement::Stacked,
            body_indent_columns: default_definition_indent(),
            min_term_gap_columns: default_term_gap(),
            term_continuation_indent_columns: 0,
            fit_constraint: None,
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

/// Native fixed-device operands for a responsive `Fit` decision.
///
/// The comparison deliberately remains in native basic units: fixed CVS adds
/// half a cell of tolerance, and `.ta` can make trailing tab width unrelated
/// to any reader-side tab cycle. All five fields are required whenever this
/// object is present; absence means that no native constraint was supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    try_from = "DefinitionFitConstraintWire"
)]
pub struct DefinitionFitConstraint {
    /// Complete logical width used by the native fit comparison, including
    /// significant trailing whitespace when `BRTRSP` is active.
    pub fit_content_basic_units: u64,
    /// Native term field width at the fill decision.
    pub field_basic_units: u64,
    /// Native label-origin phase within one formatter cell. Integer-cell
    /// translations preserve it, so readers can clip the BU field without
    /// assuming that the source origin was cell-aligned.
    pub origin_phase_basic_units: u64,
    /// Width of one canonical formatter cell.
    pub cell_basic_units: NonZeroU64,
    /// Native execution forced the label and body onto separate rows. This
    /// includes an executed word-end break and an effective hard boundary in
    /// the definition head.
    pub forced_separation: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DefinitionFitConstraintWire {
    fit_content_basic_units: u64,
    field_basic_units: u64,
    origin_phase_basic_units: u64,
    #[schemars(range(min = 1))]
    cell_basic_units: u64,
    forced_separation: bool,
}

impl TryFrom<DefinitionFitConstraintWire> for DefinitionFitConstraint {
    type Error = &'static str;

    fn try_from(value: DefinitionFitConstraintWire) -> Result<Self, Self::Error> {
        let cell_basic_units = NonZeroU64::new(value.cell_basic_units)
            .ok_or("definition fit cell width must be positive")?;
        if value.origin_phase_basic_units >= cell_basic_units.get() {
            return Err("definition fit origin phase must be below one cell");
        }
        Ok(Self {
            fit_content_basic_units: value.fit_content_basic_units,
            field_basic_units: value.field_basic_units,
            origin_phase_basic_units: value.origin_phase_basic_units,
            cell_basic_units,
            forced_separation: value.forced_separation,
        })
    }
}

fn deserialize_present_fit_constraint<'de, D>(
    deserializer: D,
) -> Result<Option<DefinitionFitConstraint>, D::Error>
where
    D: Deserializer<'de>,
{
    DefinitionFitConstraint::deserialize(deserializer).map(Some)
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
    pub fn run_in_description(&self) -> Option<(&[Inline], &LayoutHint)> {
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

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(value: &bool) -> bool {
    !*value
}
