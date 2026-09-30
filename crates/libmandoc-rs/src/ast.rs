//! Owned, renderer-neutral syntax data transferred from a completed libmandoc parse.
//!
//! The private FFI layer retains the completed parser only while it copies
//! shallow borrowed snapshots directly into these types. No C pointer escapes
//! that synchronous transfer, so the data remains valid after the parser
//! session has been released. It deliberately describes source semantics
//! rather than imposing a presentation model on downstream renderers.

/// High-level macro package detected by libmandoc.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacroSet {
    /// No supported semantic macro package was detected.
    None,
    /// The source uses the semantic mdoc(7) macro package.
    Mdoc,
    /// The source uses the traditional man(7) macro package.
    Man,
}

/// Renderer-neutral node role copied from the libmandoc syntax tree.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    /// Synthetic root containing the complete syntax tree.
    Root,
    /// A macro block, such as a section or display.
    Block,
    /// The heading or term portion of a block.
    Head,
    /// The principal content portion of a block.
    Body,
    /// The trailing portion of a block, when the macro defines one.
    Tail,
    /// A leaf-level semantic macro invocation.
    Element,
    /// Literal source text after roff escape processing.
    Text,
    /// A source comment retained by libmandoc.
    Comment,
    /// A tbl(7) table node.
    Table,
    /// An eqn(7) equation node.
    Equation,
}

/// Normalized mdoc section assigned by the pinned parser, independent of the
/// visible heading spelling or its inline styling.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedSection {
    /// No named section.
    None,
    /// NAME.
    Name,
    /// LIBRARY.
    Library,
    /// SYNOPSIS.
    Synopsis,
    /// DESCRIPTION.
    Description,
    /// CONTEXT.
    Context,
    /// IMPLEMENTATION NOTES.
    Implementation,
    /// RETURN VALUES.
    ReturnValues,
    /// ENVIRONMENT.
    Environment,
    /// FILES.
    Files,
    /// EXIT STATUS.
    ExitStatus,
    /// EXAMPLES.
    Examples,
    /// DIAGNOSTICS.
    Diagnostics,
    /// COMPATIBILITY.
    Compatibility,
    /// ERRORS.
    Errors,
    /// SEE ALSO.
    SeeAlso,
    /// STANDARDS.
    Standards,
    /// HISTORY.
    History,
    /// AUTHORS.
    Authors,
    /// CAVEATS.
    Caveats,
    /// BUGS.
    Bugs,
    /// SECURITY.
    Security,
    /// An authored heading outside the standard set.
    Custom,
}

/// Explicit mdoc body close marker. `body_id` identifies the original body
/// within this owned document; it is never a native pointer or source line.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScopeEnd {
    /// Identity of the original body closed by this marker.
    pub body_id: u32,
}

/// Normalized mdoc list behavior copied independently of upstream enum values.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedListKind {
    /// An unordered list whose items carry bullets.
    Bullet,
    /// An unordered list whose items carry dash markers.
    Dash,
    /// An ordered list whose items carry ordinal markers.
    Ordered,
    /// A term-and-description list.
    Definition,
    /// A list laid out as aligned columns.
    Column,
    /// A marker-free list.
    Plain,
}

/// Source-level layout style of an mdoc definition list.
///
/// All variants lower to [`NormalizedListKind::Definition`], but retaining
/// the authored style lets semantic consumers distinguish tagged definitions
/// from diagnostic or hanging presentation without reparsing source text.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefinitionListStyle {
    /// `Bl -tag`: terms occupy a measured tag column.
    Tag,
    /// `Bl -diag`: terms use diagnostic-message presentation.
    Diagnostic,
    /// `Bl -hang`: descriptions hang from their terms.
    Hang,
    /// `Bl -inset`: terms are inset without a fixed tag column.
    Inset,
    /// `Bl -ohang`: descriptions begin below overhanging terms.
    Overhang,
}

/// Whether an mdoc display preserves source line layout.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayKind {
    /// Preserve input line breaks and horizontal whitespace.
    Literal,
    /// Preserve source line breaks without changing the current tab stops.
    Unfilled,
    /// Reflow content as filled prose.
    Filled,
}

/// Normalized font selected by an mdoc `Bf` block.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedFont {
    /// Typographic emphasis.
    Emphasis,
    /// Literal or fixed-width text.
    Literal,
    /// Symbolic text, conventionally rendered in bold.
    Symbolic,
}

/// Explicit author layout mode selected by an mdoc `An` control macro.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorMode {
    /// Render each subsequent author separately.
    Split,
    /// Keep subsequent authors in a continuous group.
    NoSplit,
}

/// Delimiters selected by the obsolete mdoc `Es`/`En` enclosure pair.
///
/// libmandoc resolves the stateful `Es` definition while validating each
/// `En` invocation. Copying that result keeps downstream renderers from
/// replaying formatter state or exposing the non-printing `Es` arguments.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedEnclosure {
    /// Visible opening delimiter.
    pub opening: String,
    /// Visible closing delimiter, when the definition supplied one.
    pub closing: Option<String>,
}

/// Horizontal alignment retained for one parsed tbl(7) cell.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableAlignment {
    /// Align cell content to the left edge.
    Left,
    /// Center cell content horizontally.
    Center,
    /// Align cell content to the right edge.
    Right,
}

/// Effective layout font for one native tbl(7) data cell.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableFont {
    /// Roman text.
    Roman,
    /// Bold text.
    Bold,
    /// Italic text.
    Italic,
    /// Bold italic text.
    BoldItalic,
    /// Constant-width text.
    Code,
    /// Bold constant-width text.
    CodeBold,
    /// Italic constant-width text.
    CodeItalic,
}

/// Effective native tbl cell content after layout-rule precedence.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableCellKind {
    /// Printable data (subject to vertical continuation).
    Text,
    /// Uninitialized data, with no printable payload.
    Empty,
    /// A connecting single horizontal rule.
    HorizontalRule,
    /// A connecting double horizontal rule.
    DoubleHorizontalRule,
    /// An isolated single horizontal rule.
    IsolatedHorizontalRule,
    /// An isolated double horizontal rule.
    IsolatedDoubleHorizontalRule,
}

/// Native kind of one tbl(7) row.
///
/// Empty data rows and whole-row rules both contain no cells, so consumers
/// must not infer this distinction from [`Node::table_cells`].
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TableRowKind {
    /// A data row, including an intentionally empty input line.
    Data,
    /// A whole-row single horizontal rule (`_`).
    HorizontalRule,
    /// A whole-row double horizontal rule (`=`).
    DoubleHorizontalRule,
    /// A rule row authored in the tbl layout, retaining one rule strength
    /// for each logical column.  CVS represents this as an otherwise empty
    /// data span rather than as a whole-row rule span.
    LayoutRule {
        /// Rule strengths in logical column order.
        cells: Vec<TableRuleCellKind>,
    },
}

/// Horizontal rule strength for one cell of a layout-only tbl row.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableRuleCellKind {
    /// A single horizontal rule (`_` or `-`).
    Horizontal,
    /// A double horizontal rule (`=`).
    DoubleHorizontal,
}

/// Owned payload of one cell in a libmandoc table row.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCell {
    /// Native content kind; rule cells suppress even a nonempty text payload.
    pub kind: TableCellKind,
    /// Native tbl layout font, when this cell has a layout slot.
    pub font: Option<TableFont>,
    /// Native cell payload, or `None` for a spanning/empty cell.
    /// Only printable when [`Self::kind`] is [`TableCellKind::Text`] and the
    /// cell is not a vertical continuation.
    pub text: Option<String>,
    /// Parser text before native layout sentinels are normalized. Present
    /// only when such a sentinel occurs; never emit it into public IR text.
    pub native_text: Option<String>,
    /// The cell was written using a multiline tbl(7) `T{`/`T}` text block.
    pub text_block: bool,
    /// Whether the complete native input for this cell bypassed user-defined
    /// or renamed roff macros.
    ///
    /// This is a cell-local execution fact. A source-backed consumer may use
    /// it to enrich a closed inline fragment only when its own operands also
    /// corroborate this native payload; it is not a claim about sibling cells
    /// or the table row as a whole.
    pub source_recovery_safe: bool,
    /// This cell continues a vertical span owned by a cell in an earlier row.
    ///
    /// tbl(7) permits both a `^` layout cell and a literal `\^` data cell for
    /// this purpose. Neither spelling produces printable cell content.
    pub vertical_continuation: bool,
    /// Number of logical columns occupied by the cell.
    pub column_span: u16,
    /// Number of logical rows occupied by the cell.
    pub row_span: u16,
    /// Horizontal alignment requested by tbl(7).
    pub alignment: TableAlignment,
}

impl TableCell {
    /// Text spelling for the roff decoder, including private parser sentinels.
    #[must_use]
    pub fn decoder_text(&self) -> Option<&str> {
        self.native_text.as_deref().or(self.text.as_deref())
    }
}

/// Source and renderer flags needed by a lowering or rendering pass.
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeFlags {
    /// The node was synthesized by libmandoc rather than written explicitly.
    pub generated: bool,
    /// The node ends a sentence according to libmandoc punctuation rules.
    pub sentence_end: bool,
    /// The node must not contribute visible output.
    pub no_print: bool,
    /// The node belongs to a no-fill region that preserves source lines.
    pub no_fill: bool,
    /// Native `NODE_BROKEN`: a later explicit close ended this pending block's
    /// formatting scope while the parser retained its original tree owner.
    pub broken: bool,
    /// libmandoc selected this node as a same-document destination.
    pub deep_link_target: bool,
    /// libmandoc renders a self-link for this destination.
    pub permalink: bool,
    /// This node begins a roff input line (`NODE_LINE`).
    ///
    /// Some man macros keep same-line layout arguments and next-line visible
    /// content in one syntax head, so source-line role is semantic data.
    pub line_start: bool,
    /// This text node is opening punctuation and suppresses spacing after it.
    pub delimiter_open: bool,
    /// This text node is closing punctuation and suppresses spacing before it.
    pub delimiter_close: bool,
    /// This text node ends with the roff `\c` escape and joins the next input
    /// line without an implicit space or line break.
    pub line_continuation: bool,
    /// libmandoc selected synopsis-style presentation for this node.
    ///
    /// Some semantic punctuation is generated only in this context, notably
    /// the terminating semicolon of mdoc `Fn` and `Fo` declarations.
    pub synopsis_pretty: bool,
    /// First data row of a distinct native tbl table, even after leading
    /// rule-only rows. A `T&` layout change does not begin another table.
    pub table_start: bool,
}

/// An owned syntax node with no pointers into the C parser.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    /// Stable identity within one owned parse report.
    pub id: u32,
    /// Structural role of this node in the libmandoc tree.
    pub kind: NodeKind,
    /// Parser-normalized mdoc section, including custom sections.
    pub section: NormalizedSection,
    /// Explicit body close marker, when the node closes another body.
    pub scope_end: Option<ScopeEnd>,
    /// Normalized `%T` title quote rule inside an `Rs` reference.
    pub reference_quotes_title: bool,
    /// Source macro name, without the leading dot, when applicable.
    pub macro_name: Option<String>,
    /// Visible text carried by a text node, with libmandoc's internal break,
    /// discretionary-hyphen, and non-breaking-space sentinels normalized.
    pub text: Option<String>,
    /// Parser text before native layout sentinels are normalized. Present
    /// only when a sentinel occurs; the ordinary `text` stays printable.
    pub native_text: Option<String>,
    /// Canonical same-document tag assigned during libmandoc validation, with
    /// libmandoc's internal text sentinels normalized.
    pub tag: Option<String>,
    /// One-based source line reported by libmandoc, or zero when unavailable.
    pub line: u32,
    /// One-based source column reported by libmandoc, or zero when unavailable.
    pub column: u32,
    /// Per-document generation of executed paragraph/container boundaries.
    ///
    /// Recorded when the native node is allocated, after conditionals and
    /// user macros execute but before empty paragraphs can be removed. Equal
    /// values on adjacent declaration blocks mean no intervening executed
    /// flow boundary. This is not a source line or a persistent identity;
    /// generations are only comparable within the same parse report.
    pub flow_epoch: usize,
    /// Escape character active when the native parser read this tbl(7) row.
    ///
    /// `Some(0)` means `.eo` had disabled escapes. `None` means the node is
    /// not a table row. This is an execution fact, not a source scan.
    pub table_escape: Option<u8>,
    /// Whether native tbl input for this row reached tbl without user macro
    /// expansion or request renaming.
    ///
    /// This is a parser execution fact. It lets a downstream presentation
    /// layer enrich a closed high-level macro fragment only when the original
    /// spelling and the native table payload have the same provenance.
    pub table_source_recovery_safe: bool,
    /// Native tbl row kind, when this node represents a table span.
    pub table_row_kind: Option<TableRowKind>,
    /// Source and renderer flags attached to the node.
    pub flags: NodeFlags,
    /// Normalized list behavior for an mdoc list block.
    pub list_kind: Option<NormalizedListKind>,
    /// Authored mdoc definition-list style, when [`Self::list_kind`] is
    /// [`NormalizedListKind::Definition`].
    pub definition_list_style: Option<DefinitionListStyle>,
    /// Fill behavior for an mdoc display block.
    pub display_kind: Option<DisplayKind>,
    /// Font selected by an mdoc font block.
    pub font: Option<NormalizedFont>,
    /// Author layout mode selected by an mdoc author macro.
    pub author_mode: Option<AuthorMode>,
    /// Stateful delimiters resolved for an mdoc `En` invocation.
    pub enclosure: Option<NormalizedEnclosure>,
    /// Whether the enclosing list requests compact vertical layout.
    pub compact: bool,
    /// Raw normalized display/list offset, including a roff scale suffix.
    pub offset: Option<String>,
    /// Normalized mdoc(7) list width, including its roff scale suffix.
    pub width: Option<String>,
    /// Declared `Bl -column` width strings, in column order. Empty for every
    /// other list kind; `Bl.cols`/`Bl.ncols` in the native tree.
    pub columns: Vec<String>,
    /// Cells copied from a tbl(7) row represented by this node.
    pub table_cells: Vec<TableCell>,
    /// Owned native eqn(7) expression carried by this node.
    pub equation: Option<crate::EquationBox>,
    /// Child nodes in source order.
    pub children: Vec<Self>,
}

impl Node {
    /// Text spelling for the roff decoder, including private parser sentinels.
    #[must_use]
    pub fn decoder_text(&self) -> Option<&str> {
        self.native_text.as_deref().or(self.text.as_deref())
    }
}

/// Metadata copied from a completed libmandoc parse.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Metadata {
    /// Canonical manual title, normally derived from `TH` or `Dt`.
    pub title: Option<String>,
    /// Native manual category such as `1` or `3p`.
    pub section: Option<String>,
    /// Manual volume or collection label.
    pub volume: Option<String>,
    /// Operating-system label declared by the page.
    pub os: Option<String>,
    /// Architecture qualifier declared by the page.
    pub arch: Option<String>,
    /// Primary display name extracted from the NAME section.
    pub name: Option<String>,
    /// Normalized source date when libmandoc recognized it.
    pub date: Option<String>,
    /// Target named by a top-level `.so` alias page.
    pub alias_target: Option<String>,
    /// Whether the parsed source produced a document body.
    pub has_body: bool,
}

/// Complete owned output of the low-level parser, excluding diagnostics.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    /// Macro package selected for the source.
    pub macro_set: MacroSet,
    /// Metadata validated and normalized by libmandoc.
    pub metadata: Metadata,
    /// Root of the owned syntax tree.
    pub root: Node,
}
